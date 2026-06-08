//! Error types for the diffguard application.
//!
//! Provides a unified [`DiffguardError`] enum covering all failure modes
//! encountered during diff fetching, LLM interaction, verdict parsing,
//! GitHub API communication, and general I/O.

use thiserror::Error;

/// Unified error type for all diffguard operations.
#[derive(Error, Debug)]
pub enum DiffguardError {
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
        "Diff too large: {size_bytes} bytes ({line_count} lines). Maximum is 100KB or 1500 lines."
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

impl DiffguardError {
    /// Returns `true` if this error is transient and the operation should be retried.
    ///
    /// Retryable conditions:
    /// - HTTP 429 (rate limited), 502, 503, or 504
    /// - Status 0 (connection error, timeout, DNS failure)
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            DiffguardError::GitHubApi {
                status: 0 | 429 | 502 | 503 | 504,
                ..
            } | DiffguardError::LlmApi {
                status: 0 | 429 | 502 | 503 | 504,
                ..
            }
        )
    }

    /// Returns `true` if this error indicates insufficient GitHub permissions.
    pub fn is_permission_denied(&self) -> bool {
        match self {
            DiffguardError::GitHubApi { status: 403, .. } => true,
            DiffguardError::GitHubApi {
                status: 422,
                message,
            } => message.to_lowercase().contains("not permitted"),
            DiffguardError::PermissionDenied { .. } => true,
            _ => false,
        }
    }
}
