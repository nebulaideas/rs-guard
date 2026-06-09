//! Diff fetching from GitHub Pull Requests and local git staging.
//!
//! Provides [`fetch_pr_diff`] for retrieving PR diffs via the GitHub REST API
//! and [`fetch_local_diff`] for reading `git diff --cached` output.

use crate::error::RsGuardError;
use crate::http::{build_github_http_client, github_diff_headers, validate_github_base_url};
use crate::retry::with_retry_simple;
use std::borrow::Cow;

/// Maximum allowed diff size in bytes (100 KB).
const MAX_DIFF_BYTES: usize = 100 * 1024;

/// Maximum allowed diff line count.
const MAX_DIFF_LINES: usize = 1500;

/// HTTP request timeout for diff fetching.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Default number of lines to preserve from the head when chunking a large diff.
///
/// Raised from 50 to 400 to better utilise modern LLM context windows
/// (DeepSeek 64K, Kimi/GPT-4o-mini 128K). The combined default of 800 lines
/// covers the full diff for the vast majority of real PRs while still fitting
/// comfortably within the smallest supported context window.
pub const DEFAULT_CHUNK_HEAD_LINES: usize = 400;

/// Default number of lines to preserve from the tail when chunking a large diff.
pub const DEFAULT_CHUNK_TAIL_LINES: usize = 400;

/// Placeholder inserted in place of chunked middle lines.
const CHUNK_PLACEHOLDER: &str = "\n# ... [diff truncated: {removed} lines omitted] ...\n";

/// Result of a successful diff fetch operation.
#[derive(Debug, Clone)]
#[must_use = "DiffResult should be used for review processing"]
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
/// Returns [`RsGuardError::InvalidDiffContent`] if the content does not
/// appear to be a valid diff.
fn validate_diff_content(content: &str) -> Result<(), RsGuardError> {
    let trimmed = content.trim_start();

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return Err(RsGuardError::InvalidDiffContent);
    }

    let has_diff_markers = content.contains("diff --git")
        || content.contains("@@ ")
        || content.contains("--- a/")
        || content.contains("+++ b/")
        || content.starts_with("diff ")
        || content.starts_with("index ");

    if !has_diff_markers {
        return Err(RsGuardError::InvalidDiffContent);
    }

    Ok(())
}

/// Chunks a large diff by preserving the first N and last N lines.
///
/// When the diff exceeds `head_lines + tail_lines`, the middle section is
/// replaced with a placeholder. Returns the original content unchanged (as a
/// borrowed reference) when no truncation is needed, avoiding allocation.
///
/// Uses [`DEFAULT_CHUNK_HEAD_LINES`] and [`DEFAULT_CHUNK_TAIL_LINES`] as
/// defaults. Pass explicit values via [`chunk_diff_with_params`] when the
/// caller has per-provider or user-configured thresholds.
///
/// # Arguments
///
/// * `content` — The full diff content.
///
/// # Returns
///
/// A tuple of `(chunked_content, was_truncated, removed_lines)`.
pub fn chunk_diff(content: &str) -> (Cow<'_, str>, bool, usize) {
    chunk_diff_with_params(content, DEFAULT_CHUNK_HEAD_LINES, DEFAULT_CHUNK_TAIL_LINES)
}

/// Chunks a large diff with explicit head and tail line counts.
///
/// When the diff exceeds `head_lines + tail_lines`, the middle section is
/// replaced with a placeholder. Returns the original content unchanged (as a
/// borrowed reference) when no truncation is needed, avoiding allocation.
///
/// # Arguments
///
/// * `content` — The full diff content.
/// * `head_lines` — Number of lines to keep from the beginning.
/// * `tail_lines` — Number of lines to keep from the end.
///
/// # Returns
///
/// A tuple of `(chunked_content, was_truncated, removed_lines)`.
pub fn chunk_diff_with_params(
    content: &str,
    head_lines: usize,
    tail_lines: usize,
) -> (Cow<'_, str>, bool, usize) {
    // Detect line ending style from the original content
    let has_crlf = content.contains("\r\n");
    let line_ending = if has_crlf { "\r\n" } else { "\n" };
    let ends_with_newline = content.ends_with('\n') || content.ends_with("\r\n");

    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    let threshold = head_lines + tail_lines;

    if total <= threshold {
        return (Cow::Borrowed(content), false, 0);
    }

    let head = &lines[..head_lines];
    let tail = &lines[total - tail_lines..];
    let removed = total - head_lines - tail_lines;
    let placeholder = CHUNK_PLACEHOLDER.replace("{removed}", &removed.to_string());

    let mut result = String::with_capacity(content.len() / 2);

    // Add head lines with detected line endings
    for line in head {
        result.push_str(line);
        result.push_str(line_ending);
    }

    result.push_str(&placeholder);

    // Add tail lines with detected line endings
    for (i, line) in tail.iter().enumerate() {
        result.push_str(line);
        // Add line ending after each tail line except the last one if original didn't end with newline
        if i < tail.len() - 1 || ends_with_newline {
            result.push_str(line_ending);
        }
    }

    (Cow::Owned(result), true, removed)
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
/// Returns [`RsGuardError::Config`] if `base_url` is not allowlisted,
/// [`RsGuardError::GitHubApi`] on HTTP errors,
/// [`RsGuardError::EmptyDiff`] if the diff is empty,
/// [`RsGuardError::InvalidDiffContent`] if the response is not a valid diff,
/// or [`RsGuardError::DiffTooLarge`] if the diff exceeds size limits.
pub async fn fetch_pr_diff(
    base_url: &str,
    owner: &str,
    repo: &str,
    pr_number: u64,
    token: &str,
) -> Result<DiffResult, RsGuardError> {
    validate_github_base_url(base_url)?;

    let client = build_github_http_client(REQUEST_TIMEOUT)?;

    let url = format!(
        "{}/repos/{}/{}/pulls/{}",
        base_url.trim_end_matches('/'),
        owner,
        repo,
        pr_number
    );
    let headers = github_diff_headers(token)?;

    let response = with_retry_simple(|| async {
        let resp = client
            .get(&url)
            .headers(headers.clone())
            .send()
            .await
            .map_err(|e| {
                let status = e.status().map(|s| s.as_u16()).unwrap_or(0);
                RsGuardError::GitHubApi {
                    status,
                    message: e.to_string(),
                }
            })?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("[failed to read response body: {}]", e));
            return Err(RsGuardError::GitHubApi {
                status: status.as_u16(),
                message: body,
            });
        }

        let body = resp.text().await.map_err(|e| RsGuardError::GitHubApi {
            status: 0,
            message: e.to_string(),
        })?;

        Ok(body)
    })
    .await?;

    if response.is_empty() {
        return Err(RsGuardError::EmptyDiff);
    }

    validate_diff_content(&response)?;

    let size_bytes = response.len();
    let line_count = response.lines().count();

    if size_bytes > MAX_DIFF_BYTES || line_count > MAX_DIFF_LINES {
        return Err(RsGuardError::DiffTooLarge {
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

/// Fetches diff content from a pre-existing file on disk.
///
/// Reads the file, validates that it looks like a diff, and checks size
/// limits. Used when `--diff-file` is specified to skip the GitHub API call.
///
/// # Errors
///
/// Returns [`RsGuardError::Config`] if the file does not exist or cannot
/// be read, [`RsGuardError::EmptyDiff`] if the file is empty,
/// [`RsGuardError::InvalidDiffContent`] if the content does not look
/// like a diff, or [`RsGuardError::DiffTooLarge`] if it exceeds size limits.
pub fn fetch_file_diff(path: &str) -> Result<DiffResult, RsGuardError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| RsGuardError::Config(format!("Failed to read diff file '{}': {}", path, e)))?;

    if content.is_empty() {
        return Err(RsGuardError::EmptyDiff);
    }

    validate_diff_content(&content)?;

    let size_bytes = content.len();
    let line_count = content.lines().count();

    if size_bytes > MAX_DIFF_BYTES || line_count > MAX_DIFF_LINES {
        return Err(RsGuardError::DiffTooLarge {
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

/// Fetches the locally staged diff via `git diff --cached`.
///
/// # Errors
///
/// Returns [`RsGuardError::Io`] if the git command fails,
/// [`RsGuardError::Config`] if `git diff --cached` exits with a non-zero status,
/// [`RsGuardError::EmptyDiff`] if there are no staged changes,
/// [`RsGuardError::InvalidDiffContent`] if the output does not look like a diff,
/// or [`RsGuardError::DiffTooLarge`] if the diff exceeds size limits.
pub fn fetch_local_diff() -> Result<DiffResult, RsGuardError> {
    let output = std::process::Command::new("git")
        .args(["diff", "--cached"])
        .output()
        .map_err(RsGuardError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RsGuardError::Config(format!(
            "git diff --cached failed: {}",
            stderr
        )));
    }

    let content = String::from_utf8_lossy(&output.stdout).to_string();
    build_local_diff_result(content)
}

/// Builds a [`DiffResult`] from already-validated local diff content.
///
/// Extracted from [`fetch_local_diff`] to enable unit testing of content
/// validation without spawning a git process.
///
/// # Errors
///
/// Returns [`RsGuardError::EmptyDiff`], [`RsGuardError::InvalidDiffContent`],
/// or [`RsGuardError::DiffTooLarge`] based on the content.
pub(crate) fn build_local_diff_result(content: String) -> Result<DiffResult, RsGuardError> {
    if content.is_empty() {
        return Err(RsGuardError::EmptyDiff);
    }

    validate_diff_content(&content)?;

    let size_bytes = content.len();
    let line_count = content.lines().count();

    if size_bytes > MAX_DIFF_BYTES || line_count > MAX_DIFF_LINES {
        return Err(RsGuardError::DiffTooLarge {
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

    #[test]
    fn test_chunk_diff_small_diff_unchanged() {
        let content = "line1\nline2\nline3";
        let (result, truncated, _) = chunk_diff(content);
        assert!(!truncated);
        assert_eq!(result.as_ref(), content);
    }

    #[test]
    fn test_chunk_diff_truncates_large_diff() {
        // Use explicit 50/50 params to test truncation behaviour
        // independent of the default constants.
        let lines: Vec<String> = (0..200).map(|i| format!("line {}", i)).collect();
        let content = lines.join("\n");

        let (result, truncated, removed) = chunk_diff_with_params(&content, 50, 50);
        assert!(truncated);
        // 200 - 50 - 50 = 100 removed
        assert_eq!(removed, 100);
        // Result should have head + placeholder + tail
        assert!(result.contains("line 0"));
        assert!(result.contains("line 49"));
        assert!(result.contains("line 150"));
        assert!(result.contains("line 199"));
        assert!(result.contains("100 lines omitted"));
        // Middle lines should NOT be present
        assert!(!result.contains("line 100"));
    }

    #[test]
    fn test_chunk_diff_exactly_at_threshold_unchanged() {
        // 100 lines = exactly threshold with explicit 50+50 params
        let lines: Vec<String> = (0..100).map(|i| format!("line {}", i)).collect();
        let content = lines.join("\n");

        let (result, truncated, _) = chunk_diff_with_params(&content, 50, 50);
        assert!(!truncated);
        assert_eq!(result.as_ref(), content);
    }

    #[test]
    fn test_chunk_diff_preserves_head_and_tail_order() {
        let lines: Vec<String> = (0..150).map(|i| format!("line {}", i)).collect();
        let content = lines.join("\n");

        let (result, truncated, _) = chunk_diff_with_params(&content, 50, 50);
        assert!(truncated);

        // Head lines should appear before the placeholder
        let head_pos = result.find("line 0").unwrap();
        let placeholder_pos = result.find("lines omitted").unwrap();
        let tail_pos = result.find("line 100").unwrap();

        assert!(head_pos < placeholder_pos);
        assert!(placeholder_pos < tail_pos);
    }

    #[test]
    fn test_chunk_diff_preserves_line_endings() {
        // Test with content that has trailing newline, using explicit 50+50 params
        let lines: Vec<String> = (0..150).map(|i| format!("line {}", i)).collect();
        let content = lines.join("\n") + "\n";

        let (result, truncated, _) = chunk_diff_with_params(&content, 50, 50);
        assert!(truncated);
        assert!(result.ends_with('\n'));
    }

    #[test]
    fn test_chunk_diff_preserves_crlf_line_endings() {
        // Test with CRLF line endings (Windows-style), using explicit 50+50 params
        let lines: Vec<String> = (0..150).map(|i| format!("line {}", i)).collect();
        let content = lines.join("\r\n") + "\r\n";

        let (result, truncated, removed) = chunk_diff_with_params(&content, 50, 50);
        assert!(truncated);
        assert_eq!(removed, 50); // 150 - 50 head - 50 tail
                                 // Result should use CRLF line endings
        assert!(result.contains("\r\n"));
        assert!(result.ends_with("\r\n"));
    }

    #[test]
    fn test_chunk_diff_small_crlf_unchanged() {
        let content = "line1\r\nline2\r\nline3\r\n";
        let (result, truncated, _) = chunk_diff(content);
        assert!(!truncated);
        assert_eq!(result.as_ref(), content);
    }

    #[test]
    fn test_chunk_diff_no_allocation_when_small() {
        // Verify that small diffs don't allocate (Cow::Borrowed)
        let content = "line1\nline2\nline3";
        let (result, truncated, _) = chunk_diff(content);
        assert!(!truncated);
        // This would fail to compile if result was not Cow
        assert!(matches!(result, Cow::Borrowed(_)));
    }

    // --- New default-threshold tests (issues #7 & #29) ---

    #[test]
    fn test_chunk_diff_default_does_not_truncate_200_lines() {
        // 200 lines is well below the new 800-line default threshold — should pass unchanged
        let lines: Vec<String> = (0..200).map(|i| format!("line {}", i)).collect();
        let content = lines.join("\n");

        let (result, truncated, removed) = chunk_diff(&content);
        assert!(
            !truncated,
            "200-line diff should not be truncated at new 800-line default"
        );
        assert_eq!(removed, 0);
        assert!(matches!(result, Cow::Borrowed(_)));
    }

    #[test]
    fn test_chunk_diff_default_truncates_at_1000_lines() {
        // 1000 lines exceeds the 800-line default threshold
        let lines: Vec<String> = (0..1000).map(|i| format!("line {}", i)).collect();
        let content = lines.join("\n");

        let (result, truncated, removed) = chunk_diff(&content);
        assert!(
            truncated,
            "1000-line diff should be truncated at 800-line default"
        );
        // 1000 - 400 head - 400 tail = 200 removed
        assert_eq!(removed, 200);
        assert!(result.contains("200 lines omitted"));
    }

    #[test]
    fn test_chunk_diff_default_exactly_at_threshold() {
        // 800 lines = exactly the new default threshold, should NOT truncate
        let lines: Vec<String> = (0..800).map(|i| format!("line {}", i)).collect();
        let content = lines.join("\n");

        let (result, truncated, _) = chunk_diff(&content);
        assert!(
            !truncated,
            "800-line diff at threshold should not be truncated"
        );
        assert!(matches!(result, Cow::Borrowed(_)));
    }

    #[test]
    fn test_chunk_diff_with_params_custom_thresholds() {
        // Verify chunk_diff_with_params honours custom head/tail counts
        let lines: Vec<String> = (0..100).map(|i| format!("line {}", i)).collect();
        let content = lines.join("\n");

        let (result, truncated, removed) = chunk_diff_with_params(&content, 20, 20);
        assert!(truncated);
        assert_eq!(removed, 60); // 100 - 20 - 20
        assert!(result.contains("line 0"));
        assert!(result.contains("line 19"));
        assert!(result.contains("line 80"));
        assert!(result.contains("line 99"));
        assert!(!result.contains("line 50")); // middle omitted
    }

    #[test]
    fn test_fetch_file_diff_valid() {
        let dir = tempfile::tempdir().unwrap();
        let diff_path = dir.path().join("test.diff");
        let diff_content =
            "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1 +1,2 @@\n+line1\n line0";
        std::fs::write(&diff_path, diff_content).unwrap();

        let result = fetch_file_diff(diff_path.to_str().unwrap()).unwrap();
        assert_eq!(result.content, diff_content);
        assert!(result.size_bytes > 0);
        assert!(result.line_count > 0);
    }

    #[test]
    fn test_fetch_file_diff_empty() {
        let dir = tempfile::tempdir().unwrap();
        let diff_path = dir.path().join("empty.diff");
        std::fs::write(&diff_path, "").unwrap();

        let result = fetch_file_diff(diff_path.to_str().unwrap());
        assert!(matches!(result, Err(RsGuardError::EmptyDiff)));
    }

    #[test]
    fn test_fetch_file_diff_invalid_content() {
        let dir = tempfile::tempdir().unwrap();
        let diff_path = dir.path().join("invalid.diff");
        std::fs::write(&diff_path, "not a diff").unwrap();

        let result = fetch_file_diff(diff_path.to_str().unwrap());
        assert!(matches!(result, Err(RsGuardError::InvalidDiffContent)));
    }

    #[test]
    fn test_fetch_file_diff_too_large() {
        let dir = tempfile::tempdir().unwrap();
        let diff_path = dir.path().join("large.diff");
        // Create a valid diff header followed by large content to exceed MAX_DIFF_BYTES (100KB)
        let diff_header = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1 +1,2 @@\n";
        let large_content = format!("{}{}", diff_header, "+line\n".repeat(200 * 1024));
        std::fs::write(&diff_path, &large_content).unwrap();

        let result = fetch_file_diff(diff_path.to_str().unwrap());
        assert!(matches!(result, Err(RsGuardError::DiffTooLarge { .. })));
    }

    #[test]
    fn test_fetch_file_diff_not_found() {
        let result = fetch_file_diff("/nonexistent/path.diff");
        assert!(matches!(result, Err(RsGuardError::Config(_))));
    }

    #[test]
    #[serial_test::serial]
    fn test_fetch_local_diff_requires_git_repo() {
        // Calling fetch_local_diff outside a git repo returns an error
        let dir = tempfile::tempdir().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let result = fetch_local_diff();
        // Depending on environment, git may not be installed (Io error),
        // may return non-zero exit (Config error), or may succeed with
        // empty output (EmptyDiff). All are valid error states.
        assert!(result.is_err(), "expected error, got Ok");

        let _ = std::env::set_current_dir(&original_dir);
    }

    // --- build_local_diff_result unit tests (issue #8) ---

    #[test]
    fn test_build_local_diff_result_rejects_invalid_content() {
        // Non-diff content (e.g. corrupted git output) must be rejected
        let result = build_local_diff_result("this is not a diff at all".to_string());
        assert!(
            matches!(result, Err(RsGuardError::InvalidDiffContent)),
            "expected InvalidDiffContent, got {:?}",
            result
        );
    }

    #[test]
    fn test_build_local_diff_result_rejects_json_content() {
        // JSON error bodies from git should be rejected (e.g. corrupt stdout)
        let result = build_local_diff_result(r#"{"error": "something went wrong"}"#.to_string());
        assert!(
            matches!(result, Err(RsGuardError::InvalidDiffContent)),
            "expected InvalidDiffContent, got {:?}",
            result
        );
    }

    #[test]
    fn test_build_local_diff_result_rejects_empty() {
        let result = build_local_diff_result(String::new());
        assert!(matches!(result, Err(RsGuardError::EmptyDiff)));
    }

    #[test]
    fn test_build_local_diff_result_accepts_valid_diff() {
        let content = "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1,2 @@\n+new line\n old line".to_string();
        let result = build_local_diff_result(content.clone());
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let diff = result.unwrap();
        assert_eq!(diff.content, content);
        assert!(diff.size_bytes > 0);
        assert!(diff.line_count > 0);
    }

    #[test]
    fn test_build_local_diff_result_rejects_too_large() {
        let header = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1 +1,2 @@\n";
        let huge = format!("{}{}", header, "+line\n".repeat(200 * 1024));
        let result = build_local_diff_result(huge);
        assert!(matches!(result, Err(RsGuardError::DiffTooLarge { .. })));
    }
}
