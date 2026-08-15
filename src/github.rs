//! GitHub API interaction for submitting reviews and dismissing stale blockers.
//!
//! Provides [`submit_review`] with automatic permission-fallback to `COMMENT`,
//! and [`dismiss_previous_reviews`] for cleaning up outdated bot reviews.

use crate::error::RsGuardError;
use crate::http::{build_github_http_client, github_headers, validate_github_base_url};
use crate::retry::with_retry_simple;
use crate::verdict::{Finding, FindingSeverity, ReviewState};
use serde_json::json;
use std::collections::HashMap;

/// HTTP request timeout for GitHub API calls.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// HTML comment signature used to identify rs-guard bot reviews.
const BOT_SIGNATURE: &str = "<!-- rs-guard-bot -->";

/// GitHub's maximum character limit for review body.
const GITHUB_REVIEW_BODY_LIMIT: usize = 65536;

/// Submits a review to a GitHub Pull Request without permission fallback.
async fn submit_review_inner(
    base_url: &str,
    owner: &str,
    repo: &str,
    pr_number: u64,
    state: &ReviewState,
    message: &str,
    token: &str,
) -> Result<(), RsGuardError> {
    let client = build_github_http_client(REQUEST_TIMEOUT)?;

    let url = format!(
        "{}/repos/{}/{}/pulls/{}/reviews",
        base_url.trim_end_matches('/'),
        owner,
        repo,
        pr_number
    );

    let headers = github_headers(token)?;

    let body = json!({
        "body": format!("{}\n\n{}", message, BOT_SIGNATURE),
        "event": state.as_github_state(),
    });

    with_retry_simple(|| async {
        let resp = client
            .post(&url)
            .headers(headers.clone())
            .json(&body)
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
            let body_text = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("[failed to read response body: {}]", e));

            // Explicit handling for 422 "body is too long" error
            if status.as_u16() == 422 && body_text.contains("body is too long") {
                return Err(RsGuardError::GitHubApi {
                    status: status.as_u16(),
                    message: "Review body exceeds GitHub's character limit. \
                        Consider using a shorter prompt or chunking the diff."
                        .to_string(),
                });
            }

            return Err(RsGuardError::GitHubApi {
                status: status.as_u16(),
                message: body_text,
            });
        }

        Ok(())
    })
    .await
}

/// Submits a review to a GitHub Pull Request with automatic permission fallback.
///
/// If the initial review state (e.g. `APPROVE` or `REQUEST_CHANGES`) fails due
/// to insufficient permissions (HTTP 403), the function retries with `COMMENT`
/// and prepends `[Bot fallback from {state}]` to the message.
///
/// The `base_url` is validated against an allowlist before any request is made.
///
/// # Arguments
///
/// * `base_url` — GitHub API base URL (e.g. `"https://api.github.com"`).
/// * `owner` — Repository owner.
/// * `repo` — Repository name.
/// * `pr_number` — Pull request number.
/// * `state` — Desired review state.
/// * `message` — Review body text.
/// * `token` — GitHub authentication token.
pub async fn submit_review(
    base_url: &str,
    owner: &str,
    repo: &str,
    pr_number: u64,
    state: ReviewState,
    message: &str,
    token: &str,
) -> Result<(), RsGuardError> {
    validate_github_base_url(base_url)?;

    // Validate review body length before submission
    let full_body = format!("{}\n\n{}", message, BOT_SIGNATURE);
    if full_body.len() > GITHUB_REVIEW_BODY_LIMIT {
        return Err(RsGuardError::GitHubApi {
            status: 0,
            message: format!(
                "Review body exceeds GitHub's character limit ({} chars). \
                Consider using a shorter prompt or chunking the diff.",
                GITHUB_REVIEW_BODY_LIMIT
            ),
        });
    }

    let result =
        submit_review_inner(base_url, owner, repo, pr_number, &state, message, token).await;

    match result {
        Ok(()) => Ok(()),
        Err(err) if err.is_permission_denied() && state != ReviewState::Comment => {
            log::warn!(
                "Permission denied for {}. Falling back to COMMENT...",
                state
            );
            let fallback_msg = format!("[Bot fallback from {}]\n\n{}", state, message);
            submit_review_inner(
                base_url,
                owner,
                repo,
                pr_number,
                &ReviewState::Comment,
                &fallback_msg,
                token,
            )
            .await
        }
        Err(err) => Err(err),
    }
}

/// Dismisses previous rs-guard `CHANGES_REQUESTED` reviews on a Pull Request.
///
/// Queries all reviews on the PR, identifies those with state `CHANGES_REQUESTED`
/// that contain the `BOT_SIGNATURE` marker, and dismisses each one with the
/// message "Outdated — new review submitted".
///
/// Individual dismissal failures are logged as warnings but do not cause this
/// function to return an error.
///
/// The `base_url` is validated against an allowlist before any request is made.
///
/// # Arguments
///
/// * `base_url` — GitHub API base URL (e.g. `"https://api.github.com"`).
/// * `owner` — Repository owner.
/// * `repo` — Repository name.
/// * `pr_number` — Pull request number.
/// * `token` — GitHub authentication token.
pub async fn dismiss_previous_reviews(
    base_url: &str,
    owner: &str,
    repo: &str,
    pr_number: u64,
    token: &str,
) -> Result<(), RsGuardError> {
    validate_github_base_url(base_url)?;

    let client = build_github_http_client(REQUEST_TIMEOUT)?;

    let url = format!(
        "{}/repos/{}/{}/pulls/{}/reviews",
        base_url.trim_end_matches('/'),
        owner,
        repo,
        pr_number
    );

    let headers = github_headers(token)?;

    let reviews: Vec<serde_json::Value> = with_retry_simple(|| async {
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

        resp.json().await.map_err(|e| RsGuardError::GitHubApi {
            status: 0,
            message: e.to_string(),
        })
    })
    .await?;

    for review in reviews {
        let state = review["state"].as_str().unwrap_or("");
        let body = review["body"].as_str().unwrap_or("");
        let review_id = review["id"].as_u64();

        if state == "CHANGES_REQUESTED" && body.contains(BOT_SIGNATURE) {
            if let Some(id) = review_id {
                let dismiss_url = format!(
                    "{}/repos/{}/{}/pulls/{}/reviews/{}/dismissals",
                    base_url.trim_end_matches('/'),
                    owner,
                    repo,
                    pr_number,
                    id
                );

                let dismiss_body = json!({
                    "message": "Outdated — new review submitted",
                });

                if let Err(e) = with_retry_simple(|| async {
                    let resp = client
                        .put(&dismiss_url)
                        .headers(headers.clone())
                        .json(&dismiss_body)
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

                    Ok(())
                })
                .await
                {
                    log::warn!("Failed to dismiss review {}: {}", id, e);
                }
            }
        }
    }

    Ok(())
}

/// Maximum number of inline comments per review (GitHub API limit).
const MAX_INLINE_COMMENTS: usize = 50;

/// Maps (file_path, line_number) to GitHub diff position for inline comments.
#[derive(Debug, Clone, Default)]
pub struct DiffPositionMap {
    positions: HashMap<(String, u32), u32>,
}

impl DiffPositionMap {
    /// Returns the diff position for a given file path and line number.
    pub fn get(&self, path: &str, line: u32) -> Option<u32> {
        self.positions.get(&(path.to_string(), line)).copied()
    }

    /// Returns the number of mappings.
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    /// Returns `true` if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

/// Builds a diff position map from a unified diff string.
///
/// Parses the unified diff to map each `(file_path, line_number)` pair to
/// the 1-based GitHub diff position index. Per GitHub API docs: "The
/// position value equals the number of lines down from the first `@@`
/// hunk header in the file. The line just below the `@@` line is
/// position 1." Position resets to 0 at the start of each new file.
pub fn build_diff_position_map(diff: &str) -> DiffPositionMap {
    let mut positions = HashMap::new();
    let mut current_file: Option<String> = None;
    let mut diff_position: u32 = 0;
    let mut new_line: u32 = 0;
    let mut in_hunk = false;

    for line in diff.lines() {
        // File header — reset per-file state
        if let Some(path) = line.strip_prefix("+++ b/") {
            current_file = Some(path.to_string());
            diff_position = 0;
            new_line = 0;
            in_hunk = false;
            continue;
        }
        if line.starts_with("--- a/") || line.starts_with("--- /dev/null") {
            continue;
        }
        if line.starts_with("diff --git") || line.starts_with("index ") {
            in_hunk = false;
            continue;
        }

        // Hunk header — parse new-file start line, mark in-hunk.
        // Position does NOT reset here — per GitHub docs, it continues
        // across additional hunks until a new file begins.
        if line.starts_with("@@") {
            in_hunk = true;
            if let Some(at_pos) = line.find(" +") {
                let after_plus = &line[at_pos + 2..];
                if let Some(comma_pos) = after_plus.find(',') {
                    if let Ok(n) = after_plus[..comma_pos].parse::<u32>() {
                        new_line = n;
                    }
                } else if let Some(space_pos) = after_plus.find(' ') {
                    if let Ok(n) = after_plus[..space_pos].parse::<u32>() {
                        new_line = n;
                    }
                }
            }
            continue;
        }

        // Skip lines before the first hunk header (file prelude)
        if !in_hunk {
            continue;
        }

        // "\ No newline at end of file" — not a content line, skip
        if line.starts_with("\\ ") {
            continue;
        }

        let file = match &current_file {
            Some(f) => f.clone(),
            None => continue,
        };

        // Position increments for every line after the @@ header
        diff_position += 1;

        if line.starts_with('+') {
            positions.insert((file, new_line), diff_position);
            new_line += 1;
        } else if line.starts_with('-') {
            // Removed lines don't advance new-file line counter
        } else {
            // Context line
            positions.insert((file, new_line), diff_position);
            new_line += 1;
        }
    }

    DiffPositionMap { positions }
}

/// Formats unmappable findings as bullet points for the review body.
///
/// Findings that cannot be mapped to a diff position are appended as prose
/// bullets so no finding is silently dropped.
pub fn format_unmappable_findings(findings: &[Finding]) -> String {
    if findings.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\n### Additional findings (not in diff)\n\n");
    for f in findings {
        out.push_str(&format!(
            "- **[{}] {}:{}** — {}",
            f.severity, f.path, f.line, f.message
        ));
        if let Some(ref s) = f.suggestion {
            out.push_str(&format!(" _(suggestion: {})_", s));
        }
        out.push('\n');
    }
    out
}

/// Severity to emoji for inline comment headers.
fn severity_emoji(severity: &FindingSeverity) -> &'static str {
    match severity {
        FindingSeverity::Critical => "🔴",
        FindingSeverity::Security => "🛡️",
        FindingSeverity::Important => "🟡",
        FindingSeverity::Suggestion => "💡",
    }
}

/// Formats a finding as an inline review comment body.
pub fn format_inline_comment(finding: &Finding) -> String {
    let emoji = severity_emoji(&finding.severity);
    let mut body = format!("{} **[{}]** {}", emoji, finding.severity, finding.message);
    if let Some(ref s) = finding.suggestion {
        body.push_str(&format!("\n\n> 💡 Suggestion: {}", s));
    }
    body
}

/// Submits a GitHub review with inline comments on specific diff positions.
///
/// Falls back to a non-inline review if the inline submission fails
/// (e.g. 422 invalid position after force-push).
#[allow(clippy::too_many_arguments)]
pub async fn submit_inline_review(
    base_url: &str,
    owner: &str,
    repo: &str,
    pr_number: u64,
    state: ReviewState,
    body: &str,
    inline_comments: &[(u32, String, String)],
    token: &str,
) -> Result<(), RsGuardError> {
    validate_github_base_url(base_url)?;

    let client = build_github_http_client(REQUEST_TIMEOUT)?;
    let url = format!(
        "{}/repos/{}/{}/pulls/{}/reviews",
        base_url.trim_end_matches('/'),
        owner,
        repo,
        pr_number
    );
    let headers = github_headers(token)?;

    let comments: Vec<serde_json::Value> = inline_comments
        .iter()
        .take(MAX_INLINE_COMMENTS)
        .map(|(pos, path, comment_body)| {
            json!({
                "path": path,
                "position": pos,
                "body": comment_body,
            })
        })
        .collect();

    let full_body = format!("{}\n\n{}", body, BOT_SIGNATURE);
    let review_body = json!({
        "body": full_body,
        "event": state.as_github_state(),
        "comments": comments,
    });

    let result = with_retry_simple(|| async {
        let resp = client
            .post(&url)
            .headers(headers.clone())
            .json(&review_body)
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
            let body_text = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("[failed to read response body: {}]", e));
            return Err(RsGuardError::GitHubApi {
                status: status.as_u16(),
                message: body_text,
            });
        }
        Ok(())
    })
    .await;

    match result {
        Ok(()) => Ok(()),
        Err(err) if err.is_permission_denied() && state != ReviewState::Comment => {
            log::warn!(
                "Permission denied for inline review {}. Falling back to COMMENT...",
                state
            );
            let fallback_msg = format!("[Bot fallback from {}]\n\n{}", state, body);
            submit_review(
                base_url,
                owner,
                repo,
                pr_number,
                ReviewState::Comment,
                &fallback_msg,
                token,
            )
            .await
        }
        Err(err) => {
            log::warn!(
                "Inline review failed ({}). Falling back to non-inline review.",
                err
            );
            submit_review(base_url, owner, repo, pr_number, state, body, token).await
        }
    }
}

/// Maps a [`ReviewState`] to the GitHub Check Run `conclusion` field.
///
/// | `ReviewState`    | Conclusion  |
/// |------------------|-------------|
/// | `Approve`        | `"success"` |
/// | `RequestChanges` | `"failure"` |
/// | `Comment`        | `"neutral"` |
#[must_use]
pub(crate) fn review_state_to_conclusion(state: &ReviewState) -> &'static str {
    match state {
        ReviewState::Approve => "success",
        ReviewState::RequestChanges => "failure",
        ReviewState::Comment => "neutral",
    }
}

/// Maximum length for a Check Run `output.text` field (GitHub API constraint).
const CHECK_RUN_OUTPUT_TEXT_LIMIT: usize = 65_536;

/// Truncates the Check Run `output.text` to fit GitHub's size limit, appending
/// a truncation notice when content is cut.
fn truncate_check_run_text(text: &str) -> String {
    if text.len() <= CHECK_RUN_OUTPUT_TEXT_LIMIT {
        return text.to_string();
    }
    let notice = "\n\n…[truncated: output exceeds GitHub Check Run text limit]";
    let budget = CHECK_RUN_OUTPUT_TEXT_LIMIT.saturating_sub(notice.len());
    // Cut on a char boundary to avoid splitting a UTF-8 codepoint.
    let mut end = budget;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(CHECK_RUN_OUTPUT_TEXT_LIMIT);
    out.push_str(&text[..end]);
    out.push_str(notice);
    out
}

/// Resolves the commit SHA for a GitHub Check Run.
///
/// Resolution order:
/// 1. `explicit_sha` — from `--check-run-sha` / `RS_GUARD_CHECK_RUN_SHA`.
/// 2. `GITHUB_EVENT_PATH` — for `pull_request`/`pull_request_target` events,
///    reads `pull_request.head.sha` from the event payload JSON. This is the
///    PR head SHA, which is what Check Runs must target (not the synthetic
///    merge commit in `GITHUB_SHA`).
/// 3. `GITHUB_SHA` — fallback for push events and non-GitHub-Actions CI.
///
/// # Errors
///
/// Returns [`RsGuardError::Config`] if no SHA can be resolved, or if
/// `GITHUB_EVENT_PATH` is set but cannot be read or parsed.
pub fn resolve_check_run_sha(explicit_sha: Option<&str>) -> Result<String, RsGuardError> {
    if let Some(sha) = explicit_sha {
        let trimmed = sha.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    if let Ok(path) = std::env::var("GITHUB_EVENT_PATH") {
        if !path.is_empty() {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(head_sha) = payload
                            .get("pull_request")
                            .and_then(|pr| pr.get("head"))
                            .and_then(|head| head.get("sha"))
                            .and_then(|s| s.as_str())
                        {
                            return Ok(head_sha.to_string());
                        }
                    }
                    // Not a pull_request event or missing head.sha — fall through.
                }
                Err(e) => {
                    log::warn!(
                        "GITHUB_EVENT_PATH is set but could not be read ({}); \
                         falling back to GITHUB_SHA",
                        e
                    );
                }
            }
        }
    }

    std::env::var("GITHUB_SHA").map_err(|_| {
        RsGuardError::Config(
            "Could not resolve a commit SHA for the Check Run. Set \
             --check-run-sha / RS_GUARD_CHECK_RUN_SHA, or run in GitHub Actions \
             (GITHUB_EVENT_PATH / GITHUB_SHA)."
                .to_string(),
        )
    })
}

/// Creates a GitHub Check Run for the current commit.
///
/// The Check Run is created with `status: "completed"` and a conclusion
/// derived from the review state. The `head_sha` is resolved via
/// [`resolve_check_run_sha`] (explicit override → `GITHUB_EVENT_PATH`
/// `pull_request.head.sha` → `GITHUB_SHA`).
///
/// A stable `external_id` (derived from the SHA, name, and conclusion) makes
/// retries idempotent: GitHub deduplicates Check Runs with the same
/// `external_id`, so a request that succeeds but whose response is lost will
/// not create a duplicate on retry.
///
/// Check Run creation failure does NOT fail the pipeline — callers should
/// log the error as a warning.
///
/// # Arguments
///
/// * `base_url` — GitHub API base URL (e.g. `"https://api.github.com"`).
/// * `owner` — Repository owner.
/// * `repo` — Repository name.
/// * `name` — Check Run name (e.g. `"rs-guard"`).
/// * `head_sha` — Commit SHA the Check Run targets.
/// * `state` — Review state, mapped to a Check Run conclusion.
/// * `summary` — Short summary of the review verdict.
/// * `text` — Full review text (optional, can be empty; truncated to the
///   GitHub limit).
/// * `token` — GitHub authentication token.
///
/// # Errors
///
/// Returns [`RsGuardError::GitHubApi`] if the API request fails after retries.
/// The `base_url` is validated against an allowlist before any request is made.
#[allow(clippy::too_many_arguments)]
pub async fn create_check_run(
    base_url: &str,
    owner: &str,
    repo: &str,
    name: &str,
    head_sha: &str,
    state: &ReviewState,
    summary: &str,
    text: &str,
    token: &str,
) -> Result<(), RsGuardError> {
    validate_github_base_url(base_url)?;

    let client = build_github_http_client(REQUEST_TIMEOUT)?;

    let url = format!(
        "{}/repos/{}/{}/check-runs",
        base_url.trim_end_matches('/'),
        owner,
        repo
    );

    let headers = github_headers(token)?;

    let conclusion = review_state_to_conclusion(state);
    let external_id = format!("rs-guard:{}:{}", head_sha, conclusion);
    let truncated_text = truncate_check_run_text(text);

    let body = json!({
        "name": name,
        "head_sha": head_sha,
        "status": "completed",
        "conclusion": conclusion,
        "external_id": external_id,
        "output": {
            "title": format!("rs-guard: {}", state),
            "summary": summary,
            "text": truncated_text,
        }
    });

    with_retry_simple(|| async {
        let resp = client
            .post(&url)
            .headers(headers.clone())
            .json(&body)
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
            let body_text = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("[failed to read response body: {}]", e));
            return Err(RsGuardError::GitHubApi {
                status: status.as_u16(),
                message: body_text,
            });
        }

        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_submit_review_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let result = submit_review(
            &mock_server.uri(),
            "owner",
            "repo",
            1,
            ReviewState::Approve,
            "looks good",
            "token",
        )
        .await;

        assert!(result.is_ok());
    }

    /// Regression test for the request body sent to `POST /repos/.../reviews`.
    ///
    /// Asserts that `ReviewState::RequestChanges` is serialised as the request
    /// `event` value `"REQUEST_CHANGES"` (the GitHub REST API spec), not
    /// `"CHANGES_REQUESTED"` (which GitHub returns on the response side and
    /// would cause a 422 with `Variable $event of type PullRequestReviewEvent
    /// was provided invalid value`).
    #[tokio::test]
    async fn test_submit_review_request_changes_sends_correct_event() {
        use wiremock::matchers::body_partial_json;

        let mock_server = MockServer::start().await;

        // Mock that only matches when the request body contains
        // `"event": "REQUEST_CHANGES"`. If the bug regresses, this mock will
        // not match and the test will fail with a 404 from wiremock.
        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .and(body_partial_json(json!({"event": "REQUEST_CHANGES"})))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let result = submit_review(
            &mock_server.uri(),
            "owner",
            "repo",
            1,
            ReviewState::RequestChanges,
            "please fix",
            "token",
        )
        .await;

        assert!(
            result.is_ok(),
            "submit_review(RequestChanges) failed: {:?}",
            result
        );
    }

    /// Companion test for `Approve` — ensures the correct `event` value is sent
    /// and that no regression inverts the value to something like
    /// `"APPROVED"` (the response-side form).
    #[tokio::test]
    async fn test_submit_review_approve_sends_correct_event() {
        use wiremock::matchers::body_partial_json;

        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .and(body_partial_json(json!({"event": "APPROVE"})))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let result = submit_review(
            &mock_server.uri(),
            "owner",
            "repo",
            1,
            ReviewState::Approve,
            "lgtm",
            "token",
        )
        .await;

        assert!(
            result.is_ok(),
            "submit_review(Approve) failed: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_submit_review_retryable_then_success() {
        let mock_server = MockServer::start().await;

        // First request fails with 503
        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable"))
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        // Second request succeeds
        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let result = submit_review(
            &mock_server.uri(),
            "owner",
            "repo",
            1,
            ReviewState::Comment,
            "ok",
            "token",
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_submit_review_permission_fallback_to_comment() {
        let mock_server = MockServer::start().await;

        // First call: APPROVE fails with 403
        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        // Second call: should be COMMENT fallback — verify via the mock server
        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let result = submit_review(
            &mock_server.uri(),
            "owner",
            "repo",
            1,
            ReviewState::Approve,
            "my review",
            "token",
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_submit_review_422_not_permitted_fallback_to_comment() {
        let mock_server = MockServer::start().await;

        // First call: APPROVE fails with 422 "not permitted" (GitHub Actions restriction)
        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .respond_with(
                ResponseTemplate::new(422)
                    .set_body_string(r#"{"message":"Unprocessable Entity","errors":["GitHub Actions is not permitted to approve pull requests."]}"#),
            )
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        // Second call: should be COMMENT fallback
        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let result = submit_review(
            &mock_server.uri(),
            "owner",
            "repo",
            1,
            ReviewState::Approve,
            "my review",
            "token",
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_submit_review_no_fallback_on_permission_denied_for_comment() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
            .mount(&mock_server)
            .await;

        let result = submit_review(
            &mock_server.uri(),
            "owner",
            "repo",
            1,
            ReviewState::Comment,
            "my comment",
            "token",
        )
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().is_permission_denied());
    }

    #[tokio::test]
    async fn test_submit_review_invalid_base_url() {
        let result = submit_review(
            "https://evil.example.com",
            "owner",
            "repo",
            1,
            ReviewState::Comment,
            "msg",
            "token",
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("allowlist"));
    }

    #[tokio::test]
    async fn test_submit_review_invalid_token() {
        let result = submit_review(
            "http://127.0.0.1:1",
            "owner",
            "repo",
            1,
            ReviewState::Comment,
            "msg",
            "token\x00withnull",
        )
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("token"));
    }

    #[tokio::test]
    async fn test_submit_review_body_too_long() {
        let mock_server = MockServer::start().await;

        // Create a message that exceeds the limit
        let long_message = "x".repeat(GITHUB_REVIEW_BODY_LIMIT + 100);

        let result = submit_review(
            &mock_server.uri(),
            "owner",
            "repo",
            1,
            ReviewState::Comment,
            &long_message,
            "token",
        )
        .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("exceeds GitHub's character limit"));
    }

    #[tokio::test]
    async fn test_submit_review_body_at_limit() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        // Create a message that is exactly at the limit (minus signature)
        let message = "x".repeat(GITHUB_REVIEW_BODY_LIMIT - BOT_SIGNATURE.len() - 2);

        let result = submit_review(
            &mock_server.uri(),
            "owner",
            "repo",
            1,
            ReviewState::Comment,
            &message,
            "token",
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_submit_review_422_body_too_long_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .respond_with(ResponseTemplate::new(422).set_body_string(
                r#"{"message":"Unprocessable Entity","errors":["body is too long"]}"#,
            ))
            .mount(&mock_server)
            .await;

        let result = submit_review(
            &mock_server.uri(),
            "owner",
            "repo",
            1,
            ReviewState::Comment,
            "test message",
            "token",
        )
        .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("exceeds GitHub's character limit"));
    }

    #[tokio::test]
    async fn test_dismiss_previous_reviews_no_reviews() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&mock_server)
            .await;

        let result =
            dismiss_previous_reviews(&mock_server.uri(), "owner", "repo", 1, "token").await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dismiss_previous_reviews_dismisses_bot_request_changes() {
        let mock_server = MockServer::start().await;

        let bot_review = json!({
            "id": 42,
            "state": "CHANGES_REQUESTED",
            "body": "Some review\n\n<!-- rs-guard-bot -->"
        });

        Mock::given(method("GET"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([bot_review])))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path_regex(
                r"/repos/owner/repo/pulls/\d+/reviews/\d+/dismissals",
            ))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let result =
            dismiss_previous_reviews(&mock_server.uri(), "owner", "repo", 1, "token").await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dismiss_previous_reviews_skips_non_bot_reviews() {
        let mock_server = MockServer::start().await;

        let human_review = json!({
            "id": 99,
            "state": "CHANGES_REQUESTED",
            "body": "Please fix this issue"
        });

        Mock::given(method("GET"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([human_review])))
            .mount(&mock_server)
            .await;

        let result =
            dismiss_previous_reviews(&mock_server.uri(), "owner", "repo", 1, "token").await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dismiss_previous_reviews_skips_approved_reviews() {
        let mock_server = MockServer::start().await;

        let approved_review = json!({
            "id": 55,
            "state": "APPROVED",
            "body": "<!-- rs-guard-bot -->\nLGTM"
        });

        Mock::given(method("GET"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([approved_review])))
            .mount(&mock_server)
            .await;

        let result =
            dismiss_previous_reviews(&mock_server.uri(), "owner", "repo", 1, "token").await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dismiss_previous_reviews_handles_dismissal_error() {
        let mock_server = MockServer::start().await;

        let bot_review = json!({
            "id": 42,
            "state": "CHANGES_REQUESTED",
            "body": "<!-- rs-guard-bot -->\nReview"
        });

        Mock::given(method("GET"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([bot_review])))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path_regex(
                r"/repos/owner/repo/pulls/\d+/reviews/\d+/dismissals",
            ))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server"))
            .up_to_n_times(4) // retries up to 3 times + initial
            .mount(&mock_server)
            .await;

        let result =
            dismiss_previous_reviews(&mock_server.uri(), "owner", "repo", 1, "token").await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dismiss_previous_reviews_invalid_base_url() {
        let result =
            dismiss_previous_reviews("https://evil.example.com", "owner", "repo", 1, "token").await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("allowlist"));
    }

    #[tokio::test]
    async fn test_dismiss_previous_reviews_handles_get_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/repos/owner/repo/pulls/\d+/reviews"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Server Error"))
            .mount(&mock_server)
            .await;

        let result =
            dismiss_previous_reviews(&mock_server.uri(), "owner", "repo", 1, "token").await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("500"));
    }

    // ── Diff position map tests ─────────────────────────────────────────

    #[test]
    fn test_build_diff_position_map_basic() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
+    println!(\"hello\");
     let x = 1;
 }
";
        let map = build_diff_position_map(diff);
        // Per GitHub: position 1 = first line after @@ header.
        // Line 1 (context "fn main()") -> position 1
        // Line 2 (added "println!") -> position 2
        // Line 3 (context "let x") -> position 3
        // Line 4 (context "}") -> position 4
        assert_eq!(map.get("src/main.rs", 1), Some(1)); // context
        assert_eq!(map.get("src/main.rs", 2), Some(2)); // added line
        assert_eq!(map.get("src/main.rs", 3), Some(3)); // context
        assert_eq!(map.get("src/main.rs", 4), Some(4)); // context
    }

    #[test]
    fn test_build_diff_position_map_multiple_files() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,2 +1,3 @@
+use std::io;
 fn main() {
 }
diff --git a/b.rs b/b.rs
--- a/b.rs
+++ b/b.rs
@@ -1,2 +1,3 @@
+use std::fs;
 fn helper() {
 }
";
        let map = build_diff_position_map(diff);
        // Each file resets position to 1 after its @@ header.
        // a.rs: +use(1) context(2) }(3)
        // b.rs: +use(1) context(2) }(3)
        assert_eq!(map.get("a.rs", 1), Some(1));
        assert_eq!(map.get("a.rs", 2), Some(2));
        assert_eq!(map.get("b.rs", 1), Some(1));
        assert_eq!(map.get("b.rs", 2), Some(2));
        // Non-existent file
        assert_eq!(map.get("c.rs", 1), None);
    }

    #[test]
    fn test_build_diff_position_map_empty_diff() {
        let map = build_diff_position_map("");
        assert!(map.is_empty());
    }

    #[test]
    fn test_diff_position_map_nonexistent_line() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,2 +1,3 @@
+added
 existing
 ";
        let map = build_diff_position_map(diff);
        assert_eq!(map.get("a.rs", 999), None);
    }

    #[test]
    fn test_build_diff_position_map_new_file() {
        // New file: --- /dev/null, +++ b/new.rs
        let diff = "\
diff --git a/new.rs b/new.rs
new file mode 100644
--- /dev/null
+++ b/new.rs
@@ -0,0 +1,3 @@
+fn main() {
+    println!(\"hi\");
+}
";
        let map = build_diff_position_map(diff);
        // All 3 lines are additions starting at line 1
        assert_eq!(map.get("new.rs", 1), Some(1));
        assert_eq!(map.get("new.rs", 2), Some(2));
        assert_eq!(map.get("new.rs", 3), Some(3));
    }

    #[test]
    fn test_build_diff_position_map_deleted_file() {
        // Deleted file: --- a/old.rs, +++ /dev/null — no new-file lines to map
        let diff = "\
diff --git a/old.rs b/old.rs
deleted file mode 100644
--- a/old.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-fn main() {
-}
";
        let map = build_diff_position_map(diff);
        // No +++ b/ path, so current_file stays None — no mappings
        assert!(map.is_empty());
    }

    #[test]
    fn test_build_diff_position_map_multiple_hunks_same_file() {
        // Two hunks in the same file — position continues across hunks
        let diff = "\
diff --git a/lib.rs b/lib.rs
--- a/lib.rs
+++ b/lib.rs
@@ -1,2 +1,3 @@
 fn a() {
+    let x = 1;
 }
@@ -10,2 +11,3 @@
 fn b() {
+    let y = 2;
 }
";
        let map = build_diff_position_map(diff);
        // First hunk: fn a()(1) +let x(2) }(3) — position 1-3
        // Second hunk: fn b()(4) +let y(5) }(6) — position continues
        assert_eq!(map.get("lib.rs", 1), Some(1)); // fn a()
        assert_eq!(map.get("lib.rs", 2), Some(2)); // added let x
        assert_eq!(map.get("lib.rs", 3), Some(3)); // }
        assert_eq!(map.get("lib.rs", 11), Some(4)); // fn b()
        assert_eq!(map.get("lib.rs", 12), Some(5)); // added let y
        assert_eq!(map.get("lib.rs", 13), Some(6)); // }
    }

    #[test]
    fn test_build_diff_position_map_no_newline_marker() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,2 +1,3 @@
+added
 existing
\\ No newline at end of file
";
        let map = build_diff_position_map(diff);
        // Position: +added(1) existing(2) — \ marker is skipped, not position 3
        assert_eq!(map.get("a.rs", 1), Some(1));
        assert_eq!(map.get("a.rs", 2), Some(2));
        // No phantom mapping for the \ marker
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_build_diff_position_map_malformed_hunk_header() {
        // Hunk header with unparseable new-file start — new_line stays at
        // its reset value (0), so mappings would be for line 0 if any
        // content follows. We verify no crash and no valid mappings.
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ garbage @@
+added
";
        let map = build_diff_position_map(diff);
        // Malformed header: new_line not parsed, stays 0.
        // The +added line maps to (a.rs, 0) at position 1.
        assert_eq!(map.get("a.rs", 0), Some(1));
        assert_eq!(map.get("a.rs", 1), None);
    }

    // ── Format tests ────────────────────────────────────────────────────

    #[test]
    fn test_format_unmappable_findings_empty() {
        assert_eq!(format_unmappable_findings(&[]), "");
    }

    #[test]
    fn test_format_unmappable_findings_with_entries() {
        let findings = vec![Finding {
            path: "src/main.rs".into(),
            line: 42,
            severity: FindingSeverity::Critical,
            message: "Null deref".into(),
            suggestion: Some("Add null check".into()),
        }];
        let result = format_unmappable_findings(&findings);
        assert!(result.contains("Additional findings"));
        assert!(result.contains("[Critical]"));
        assert!(result.contains("src/main.rs:42"));
        assert!(result.contains("Null deref"));
        assert!(result.contains("Add null check"));
    }

    #[test]
    fn test_format_inline_comment_critical() {
        let finding = Finding {
            path: "a.rs".into(),
            line: 1,
            severity: FindingSeverity::Critical,
            message: "Buffer overflow".into(),
            suggestion: None,
        };
        let comment = format_inline_comment(&finding);
        assert!(comment.contains("🔴"));
        assert!(comment.contains("[Critical]"));
        assert!(comment.contains("Buffer overflow"));
        assert!(!comment.contains("Suggestion"));
    }

    #[test]
    fn test_format_inline_comment_with_suggestion() {
        let finding = Finding {
            path: "a.rs".into(),
            line: 1,
            severity: FindingSeverity::Suggestion,
            message: "Use const".into(),
            suggestion: Some("Extract to MAGIC_NUMBER".into()),
        };
        let comment = format_inline_comment(&finding);
        assert!(comment.contains("💡"));
        assert!(comment.contains("Extract to MAGIC_NUMBER"));
    }

    // ─── Check Run: conclusion mapping ───────────────────────────────────

    #[test]
    fn test_review_state_to_conclusion_approve() {
        assert_eq!(review_state_to_conclusion(&ReviewState::Approve), "success");
    }

    #[test]
    fn test_review_state_to_conclusion_request_changes() {
        assert_eq!(
            review_state_to_conclusion(&ReviewState::RequestChanges),
            "failure"
        );
    }

    #[test]
    fn test_review_state_to_conclusion_comment() {
        assert_eq!(review_state_to_conclusion(&ReviewState::Comment), "neutral");
    }

    // ─── Check Run: text truncation ──────────────────────────────────────

    #[test]
    fn test_truncate_check_run_text_short_unchanged() {
        let text = "short review text";
        assert_eq!(truncate_check_run_text(text), text);
    }

    #[test]
    fn test_truncate_check_run_text_empty() {
        assert_eq!(truncate_check_run_text(""), "");
    }

    #[test]
    fn test_truncate_check_run_text_exact_limit_unchanged() {
        let text = "x".repeat(CHECK_RUN_OUTPUT_TEXT_LIMIT);
        assert_eq!(
            truncate_check_run_text(&text).len(),
            CHECK_RUN_OUTPUT_TEXT_LIMIT
        );
        assert!(!truncate_check_run_text(&text).contains("truncated"));
    }

    #[test]
    fn test_truncate_check_run_text_over_limit_truncates_with_notice() {
        let text = "x".repeat(CHECK_RUN_OUTPUT_TEXT_LIMIT + 1000);
        let out = truncate_check_run_text(&text);
        assert!(out.len() <= CHECK_RUN_OUTPUT_TEXT_LIMIT, "must fit limit");
        assert!(out.contains("truncated"), "must append truncation notice");
        assert!(out.starts_with('x'));
    }

    #[test]
    fn test_truncate_check_run_text_preserves_char_boundary() {
        // Build a string that ends with a multi-byte char right at the budget.
        let prefix = "x".repeat(CHECK_RUN_OUTPUT_TEXT_LIMIT - 1);
        let mut text = prefix;
        text.push('🦀'); // 4 bytes
        let out = truncate_check_run_text(&text);
        // The result must be valid UTF-8 (String guarantees this) and fit.
        assert!(out.len() <= CHECK_RUN_OUTPUT_TEXT_LIMIT);
        assert!(out.contains("truncated"));
    }

    // ─── Check Run: SHA resolution ───────────────────────────────────────

    /// RAII guard that saves and restores an environment variable.
    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, prev }
        }
        fn remove(key: &'static str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn test_resolve_check_run_sha_explicit_wins() {
        let _g = EnvGuard::remove("GITHUB_SHA");
        let _g2 = EnvGuard::remove("GITHUB_EVENT_PATH");
        let sha = resolve_check_run_sha(Some("explicit-sha-123")).expect("explicit sha should win");
        assert_eq!(sha, "explicit-sha-123");
    }

    #[test]
    fn test_resolve_check_run_sha_explicit_empty_falls_through() {
        let _g = EnvGuard::set("GITHUB_SHA", "fallback-sha");
        let _g2 = EnvGuard::remove("GITHUB_EVENT_PATH");
        let sha = resolve_check_run_sha(Some("   ")).expect("empty explicit falls through");
        assert_eq!(sha, "fallback-sha");
    }

    #[test]
    fn test_resolve_check_run_sha_from_event_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let event_path = dir.path().join("event.json");
        let payload = serde_json::json!({
            "pull_request": { "head": { "sha": "pr-head-sha-abc" } }
        });
        std::fs::write(&event_path, payload.to_string()).expect("write event");
        let _g = EnvGuard::set("GITHUB_EVENT_PATH", event_path.to_str().unwrap());
        let _g2 = EnvGuard::set("GITHUB_SHA", "merge-sha");
        let sha = resolve_check_run_sha(None).expect("should read event path");
        assert_eq!(sha, "pr-head-sha-abc");
    }

    #[test]
    fn test_resolve_check_run_sha_event_path_not_pull_request_falls_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let event_path = dir.path().join("event.json");
        let payload = serde_json::json!({ "ref": "refs/heads/main" });
        std::fs::write(&event_path, payload.to_string()).expect("write event");
        let _g = EnvGuard::set("GITHUB_EVENT_PATH", event_path.to_str().unwrap());
        let _g2 = EnvGuard::set("GITHUB_SHA", "push-sha");
        let sha = resolve_check_run_sha(None).expect("should fall back to GITHUB_SHA");
        assert_eq!(sha, "push-sha");
    }

    #[test]
    fn test_resolve_check_run_sha_no_event_path_uses_github_sha() {
        let _g = EnvGuard::set("GITHUB_SHA", "just-sha");
        let _g2 = EnvGuard::remove("GITHUB_EVENT_PATH");
        let sha = resolve_check_run_sha(None).expect("should use GITHUB_SHA");
        assert_eq!(sha, "just-sha");
    }

    #[test]
    fn test_resolve_check_run_sha_nothing_set_errors() {
        let _g = EnvGuard::remove("GITHUB_SHA");
        let _g2 = EnvGuard::remove("GITHUB_EVENT_PATH");
        let result = resolve_check_run_sha(None);
        assert!(result.is_err(), "should error when no SHA source available");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("commit SHA") || msg.contains("GITHUB_SHA"),
            "error should explain SHA resolution: {}",
            msg
        );
    }

    #[test]
    fn test_resolve_check_run_sha_unreadable_event_path_falls_back() {
        let _g = EnvGuard::set("GITHUB_EVENT_PATH", "/nonexistent/path/event.json");
        let _g2 = EnvGuard::set("GITHUB_SHA", "fallback-sha");
        let sha = resolve_check_run_sha(None).expect("should fall back to GITHUB_SHA");
        assert_eq!(sha, "fallback-sha");
    }

    // ─── Check Run: create_check_run (wiremock) ──────────────────────────

    #[serial_test::serial]
    #[tokio::test]
    async fn test_create_check_run_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/check-runs"))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_json(json!({"id": 1, "name": "rs-guard", "conclusion": "success"})),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let result = create_check_run(
            &mock_server.uri(),
            "owner",
            "repo",
            "rs-guard",
            "abc123def456",
            &ReviewState::Approve,
            "Review passed",
            "Full review text",
            "token",
        )
        .await;

        assert!(
            result.is_ok(),
            "create_check_run should succeed: {:?}",
            result
        );
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_create_check_run_request_changes_conclusion() {
        use wiremock::matchers::body_partial_json;

        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/check-runs"))
            .and(body_partial_json(json!({"conclusion": "failure"})))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(json!({"id": 2, "conclusion": "failure"})),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let result = create_check_run(
            &mock_server.uri(),
            "owner",
            "repo",
            "rs-guard",
            "abc123def456",
            &ReviewState::RequestChanges,
            "Issues found",
            "Full review",
            "token",
        )
        .await;

        assert!(
            result.is_ok(),
            "create_check_run(RequestChanges) failed: {:?}",
            result
        );
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_create_check_run_comment_conclusion() {
        use wiremock::matchers::body_partial_json;

        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/check-runs"))
            .and(body_partial_json(json!({"conclusion": "neutral"})))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(json!({"id": 3, "conclusion": "neutral"})),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let result = create_check_run(
            &mock_server.uri(),
            "owner",
            "repo",
            "rs-guard",
            "abc123def456",
            &ReviewState::Comment,
            "Comments only",
            "Full review",
            "token",
        )
        .await;

        assert!(
            result.is_ok(),
            "create_check_run(Comment) failed: {:?}",
            result
        );
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_create_check_run_sends_correct_payload() {
        use wiremock::matchers::body_partial_json;

        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/check-runs"))
            .and(body_partial_json(json!({
                "name": "rs-guard",
                "head_sha": "abc123def456",
                "status": "completed",
                "conclusion": "success",
                "external_id": "rs-guard:abc123def456:success",
                "output": {
                    "title": "rs-guard: APPROVE",
                    "summary": "All clear",
                    "text": "Details here",
                }
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": 1})))
            .expect(1)
            .mount(&mock_server)
            .await;

        let result = create_check_run(
            &mock_server.uri(),
            "owner",
            "repo",
            "rs-guard",
            "abc123def456",
            &ReviewState::Approve,
            "All clear",
            "Details here",
            "token",
        )
        .await;

        assert!(result.is_ok(), "Payload validation failed: {:?}", result);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_create_check_run_custom_name() {
        use wiremock::matchers::body_partial_json;

        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/check-runs"))
            .and(body_partial_json(json!({"name": "my-custom-check"})))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": 1})))
            .expect(1)
            .mount(&mock_server)
            .await;

        let result = create_check_run(
            &mock_server.uri(),
            "owner",
            "repo",
            "my-custom-check",
            "abc123def456",
            &ReviewState::Approve,
            "summary",
            "text",
            "token",
        )
        .await;

        assert!(result.is_ok(), "Custom name check failed: {:?}", result);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_create_check_run_truncates_long_text() {
        use wiremock::matchers::body_partial_json;

        let mock_server = MockServer::start().await;
        let long_text = "x".repeat(CHECK_RUN_OUTPUT_TEXT_LIMIT + 5000);

        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/check-runs"))
            .and(body_partial_json(json!({"conclusion": "neutral"})))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": 1})))
            .expect(1)
            .mount(&mock_server)
            .await;

        let result = create_check_run(
            &mock_server.uri(),
            "owner",
            "repo",
            "rs-guard",
            "abc123def456",
            &ReviewState::Comment,
            "summary",
            &long_text,
            "token",
        )
        .await;

        assert!(result.is_ok(), "Long text check failed: {:?}", result);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_create_check_run_api_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex(r"/repos/owner/repo/check-runs"))
            .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
            .up_to_n_times(4) // retries up to 3 times + initial
            .mount(&mock_server)
            .await;

        let result = create_check_run(
            &mock_server.uri(),
            "owner",
            "repo",
            "rs-guard",
            "abc123def456",
            &ReviewState::Approve,
            "summary",
            "text",
            "token",
        )
        .await;

        assert!(result.is_err(), "Expected error for 403");
        assert!(result.unwrap_err().to_string().contains("403"));
    }

    #[tokio::test]
    async fn test_create_check_run_invalid_base_url() {
        let result = create_check_run(
            "https://evil.example.com",
            "owner",
            "repo",
            "rs-guard",
            "abc123def456",
            &ReviewState::Approve,
            "summary",
            "text",
            "token",
        )
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("allowlist"));
    }
}
