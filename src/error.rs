//! Error types for the rs-guard application.
//!
//! Provides a unified [`RsGuardError`] enum covering all failure modes
//! encountered during diff fetching, LLM interaction, verdict parsing,
//! GitHub API communication, and general I/O.

use thiserror::Error;

/// Unified error type for all rs-guard operations.
#[derive(Error, Debug)]
pub enum RsGuardError {
    /// GitHub REST API returned an error response.
    #[error("GitHub API error: {status} - {message}")]
    GitHubApi {
        /// HTTP status code returned by GitHub (0 for connection/timeout failures).
        status: u16,
        /// Response body or description of the failure.
        message: String,
    },

    /// LLM provider API returned an error response.
    #[error("LLM API error ({provider}): {status} - {message}")]
    LlmApi {
        /// Name of the LLM provider (e.g. "deepseek").
        provider: String,
        /// HTTP status code returned by the provider (0 for connection/timeout failures).
        status: u16,
        /// Response body or description of the failure.
        message: String,
    },

    /// Failed to parse the verdict metadata block from an LLM response.
    #[error("Failed to parse verdict: {0}")]
    VerdictParse(
        /// Description of the parsing failure.
        String,
    ),

    /// Configuration is invalid or a required value is missing.
    #[error("Configuration error: {0}")]
    Config(
        /// Description of the configuration problem.
        String,
    ),

    /// An I/O operation failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The PR diff exceeds the maximum allowed size.
    #[error(
        "Diff too large: {size_bytes} bytes ({line_count} lines). Maximum is 500 KB or 5,000 lines."
    )]
    DiffTooLarge {
        /// Actual diff size in bytes.
        size_bytes: usize,
        /// Actual diff line count.
        line_count: usize,
    },

    /// The diff contained no content.
    #[error("No diff content found")]
    EmptyDiff,

    /// The diff response did not contain valid diff content (e.g. received JSON error body).
    #[error("Invalid diff content: response does not appear to be a diff")]
    InvalidDiffContent,

    /// The GitHub token lacks permission to perform the requested review action.
    #[error("Permission denied for review state {state}: {message}")]
    PermissionDenied {
        /// The review state that was attempted (e.g. "APPROVE").
        state: String,
        /// Description of the permission failure.
        message: String,
    },
}

/// Marker substring used to detect reasoning-budget exhaustion in
/// `RsGuardError::LlmApi` messages emitted for empty-content responses
/// that carry `reasoning_content`.
///
/// Kept as a single shared constant so the message producer
/// (`llm::resolve_assistant_content`) and the classifier
/// (`is_reasoning_budget_exhausted`) cannot drift apart.
pub(crate) const REASONING_BUDGET_EXHAUSTED_MARKER: &str =
    "reasoning may have consumed the token budget";

/// Prefix for LLM HTTP-client / kernel timeouts. Distinct from body-decode
/// failures so logs never report a serde mismatch as a generic timeout.
pub(crate) const LLM_TIMEOUT_MARKER: &str = "Request timed out";

/// Prefix for permanent LLM response-body decode / JSON-shape failures.
/// These are not transport timeouts and must not be retried as such.
pub(crate) const LLM_DECODE_MARKER: &str = "Failed to decode LLM response body (not a timeout)";

impl RsGuardError {
    /// Returns `true` if this error is transient and the operation should be retried.
    ///
    /// Retryable conditions:
    /// - HTTP 429 (rate limited), 502, 503, or 504
    /// - Status 0 (connection error, timeout, DNS failure)
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            RsGuardError::GitHubApi {
                status: 0 | 429 | 502 | 503 | 504,
                ..
            } | RsGuardError::LlmApi {
                status: 0 | 429 | 502 | 503 | 504,
                ..
            }
        )
    }

    /// Returns `true` if this error indicates a thinking model exhausted the
    /// output token budget on chain-of-thought reasoning before producing
    /// final content.
    ///
    /// This is detected via the marker message emitted for empty `content`
    /// responses that **do** carry `reasoning_content` (DeepSeek/Kimi thinking
    /// models). Callers use this to distinguish "escalate `max_tokens` and
    /// retry" from ordinary transient errors that are blindly retried.
    ///
    /// Empty content **without** any reasoning content does not match: that
    /// shape is treated as a plain transient failure.
    pub fn is_reasoning_budget_exhausted(&self) -> bool {
        matches!(
            self,
            RsGuardError::LlmApi {
                status: 0,
                message,
                ..
            } if message.contains(REASONING_BUDGET_EXHAUSTED_MARKER)
        )
    }

    /// Returns `true` if this is a full-duration LLM HTTP client timeout.
    ///
    /// A timeout has already waited the configured `llm_timeout_secs` budget;
    /// retrying it repeats that wait (often minutes on DeepSeek V4 thinking).
    /// Connection resets and DNS failures stay retryable and do **not** match.
    pub fn is_request_timeout(&self) -> bool {
        matches!(
            self,
            RsGuardError::LlmApi {
                status: 0,
                message,
                ..
            } if message.contains(LLM_TIMEOUT_MARKER)
        )
    }

    /// Returns `true` if this is a permanent LLM response-body decode failure.
    ///
    /// Distinct from [`is_request_timeout`]: a serde/JSON mismatch is not a
    /// timeout even when the Display string used to be status 0.
    pub fn is_response_decode_failure(&self) -> bool {
        matches!(
            self,
            RsGuardError::LlmApi { message, .. } if message.contains(LLM_DECODE_MARKER)
        )
    }

    /// Returns `true` if this error indicates insufficient GitHub permissions.
    pub fn is_permission_denied(&self) -> bool {
        match self {
            RsGuardError::GitHubApi { status: 403, .. } => true,
            RsGuardError::GitHubApi {
                status: 422,
                message,
            } => {
                let msg = message.to_ascii_lowercase();
                msg.contains("not permitted")
                    || msg.contains("own pull request")
                    || msg.contains("approve your own")
                    || msg.contains("request changes on your own")
            }
            RsGuardError::PermissionDenied { .. } => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_retryable_github_429() {
        let err = RsGuardError::GitHubApi {
            status: 429,
            message: "rate limited".to_string(),
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn test_is_retryable_github_502() {
        let err = RsGuardError::GitHubApi {
            status: 502,
            message: "bad gateway".to_string(),
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn test_is_retryable_github_503() {
        let err = RsGuardError::GitHubApi {
            status: 503,
            message: "service unavailable".to_string(),
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn test_is_retryable_github_504() {
        let err = RsGuardError::GitHubApi {
            status: 504,
            message: "gateway timeout".to_string(),
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn test_is_retryable_github_0() {
        let err = RsGuardError::GitHubApi {
            status: 0,
            message: "connection error".to_string(),
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn test_is_retryable_github_404_not_retryable() {
        let err = RsGuardError::GitHubApi {
            status: 404,
            message: "not found".to_string(),
        };
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_is_retryable_github_403_not_retryable() {
        let err = RsGuardError::GitHubApi {
            status: 403,
            message: "forbidden".to_string(),
        };
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_is_retryable_llm_429() {
        let err = RsGuardError::LlmApi {
            provider: "deepseek".to_string(),
            status: 429,
            message: "rate limited".to_string(),
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn test_is_reasoning_budget_exhausted_true() {
        let err = RsGuardError::LlmApi {
            provider: "deepseek".to_string(),
            status: 0,
            message: "Empty assistant content from LLM (reasoning_content: 66002 chars; \
                      reasoning may have consumed the token budget)"
                .to_string(),
        };
        assert!(err.is_reasoning_budget_exhausted());
        assert!(err.is_retryable());
    }

    #[test]
    fn test_is_reasoning_budget_exhausted_false_without_reasoning() {
        let err = RsGuardError::LlmApi {
            provider: "deepseek".to_string(),
            status: 0,
            message: "Empty assistant content from LLM (no reasoning content returned)".to_string(),
        };
        assert!(!err.is_reasoning_budget_exhausted());
    }

    #[test]
    fn test_is_reasoning_budget_exhausted_false_other_errors() {
        let connection = RsGuardError::LlmApi {
            provider: "deepseek".to_string(),
            status: 0,
            message: "connection error".to_string(),
        };
        assert!(!connection.is_reasoning_budget_exhausted());

        let http = RsGuardError::LlmApi {
            provider: "deepseek".to_string(),
            status: 500,
            message: "reasoning may have consumed the token budget".to_string(),
        };
        assert!(!http.is_reasoning_budget_exhausted());
    }

    #[test]
    fn test_is_request_timeout_true() {
        let err = RsGuardError::LlmApi {
            provider: "deepseek".to_string(),
            status: 0,
            message: format!(
                "{LLM_TIMEOUT_MARKER} (HTTP client timeout). This is a transport timeout, not a response-body decode failure."
            ),
        };
        assert!(err.is_request_timeout());
        assert!(err.is_retryable());
        assert!(!err.is_response_decode_failure());
    }

    #[test]
    fn test_is_request_timeout_false_for_decode() {
        let err = RsGuardError::LlmApi {
            provider: "deepseek".to_string(),
            status: 400,
            message: format!(
                "{LLM_DECODE_MARKER}: invalid JSON: expected value at line 1 (body_len=16)"
            ),
        };
        assert!(!err.is_request_timeout());
        assert!(err.is_response_decode_failure());
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_is_request_timeout_false_for_connection_reset() {
        let err = RsGuardError::LlmApi {
            provider: "deepseek".to_string(),
            status: 0,
            message: "connection reset by peer".to_string(),
        };
        assert!(!err.is_request_timeout());
        assert!(!err.is_response_decode_failure());
        assert!(err.is_retryable());
    }

    #[test]
    fn test_is_response_decode_failure_false_for_timeout() {
        let err = RsGuardError::LlmApi {
            provider: "deepseek".to_string(),
            status: 0,
            message: format!("{LLM_TIMEOUT_MARKER} after 240s"),
        };
        assert!(err.is_request_timeout());
        assert!(!err.is_response_decode_failure());
    }

    #[test]
    fn test_is_retryable_llm_0() {
        let err = RsGuardError::LlmApi {
            provider: "deepseek".to_string(),
            status: 0,
            message: "connection error".to_string(),
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn test_is_retryable_config_not_retryable() {
        let err = RsGuardError::Config("bad config".to_string());
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_is_permission_denied_403() {
        let err = RsGuardError::GitHubApi {
            status: 403,
            message: "forbidden".to_string(),
        };
        assert!(err.is_permission_denied());
    }

    #[test]
    fn test_is_permission_denied_422_not_permitted() {
        let err = RsGuardError::GitHubApi {
            status: 422,
            message: "Review not permitted for this user".to_string(),
        };
        assert!(err.is_permission_denied());
    }

    #[test]
    fn test_is_permission_denied_422_own_pull_request() {
        let err = RsGuardError::GitHubApi {
            status: 422,
            message: r#"{"message":"Unprocessable Entity","errors":["Review Can not approve your own pull request"]}"#.to_string(),
        };
        assert!(err.is_permission_denied());
    }

    #[test]
    fn test_is_permission_denied_422_case_insensitive() {
        let err = RsGuardError::GitHubApi {
            status: 422,
            message: "NOT PERMITTED".to_string(),
        };
        assert!(err.is_permission_denied());
    }

    #[test]
    fn test_is_permission_denied_422_other_message() {
        let err = RsGuardError::GitHubApi {
            status: 422,
            message: "Validation failed".to_string(),
        };
        assert!(!err.is_permission_denied());
    }

    #[test]
    fn test_is_permission_denied_explicit_variant() {
        let err = RsGuardError::PermissionDenied {
            state: "APPROVE".to_string(),
            message: "not allowed".to_string(),
        };
        assert!(err.is_permission_denied());
    }

    #[test]
    fn test_is_permission_denied_404_not_denied() {
        let err = RsGuardError::GitHubApi {
            status: 404,
            message: "not found".to_string(),
        };
        assert!(!err.is_permission_denied());
    }
}
