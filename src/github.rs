//! GitHub API interaction for submitting reviews and dismissing stale blockers.
//!
//! Provides [`submit_review`] with automatic permission-fallback to `COMMENT`,
//! and [`dismiss_previous_reviews`] for cleaning up outdated bot reviews.

use crate::error::DiffguardError;
use crate::http::{github_headers, validate_github_base_url};
use crate::retry::with_retry;
use crate::verdict::ReviewState;
use serde_json::json;

/// HTTP request timeout for GitHub API calls.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// HTML comment signature used to identify diffguard bot reviews.
const BOT_SIGNATURE: &str = "<!-- diffguard-bot -->";

/// Submits a review to a GitHub Pull Request without permission fallback.
async fn submit_review_inner(
    base_url: &str,
    owner: &str,
    repo: &str,
    pr_number: u64,
    state: &ReviewState,
    message: &str,
    token: &str,
) -> Result<(), DiffguardError> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| DiffguardError::Config(format!("Failed to build HTTP client: {}", e)))?;

    let url = format!(
        "{}/repos/{}/{}/pulls/{}/reviews",
        base_url, owner, repo, pr_number
    );

    let headers = github_headers(token)?;

    let body = json!({
        "body": format!("{}\n\n{}", message, BOT_SIGNATURE),
        "event": state.as_github_state(),
    });

    with_retry(|| async {
        let resp = client
            .post(&url)
            .headers(headers.clone())
            .json(&body)
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
            let body_text = resp.text().await.unwrap_or_default();
            return Err(DiffguardError::GitHubApi {
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
) -> Result<(), DiffguardError> {
    validate_github_base_url(base_url)?;

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

/// Dismisses previous diffguard `CHANGES_REQUESTED` reviews on a Pull Request.
///
/// Queries all reviews on the PR, identifies those with state `CHANGES_REQUESTED`
/// that contain the [`BOT_SIGNATURE`] marker, and dismisses each one with the
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
) -> Result<(), DiffguardError> {
    validate_github_base_url(base_url)?;

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| DiffguardError::Config(format!("Failed to build HTTP client: {}", e)))?;

    let url = format!(
        "{}/repos/{}/{}/pulls/{}/reviews",
        base_url, owner, repo, pr_number
    );

    let headers = github_headers(token)?;

    let reviews: Vec<serde_json::Value> = with_retry(|| async {
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

        resp.json().await.map_err(|e| DiffguardError::GitHubApi {
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
                    base_url, owner, repo, pr_number, id
                );

                let dismiss_body = json!({
                    "message": "Outdated — new review submitted",
                });

                if let Err(e) = with_retry(|| async {
                    let resp = client
                        .put(&dismiss_url)
                        .headers(headers.clone())
                        .json(&dismiss_body)
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
