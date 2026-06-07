//! Retry logic for transient API failures.
//!
//! Provides [`with_retry`], a generic async retry wrapper that applies
//! fixed-delay backoff to operations returning [`DiffguardError`].

use crate::error::DiffguardError;
use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;

/// Maximum number of retry attempts after the initial call.
const MAX_RETRIES: u32 = 2;

/// Fixed backoff delays for each retry attempt.
const BACKOFF_DELAYS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(2)];

/// Executes an async operation with automatic retry on transient failures.
///
/// Retries up to [`MAX_RETRIES`] times with fixed backoff delays when the
/// operation returns a retryable [`DiffguardError`]. Non-retryable errors
/// are returned immediately.
///
/// # Arguments
///
/// * `operation` — A closure returning a `Future` that produces `Result<T, DiffguardError>`.
///
/// # Examples
///
/// ```ignore
/// let result = with_retry(|| async {
///     client.get(&url).send().await.map_err(|e| /* ... */)
/// }).await?;
/// ```
pub async fn with_retry<T, F, Fut>(operation: F) -> Result<T, DiffguardError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, DiffguardError>>,
{
    let mut last_error = None;

    for attempt in 0..=MAX_RETRIES {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(err) => {
                if !err.is_retryable() || attempt == MAX_RETRIES {
                    return Err(err);
                }
                log::warn!(
                    "Retryable error on attempt {}: {}. Retrying in {:?}...",
                    attempt + 1,
                    err,
                    BACKOFF_DELAYS[attempt as usize]
                );
                sleep(BACKOFF_DELAYS[attempt as usize]).await;
                last_error = Some(err);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| DiffguardError::Config("Max retries exceeded".to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn test_retry_success_on_first_attempt() {
        let counter = AtomicUsize::new(0);
        let result = with_retry(|| async {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok::<_, DiffguardError>("success")
        })
        .await;
        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_eventually_succeeds() {
        let counter = AtomicUsize::new(0);
        let result = with_retry(|| async {
            let count = counter.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(DiffguardError::GitHubApi {
                    status: 503,
                    message: "temporarily unavailable".to_string(),
                })
            } else {
                Ok("success")
            }
        })
        .await;
        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_no_retry_on_non_retryable() {
        let counter = AtomicUsize::new(0);
        let result = with_retry(|| async {
            counter.fetch_add(1, Ordering::SeqCst);
            Err::<(), _>(DiffguardError::GitHubApi {
                status: 404,
                message: "not found".to_string(),
            })
        })
        .await;
        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_on_timeout_status_zero() {
        let counter = AtomicUsize::new(0);
        let result = with_retry(|| async {
            let count = counter.fetch_add(1, Ordering::SeqCst);
            if count < 1 {
                Err(DiffguardError::GitHubApi {
                    status: 0,
                    message: "connection timed out".to_string(),
                })
            } else {
                Ok("success")
            }
        })
        .await;
        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_retry_on_llm_timeout_status_zero() {
        let counter = AtomicUsize::new(0);
        let result = with_retry(|| async {
            let count = counter.fetch_add(1, Ordering::SeqCst);
            if count < 1 {
                Err(DiffguardError::LlmApi {
                    provider: "deepseek".to_string(),
                    status: 0,
                    message: "request timed out".to_string(),
                })
            } else {
                Ok("ok")
            }
        })
        .await;
        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
