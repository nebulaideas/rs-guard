//! Secret redaction and content filtering.
//!
//! Provides [`redact_secrets`] for scrubbing sensitive data from text before
//! it is logged, stored in artifacts, or posted to GitHub. Detects common
//! secret patterns including Bearer tokens, API keys, private keys, and
//! base64-encoded credentials.

use regex::Regex;
use std::sync::LazyLock;

/// Redaction placeholder inserted in place of detected secrets.
const REDACTED: &str = "[REDACTED]";

/// Compiled regex patterns for detecting secrets in text.
static SECRET_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?i)bearer\s+[a-z0-9\-._~+/]+=*").expect("bearer regex"),
        Regex::new(r"(?i)(?:api[_-]?key|secret[_-]?key|access[_-]?token|auth[_-]?token|private[_-]?key)\s*[:=]\s*\S+")
            .expect("api key regex"),
        Regex::new(r"(?i)ghp_[a-zA-Z0-9]{36}").expect("github pat regex"),
        Regex::new(r"(?i)gho_[a-zA-Z0-9]{36}").expect("github oauth regex"),
        Regex::new(r"(?i)ghu_[a-zA-Z0-9]{36}").expect("github user regex"),
        Regex::new(r"(?i)ghs_[a-zA-Z0-9]{36}").expect("github server regex"),
        Regex::new(r"(?i)ghr_[a-zA-Z0-9]{36}").expect("github refresh regex"),
        Regex::new(r"(?i)sk-[a-zA-Z0-9]{20,}").expect("openai key regex"),
        Regex::new(r"-----BEGIN\s+(?:RSA\s+)?PRIVATE\s+KEY-----[\s\S]*?-----END\s+(?:RSA\s+)?PRIVATE\s+KEY-----")
            .expect("private key regex"),
        Regex::new(r"(?i)(?:password|passwd|pwd)\s*[:=]\s*\S+").expect("password regex"),
    ]
});

/// Redacts sensitive information from the given text.
///
/// Scans for common secret patterns (Bearer tokens, API keys, GitHub PATs,
/// private keys, passwords) and replaces matches with `[REDACTED]`.
///
/// This function is safe to call on any text — if no secrets are found,
/// the original text is returned unchanged (via clone).
pub fn redact_secrets(text: &str) -> String {
    let mut result = text.to_string();
    for pattern in SECRET_PATTERNS.iter() {
        result = pattern.replace_all(&result, REDACTED).to_string();
    }
    result
}

/// Redacts secrets from text and logs the result at debug level.
///
/// Convenience wrapper that calls [`redact_secrets`] before logging,
/// ensuring no secrets appear in log output even at debug/trace levels.
pub fn log_redacted(prefix: &str, text: &str) {
    log::debug!("{}: {}", prefix, redact_secrets(text));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_secrets_unchanged() {
        let text = "This is a normal code review comment.";
        assert_eq!(redact_secrets(text), text);
    }

    #[test]
    fn test_bearer_token_redacted() {
        let text = "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.test.sig";
        let result = redact_secrets(text);
        assert!(!result.contains("eyJhbGciOiJIUzI1NiJ9"));
        assert!(result.contains(REDACTED));
    }

    #[test]
    fn test_api_key_redacted() {
        let text = "api_key=sk-1234567890abcdefghij";
        let result = redact_secrets(text);
        assert!(!result.contains("sk-1234567890abcdefghij"));
        assert!(result.contains(REDACTED));
    }

    #[test]
    fn test_github_pat_redacted() {
        let text = "token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
        let result = redact_secrets(text);
        assert!(!result.contains("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij"));
        assert!(result.contains(REDACTED));
    }

    #[test]
    fn test_private_key_redacted() {
        let text = "data\n-----BEGIN PRIVATE KEY-----\nMIIBogIBAAJ...\n-----END PRIVATE KEY-----\nmore data";
        let result = redact_secrets(text);
        assert!(!result.contains("MIIBogIBAAJ"));
        assert!(result.contains(REDACTED));
    }

    #[test]
    fn test_password_redacted() {
        let text = "password: super_secret_123";
        let result = redact_secrets(text);
        assert!(!result.contains("super_secret_123"));
        assert!(result.contains(REDACTED));
    }

    #[test]
    fn test_multiple_secrets_redacted() {
        let text = "Bearer abc123 and api_key=xyz789";
        let result = redact_secrets(text);
        assert!(!result.contains("abc123"));
        assert!(!result.contains("xyz789"));
    }

    #[test]
    fn test_diff_content_no_false_positives() {
        let text =
            "diff --git a/src/main.rs b/src/main.rs\n+fn main() {\n+    println!(\"hello\");\n+}";
        assert_eq!(redact_secrets(text), text);
    }
}
