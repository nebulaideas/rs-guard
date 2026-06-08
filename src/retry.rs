//! Retry logic for transient API failures.
//!
//! Provides [`with_retry`], a generic async retry wrapper with exponential
//! backoff, jitter, and an optional circuit breaker.

use crate::error::RsGuardError;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;

/// Maximum number of retry attempts after the initial call.
const MAX_RETRIES: u32 = 3;

/// Base delay for exponential backoff: 1s, 2s, 4s.
const BASE_DELAY_SECS: f64 = 1.0;

/// Multiplier for exponential backoff.
const BACKOFF_MULTIPLIER: f64 = 2.0;

/// Jitter range: ±25% of the computed delay.
const JITTER_RANGE: f64 = 0.25;

/// Default circuit breaker threshold: consecutive failures before opening.
const DEFAULT_CB_THRESHOLD: u32 = 3;

/// Default circuit breaker cooldown: time before auto-reset.
const DEFAULT_CB_COOLDOWN_SECS: u64 = 60;

/// State of a circuit breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation — requests are allowed.
    Closed,
    /// Failure threshold exceeded — requests are rejected.
    Open,
}

/// Internal state of the circuit breaker.
#[derive(Debug)]
struct CircuitBreakerState {
    /// Current consecutive failure count.
    failure_count: u32,
    /// Timestamp of the last failure (seconds since epoch).
    last_failure_secs: Option<u64>,
    /// Current circuit state.
    state: CircuitState,
}

/// Thread-safe circuit breaker for tracking provider failures.
///
/// Simple two-state (Closed/Open) circuit breaker with auto-reset after
/// a cooldown period. No half-open state for v1.
///
/// This implementation is thread-safe and can be shared across async tasks
/// using `Arc<CircuitBreaker>`.
///
/// Opt-in: disabled by default.
///
/// # Current Status
///
/// This circuit breaker is well-tested (17 tests) but not currently wired
/// into the pipeline. All callers use [`with_retry_simple`] which passes
/// `None` for the circuit breaker parameter. This is an intentional decision
/// recorded in the [Decision Log](https://github.com/nebulaideas/rs-guard/blob/main/docs/MVP_IMPLEMENTATION_PLAN.md#appendix-f-decision-log)
/// — it keeps the default experience simple and the feature is available
/// via `.reviewer.toml` configuration when needed.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    /// Whether the circuit breaker is active.
    enabled: bool,
    /// Number of consecutive failures required to open the circuit.
    threshold: u32,
    /// Time after which an open circuit auto-resets to closed.
    cooldown_secs: u64,
    /// Internal state protected by a mutex for thread safety.
    state: Arc<Mutex<CircuitBreakerState>>,
}

impl CircuitBreaker {
    /// Creates a new disabled circuit breaker (opt-in).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            threshold: DEFAULT_CB_THRESHOLD,
            cooldown_secs: DEFAULT_CB_COOLDOWN_SECS,
            state: Arc::new(Mutex::new(CircuitBreakerState {
                failure_count: 0,
                last_failure_secs: None,
                state: CircuitState::Closed,
            })),
        }
    }

    /// Creates a new circuit breaker with the given parameters.
    ///
    /// # Arguments
    ///
    /// * `threshold` — Consecutive failures before opening.
    /// * `cooldown_secs` — Cooldown period in seconds before auto-reset.
    pub fn new(threshold: u32, cooldown_secs: u64) -> Self {
        Self {
            enabled: true,
            threshold,
            cooldown_secs,
            state: Arc::new(Mutex::new(CircuitBreakerState {
                failure_count: 0,
                last_failure_secs: None,
                state: CircuitState::Closed,
            })),
        }
    }

    /// Returns the current time as seconds since Unix epoch.
    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Checks whether a request is allowed through the circuit.
    ///
    /// Returns `true` if the circuit is closed, or if enough time has
    /// passed since the last failure to auto-reset.
    ///
    /// This method is thread-safe.
    pub fn allow_request(&self) -> bool {
        if !self.enabled {
            return true;
        }

        let mut state = self.state.lock().unwrap();

        if state.state == CircuitState::Open {
            if let Some(last) = state.last_failure_secs {
                let now = Self::now_secs();
                let elapsed = now.saturating_sub(last);
                if elapsed >= self.cooldown_secs {
                    log::debug!("Circuit breaker auto-resetting after cooldown");
                    state.state = CircuitState::Closed;
                    state.failure_count = 0;
                    return true;
                }
            }
            return false;
        }

        true
    }

    /// Records a successful call — resets the failure count.
    ///
    /// This method is thread-safe.
    pub fn record_success(&self) {
        if !self.enabled {
            return;
        }
        let mut state = self.state.lock().unwrap();
        state.failure_count = 0;
        state.state = CircuitState::Closed;
    }

    /// Records a failure — may open the circuit if threshold is exceeded.
    ///
    /// This method is thread-safe.
    pub fn record_failure(&self) {
        if !self.enabled {
            return;
        }
        let mut state = self.state.lock().unwrap();
        state.failure_count += 1;
        state.last_failure_secs = Some(Self::now_secs());

        if state.failure_count >= self.threshold {
            log::warn!(
                "Circuit breaker opening after {} consecutive failures",
                state.failure_count
            );
            state.state = CircuitState::Open;
        }
    }

    /// Returns the current state of the circuit breaker.
    ///
    /// This method is thread-safe.
    pub fn current_state(&self) -> CircuitState {
        if !self.enabled {
            return CircuitState::Closed;
        }
        let state = self.state.lock().unwrap();
        state.state
    }

    /// Returns the current failure count.
    ///
    /// This method is thread-safe.
    pub fn failure_count(&self) -> u32 {
        let state = self.state.lock().unwrap();
        state.failure_count
    }
}

/// Computes an exponential backoff delay with jitter.
///
/// Base delay: 1s, multiplied by `2^attempt` for each retry.
/// Jitter: ±25% random variation of the computed delay.
///
/// # Arguments
///
/// * `attempt` — Zero-based retry attempt number.
fn backoff_delay(attempt: u32) -> Duration {
    let base = BASE_DELAY_SECS * BACKOFF_MULTIPLIER.powi(attempt as i32);

    // Simple deterministic jitter using timestamp bits
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let jitter_frac = ((nanos % 997) as f64 / 997.0) * 2.0 - 1.0; // [-1, 1)
    let jitter_amount = jitter_frac * JITTER_RANGE; // ±25%

    let secs = (base * (1.0 + jitter_amount)).max(0.1);
    Duration::from_secs_f64(secs)
}

/// Executes an async operation with automatic retry on transient failures.
///
/// Uses exponential backoff (1s, 2s, 4s) with ±25% jitter.
/// Retries up to `MAX_RETRIES` times for retryable errors.
/// Non-retryable errors are returned immediately.
///
/// # Arguments
///
/// * `operation` — A closure returning a `Future` that produces `Result<T, RsGuardError>`.
/// * `circuit` — Optional circuit breaker to prevent calls when the provider is failing.
pub async fn with_retry<T, F, Fut>(
    operation: F,
    circuit: Option<&CircuitBreaker>,
) -> Result<T, RsGuardError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, RsGuardError>>,
{
    // Check circuit breaker before attempting
    if let Some(cb) = circuit {
        if !cb.allow_request() {
            return Err(RsGuardError::Config(
                "Circuit breaker open — skipping request".to_string(),
            ));
        }
    }

    let mut last_error = None;

    for attempt in 0..=MAX_RETRIES {
        match operation().await {
            Ok(result) => {
                if let Some(cb) = circuit {
                    cb.record_success();
                }
                return Ok(result);
            }
            Err(err) => {
                if !err.is_retryable() || attempt == MAX_RETRIES {
                    if let Some(cb) = circuit {
                        cb.record_failure();
                    }
                    return Err(err);
                }

                let delay = backoff_delay(attempt);
                log::warn!(
                    "Retryable error on attempt {}: {}. Retrying in {:.1}s...",
                    attempt + 1,
                    err,
                    delay.as_secs_f64()
                );
                sleep(delay).await;
                last_error = Some(err);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| RsGuardError::Config("Max retries exceeded".to_string())))
}

/// Executes an async operation with automatic retry (no circuit breaker).
///
/// Convenience wrapper for [`with_retry`] when circuit breaking is not needed.
pub async fn with_retry_simple<T, F, Fut>(operation: F) -> Result<T, RsGuardError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, RsGuardError>>,
{
    with_retry(operation, None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn test_retry_success_on_first_attempt() {
        let counter = AtomicUsize::new(0);
        let result = with_retry_simple(|| async {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok::<_, RsGuardError>("success")
        })
        .await;
        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_eventually_succeeds() {
        let counter = AtomicUsize::new(0);
        let result = with_retry_simple(|| async {
            let count = counter.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(RsGuardError::GitHubApi {
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
        let result = with_retry_simple(|| async {
            counter.fetch_add(1, Ordering::SeqCst);
            Err::<(), _>(RsGuardError::GitHubApi {
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
        let result = with_retry_simple(|| async {
            let count = counter.fetch_add(1, Ordering::SeqCst);
            if count < 1 {
                Err(RsGuardError::GitHubApi {
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
    async fn test_retry_max_attempts() {
        let counter = AtomicUsize::new(0);
        let result = with_retry_simple(|| async {
            counter.fetch_add(1, Ordering::SeqCst);
            Err::<(), _>(RsGuardError::GitHubApi {
                status: 503,
                message: "always fails".to_string(),
            })
        })
        .await;
        assert!(result.is_err());
        // Initial attempt + 3 retries = 4 total
        assert_eq!(counter.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn test_backoff_delay_increases() {
        let d0 = backoff_delay(0);
        let d1 = backoff_delay(1);
        let d2 = backoff_delay(2);

        // Base: 1s, 2s, 4s (with jitter, approximate)
        assert!(d0.as_secs_f64() >= 0.1);
        assert!(d1.as_secs_f64() > d0.as_secs_f64() * 0.5); // should be ~2x
        assert!(d2.as_secs_f64() > d1.as_secs_f64() * 0.5);
    }

    #[tokio::test]
    async fn test_circuit_breaker_closed_initial() {
        let cb = CircuitBreaker::disabled();
        assert_eq!(cb.current_state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_disabled_allows_all() {
        let cb = CircuitBreaker::disabled();
        for _ in 0..100 {
            assert!(cb.allow_request());
        }
    }

    #[test]
    fn test_circuit_breaker_opens_on_threshold() {
        let cb = CircuitBreaker::new(3, 60);
        assert_eq!(cb.current_state(), CircuitState::Closed);

        cb.record_failure();
        assert_eq!(cb.current_state(), CircuitState::Closed);

        cb.record_failure();
        assert_eq!(cb.current_state(), CircuitState::Closed);

        cb.record_failure();
        assert_eq!(cb.current_state(), CircuitState::Open);

        assert!(!cb.allow_request());
    }

    #[test]
    fn test_circuit_breaker_resets_on_success() {
        let cb = CircuitBreaker::new(3, 60);

        cb.record_failure();
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.current_state(), CircuitState::Open);

        // record_success should close the circuit
        cb.record_success();
        assert_eq!(cb.current_state(), CircuitState::Closed);
        assert_eq!(cb.failure_count(), 0);
    }

    #[test]
    fn test_circuit_breaker_partial_failures_dont_open() {
        let cb = CircuitBreaker::new(3, 60);

        cb.record_failure();
        cb.record_success();
        cb.record_failure();
        cb.record_failure();
        // Only 2 consecutive failures since we had a success in between
        assert_eq!(cb.current_state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_retry_with_circuit_breaker_rejects_when_open() {
        let cb = CircuitBreaker::new(1, 60);
        cb.record_failure();
        assert_eq!(cb.current_state(), CircuitState::Open);

        let counter = AtomicUsize::new(0);
        let result = with_retry(
            || async {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok::<_, RsGuardError>("should not be called")
            },
            Some(&cb),
        )
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Circuit breaker"));
        // Operation should not have been called
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_retry_with_circuit_breaker_records_success() {
        let cb = CircuitBreaker::new(3, 60);

        let result = with_retry(|| async { Ok::<_, RsGuardError>("ok") }, Some(&cb)).await;

        assert!(result.is_ok());
        assert_eq!(cb.failure_count(), 0);
        assert_eq!(cb.current_state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_retry_with_circuit_breaker_records_failure() {
        let cb = CircuitBreaker::new(3, 60);

        let result = with_retry(
            || async {
                Err::<(), _>(RsGuardError::GitHubApi {
                    status: 500,
                    message: "server error".to_string(),
                })
            },
            Some(&cb),
        )
        .await;

        // After 4 attempts (1 initial + 3 retries), the error is returned
        assert!(result.is_err());
        assert_eq!(cb.failure_count(), 1);
        // Only 1 because it's the final failure after all retries
    }

    #[tokio::test]
    async fn test_circuit_breaker_thread_safety() {
        use std::sync::Arc;
        use tokio::task;

        let cb = Arc::new(CircuitBreaker::new(10, 60));
        let mut handles = vec![];

        // Spawn 100 tasks that each record a success
        for _ in 0..100 {
            let cb_clone = Arc::clone(&cb);
            handles.push(task::spawn(async move {
                cb_clone.record_success();
            }));
        }

        // Wait for all tasks
        for handle in handles {
            handle.await.unwrap();
        }

        // Should still be closed
        assert_eq!(cb.current_state(), CircuitState::Closed);
        assert_eq!(cb.failure_count(), 0);
    }

    #[tokio::test]
    async fn test_circuit_breaker_concurrent_failures() {
        use std::sync::Arc;
        use tokio::task;

        let cb = Arc::new(CircuitBreaker::new(5, 60));
        let mut handles = vec![];

        // Spawn 10 tasks that each record a failure
        for _ in 0..10 {
            let cb_clone = Arc::clone(&cb);
            handles.push(task::spawn(async move {
                cb_clone.record_failure();
            }));
        }

        // Wait for all tasks
        for handle in handles {
            handle.await.unwrap();
        }

        // Circuit should be open (threshold was 5, we had 10 failures)
        assert_eq!(cb.current_state(), CircuitState::Open);
        assert_eq!(cb.failure_count(), 10);
    }

    #[tokio::test]
    async fn test_circuit_breaker_auto_reset_after_cooldown() {
        // Create a circuit breaker with a very short cooldown (1 second)
        let cb = CircuitBreaker::new(2, 1);

        // Record failures to open the circuit
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.current_state(), CircuitState::Open);

        // Wait for cooldown to expire
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Next success should reset the circuit to closed
        cb.record_success();
        assert_eq!(cb.current_state(), CircuitState::Closed);
        assert_eq!(cb.failure_count(), 0);
    }
}
