//! GitHub API interaction for submitting reviews and dismissing stale blockers.
//!
//! Provides [`submit_review`] with automatic permission-fallback to `COMMENT`,
//! and [`dismiss_previous_reviews`] for cleaning up outdated bot reviews.

use crate::error::RsGuardError;
use crate::http::{build_github_http_client, github_headers, validate_github_base_url};
use crate::retry::with_retry_simple;
use crate::verdict::ReviewState;
use serde_json::json;

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
}
