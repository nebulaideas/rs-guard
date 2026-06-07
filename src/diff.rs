//! Diff fetching from GitHub Pull Requests and local git staging.
//!
//! Provides [`fetch_pr_diff`] for retrieving PR diffs via the GitHub REST API
//! and [`fetch_local_diff`] for reading `git diff --cached` output.

use crate::error::DiffguardError;
use crate::http::{github_diff_headers, validate_github_base_url};
use crate::retry::with_retry;

/// Maximum allowed diff size in bytes (100 KB).
const MAX_DIFF_BYTES: usize = 100 * 1024;

/// Maximum allowed diff line count.
const MAX_DIFF_LINES: usize = 1500;

/// HTTP request timeout for diff fetching.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Result of a successful diff fetch operation.
#[derive(Debug, Clone)]
pub struct DiffResult {
    /// The raw diff content.
    pub content: String,
    /// Size of the diff in bytes.
    pub size_bytes: usize,
    /// Number of lines in the diff.
    pub line_count: usize,
}

/// Validates that the response body looks like a diff and not a JSON error.
///
/// Checks for common diff markers (`diff --git`, `@@`, `---`, `+++`) and
/// rejects responses that appear to be JSON error bodies from the API.
///
/// # Errors
///
/// Returns [`DiffguardError::InvalidDiffContent`] if the content does not
/// appear to be a valid diff.
fn validate_diff_content(content: &str) -> Result<(), DiffguardError> {
    let trimmed = content.trim_start();

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return Err(DiffguardError::InvalidDiffContent);
    }

    let has_diff_markers = content.contains("diff --git")
        || content.contains("@@ ")
        || content.contains("--- a/")
        || content.contains("+++ b/")
        || content.starts_with("diff ")
        || content.starts_with("index ");

    if !has_diff_markers {
        return Err(DiffguardError::InvalidDiffContent);
    }

    Ok(())
}

/// Fetches the diff for a GitHub Pull Request.
///
/// Sends a GET request to the GitHub API with the `application/vnd.github.v3.diff`
/// accept header. Automatically retries on transient failures (429, 5xx, timeouts).
///
/// The `base_url` is validated against an allowlist before any request is made,
/// preventing `Authorization` headers from being sent to untrusted hosts.
///
/// # Arguments
///
/// * `base_url` — GitHub API base URL (e.g. `"https://api.github.com"`).
/// * `owner` — Repository owner.
/// * `repo` — Repository name.
/// * `pr_number` — Pull request number.
/// * `token` — GitHub authentication token.
///
/// # Errors
///
/// Returns [`DiffguardError::Config`] if `base_url` is not allowlisted,
/// [`DiffguardError::GitHubApi`] on HTTP errors,
/// [`DiffguardError::EmptyDiff`] if the diff is empty,
/// [`DiffguardError::InvalidDiffContent`] if the response is not a valid diff,
/// or [`DiffguardError::DiffTooLarge`] if the diff exceeds size limits.
pub async fn fetch_pr_diff(
    base_url: &str,
    owner: &str,
    repo: &str,
    pr_number: u64,
    token: &str,
) -> Result<DiffResult, DiffguardError> {
    validate_github_base_url(base_url)?;

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| DiffguardError::Config(format!("Failed to build HTTP client: {}", e)))?;

    let url = format!("{}/repos/{}/{}/pulls/{}", base_url, owner, repo, pr_number);
    let headers = github_diff_headers(token)?;

    let response = with_retry(|| async {
        let resp = client
            .get(&url)
            .headers(headers.clone())
            .send()
            .await
            .map_err(|e| {
                let status = e.status().map(|s| s.as_u16()).unwrap_or(0);
                DiffguardError::GitHubApi {
                    status,
                    message: e.to_string(),
                }
            })?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(DiffguardError::GitHubApi {
                status: status.as_u16(),
                message: body,
            });
        }

        let body = resp.text().await.map_err(|e| DiffguardError::GitHubApi {
            status: 0,
            message: e.to_string(),
        })?;

        Ok(body)
    })
    .await?;

    if response.is_empty() {
        return Err(DiffguardError::EmptyDiff);
    }

    validate_diff_content(&response)?;

    let size_bytes = response.len();
    let line_count = response.lines().count();

    if size_bytes > MAX_DIFF_BYTES || line_count > MAX_DIFF_LINES {
        return Err(DiffguardError::DiffTooLarge {
            size_bytes,
            line_count,
        });
    }

    Ok(DiffResult {
        content: response,
        size_bytes,
        line_count,
    })
}

/// Fetches the locally staged diff via `git diff --cached`.
///
/// # Errors
///
/// Returns [`DiffguardError::Io`] if the git command fails,
/// [`DiffguardError::EmptyDiff`] if there are no staged changes,
/// or [`DiffguardError::DiffTooLarge`] if the diff exceeds size limits.
pub fn fetch_local_diff() -> Result<DiffResult, DiffguardError> {
    let output = std::process::Command::new("git")
        .args(["diff", "--cached"])
        .output()
        .map_err(DiffguardError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DiffguardError::Config(format!(
            "git diff --cached failed: {}",
            stderr
        )));
    }

    let content = String::from_utf8_lossy(&output.stdout).to_string();

    if content.is_empty() {
        return Err(DiffguardError::EmptyDiff);
    }

    let size_bytes = content.len();
    let line_count = content.lines().count();

    if size_bytes > MAX_DIFF_BYTES || line_count > MAX_DIFF_LINES {
        return Err(DiffguardError::DiffTooLarge {
            size_bytes,
            line_count,
        });
    }

    Ok(DiffResult {
        content,
        size_bytes,
        line_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_fetch_pr_diff_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls/42"))
            .and(header("Accept", "application/vnd.github.v3.diff"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "diff --git a/file.rs b/file.rs\n--- a/file.rs\n+++ b/file.rs\n@@ -1,2 +1,3 @@\n+line",
            ))
            .mount(&mock_server)
            .await;

        let result = fetch_pr_diff(
            &mock_server.uri(),
            "test-owner",
            "test-repo",
            42,
            "test-token",
        )
        .await;

        assert!(result.is_ok());
        let diff = result.unwrap();
        assert!(diff.content.contains("diff --git"));
        assert!(diff.line_count > 0);
    }

    #[tokio::test]
    async fn test_fetch_pr_diff_not_found() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls/999"))
            .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
            .mount(&mock_server)
            .await;

        let result = fetch_pr_diff(
            &mock_server.uri(),
            "test-owner",
            "test-repo",
            999,
            "test-token",
        )
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("404"));
    }

    #[tokio::test]
    async fn test_fetch_pr_diff_rejects_json_response() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls/42"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"message": "Not Found", "documentation_url": "..." }"#),
            )
            .mount(&mock_server)
            .await;

        let result = fetch_pr_diff(
            &mock_server.uri(),
            "test-owner",
            "test-repo",
            42,
            "test-token",
        )
        .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not appear to be a diff"));
    }

    #[test]
    fn test_validate_diff_content_valid() {
        assert!(validate_diff_content("diff --git a/f.rs b/f.rs\n").is_ok());
        assert!(validate_diff_content("@@ -1,3 +1,4 @@\n").is_ok());
        assert!(validate_diff_content("--- a/f.rs\n+++ b/f.rs\n").is_ok());
        assert!(validate_diff_content("index abc123..def456 100644\n").is_ok());
    }

    #[test]
    fn test_validate_diff_content_json() {
        assert!(validate_diff_content(r#"{"message": "error"}"#).is_err());
        assert!(validate_diff_content(r#"[{"error": true}]"#).is_err());
    }

    #[test]
    fn test_validate_diff_content_no_markers() {
        assert!(validate_diff_content("just some random text\nwith no diff markers").is_err());
    }
}
