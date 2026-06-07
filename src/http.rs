//! Shared HTTP utilities for GitHub API communication.
//!
//! Provides a single [`github_headers`] builder used by both diff fetching
//! and review submission, along with [`validate_github_base_url`] for
//! strict allowlisting of GitHub API endpoints.

use crate::error::DiffguardError;
use crate::llm::providers;
use reqwest::header::{self, HeaderMap, HeaderValue};
use url::Url;

/// User-Agent string derived from package metadata at compile time.
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// Allowed GitHub API base URLs.
///
/// Only HTTPS URLs matching these patterns are permitted. This prevents
/// accidentally sending `Authorization` headers to arbitrary hosts.
const ALLOWED_BASE_URLS: &[&str] = &["https://api.github.com"];

/// Validates that a GitHub API base URL is on the allowlist.
///
/// Accepts:
/// - Exact match against [`ALLOWED_BASE_URLS`] (e.g. `https://api.github.com`)
/// - GitHub Enterprise pattern: `https://{host}/api/v3` where `{host}` is
///   any valid hostname
/// - Loopback addresses (`http://127.0.0.1`, `http://localhost`) for testing
///
/// All non-loopback URLs must use HTTPS. HTTP URLs to external hosts are rejected.
///
/// # Errors
///
/// Returns [`DiffguardError::Config`] if the URL is not allowed.
pub fn validate_github_base_url(base_url: &str) -> Result<(), DiffguardError> {
    let trimmed = base_url.trim_end_matches('/');

    if trimmed.starts_with("http://127.0.0.1") || trimmed.starts_with("http://localhost") {
        return Ok(());
    }

    if !trimmed.starts_with("https://") {
        return Err(DiffguardError::Config(format!(
            "GitHub base URL must use HTTPS: '{}'. HTTP is not allowed.",
            base_url
        )));
    }

    if ALLOWED_BASE_URLS.contains(&trimmed) {
        return Ok(());
    }

    if trimmed.ends_with("/api/v3") {
        return Ok(());
    }

    Err(DiffguardError::Config(format!(
        "GitHub base URL '{}' is not in the allowlist. \
         Allowed: {} or https://<enterprise-host>/api/v3",
        base_url,
        ALLOWED_BASE_URLS.join(", ")
    )))
}

/// Validates that a provider API base URL is safe for use in CI mode.
///
/// In CI mode, TOML `base_url` overrides are restricted to an exact allowlist
/// of known LLM provider (scheme, host) pairs to prevent SSRF attacks where a
/// malicious `.reviewer.toml` could redirect API calls (and auth headers) to
/// an attacker-controlled server.
///
/// Loopback addresses (`127.0.0.1`, `localhost`) are **rejected** in CI mode
/// to prevent token exfiltration to attacker-controlled local servers.
///
/// All URLs must use HTTPS.
///
/// # Errors
///
/// Returns [`DiffguardError::Config`] if the URL is not allowed.
pub fn validate_provider_base_url(base_url: &str) -> Result<(), DiffguardError> {
    let parsed = Url::parse(base_url).map_err(|_| {
        DiffguardError::Config(format!(
            "Provider base URL is malformed: '{}'. Expected format: https://host/path",
            base_url
        ))
    })?;

    if parsed.scheme() != "https" {
        return Err(DiffguardError::Config(format!(
            "Provider base URL must use HTTPS in CI mode: '{}'. HTTP is not allowed.",
            base_url
        )));
    }

    let host = parsed.host_str().ok_or_else(|| {
        DiffguardError::Config(format!(
            "Provider base URL is malformed: '{}'. No host found.",
            base_url
        ))
    })?;

    if host == "127.0.0.1"
        || host == "localhost"
        || host == "[::1]"
        || host == "0.0.0.0"
        || host == "[::]"
    {
        return Err(DiffguardError::Config(format!(
            "Provider base URL '{}' uses loopback address, which is not allowed in CI mode \
             to prevent token exfiltration. Use a known provider endpoint or run in local mode.",
            base_url
        )));
    }

    let ci_hosts = providers::all_ci_allowed_hosts();
    for &(allowed_scheme, allowed_host) in &ci_hosts {
        if parsed.scheme() == allowed_scheme && host == allowed_host {
            return Ok(());
        }
    }

    let allowed_display: Vec<String> = ci_hosts
        .iter()
        .map(|(s, h)| format!("{}://{}", s, h))
        .collect();

    Err(DiffguardError::Config(format!(
        "Provider base URL '{}' (host: {}) is not in the CI allowlist. \
         Allowed hosts: {}. \
         To use a custom endpoint, run in local mode (unset GITHUB_ACTIONS).",
        base_url,
        host,
        allowed_display.join(", ")
    )))
}

/// Builds default headers for GitHub API requests.
///
/// Includes `Authorization`, `Accept`, `X-GitHub-Api-Version`, and
/// `User-Agent` headers. The `User-Agent` is derived from
/// `CARGO_PKG_NAME`/`CARGO_PKG_VERSION` at compile time.
///
/// # Errors
///
/// Returns [`DiffguardError::Config`] if the token contains invalid
/// header characters.
pub fn github_headers(token: &str) -> Result<HeaderMap, DiffguardError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static("application/vnd.github+json"),
    );
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", token))
            .map_err(|e| DiffguardError::Config(format!("Invalid GitHub token format: {}", e)))?,
    );
    headers.insert(
        "X-GitHub-Api-Version",
        HeaderValue::from_static("2022-11-28"),
    );
    headers.insert(header::USER_AGENT, HeaderValue::from_static(USER_AGENT));
    Ok(headers)
}

/// Builds headers specifically for fetching PR diffs.
///
/// Same as [`github_headers`] but uses the `application/vnd.github.v3.diff`
/// accept header instead of `application/vnd.github+json`.
///
/// # Errors
///
/// Returns [`DiffguardError::Config`] if the token contains invalid
/// header characters.
pub fn github_diff_headers(token: &str) -> Result<HeaderMap, DiffguardError> {
    let mut headers = github_headers(token)?;
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static("application/vnd.github.v3.diff"),
    );
    Ok(headers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_allowed_url() {
        assert!(validate_github_base_url("https://api.github.com").is_ok());
    }

    #[test]
    fn test_validate_allowed_url_trailing_slash() {
        assert!(validate_github_base_url("https://api.github.com/").is_ok());
    }

    #[test]
    fn test_validate_enterprise_url() {
        assert!(validate_github_base_url("https://github.mycompany.com/api/v3").is_ok());
    }

    #[test]
    fn test_reject_http() {
        let result = validate_github_base_url("http://api.github.com");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("HTTPS"));
    }

    #[test]
    fn test_allow_loopback_http() {
        assert!(validate_github_base_url("http://127.0.0.1:8080").is_ok());
        assert!(validate_github_base_url("http://localhost:3000").is_ok());
    }

    #[test]
    fn test_reject_unknown_host() {
        let result = validate_github_base_url("https://evil.example.com");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("allowlist"));
    }

    #[test]
    fn test_reject_partial_match() {
        let result = validate_github_base_url("https://not-api.github.com");
        assert!(result.is_err());
    }

    #[test]
    fn test_github_headers_valid_token() {
        let headers = github_headers("valid-token-123").unwrap();
        assert_eq!(
            headers.get(header::AUTHORIZATION).unwrap(),
            "Bearer valid-token-123"
        );
        assert_eq!(headers.get(header::USER_AGENT).unwrap(), USER_AGENT);
    }

    #[test]
    fn test_github_headers_invalid_token() {
        let result = github_headers("token\x00with\x01control");
        assert!(result.is_err());
    }

    #[test]
    fn test_github_diff_headers_accept() {
        let headers = github_diff_headers("tok").unwrap();
        assert_eq!(
            headers.get(header::ACCEPT).unwrap(),
            "application/vnd.github.v3.diff"
        );
    }

    #[test]
    fn test_provider_base_url_allows_known_hosts() {
        assert!(validate_provider_base_url("https://api.deepseek.com").is_ok());
        assert!(validate_provider_base_url("https://api.deepseek.com/v1").is_ok());
        assert!(validate_provider_base_url("https://api.moonshot.ai/v1").is_ok());
        assert!(validate_provider_base_url(
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
        )
        .is_ok());
        assert!(validate_provider_base_url("https://openrouter.ai/api/v1").is_ok());
        assert!(validate_provider_base_url("https://api.openai.com/v1").is_ok());
    }

    #[test]
    fn test_provider_base_url_rejects_loopback() {
        let result = validate_provider_base_url("http://127.0.0.1:11434/v1");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("loopback") || err.contains("HTTPS"));

        let result = validate_provider_base_url("https://localhost:8080");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("loopback"));
    }

    #[test]
    fn test_provider_base_url_rejects_subdomain_spoof() {
        let result = validate_provider_base_url("https://api.deepseek.com.evil.com/v1");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not in the CI allowlist"));
    }

    #[test]
    fn test_provider_base_url_rejects_unknown_host() {
        let result = validate_provider_base_url("https://evil.example.com/v1");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not in the CI allowlist"));
    }

    #[test]
    fn test_provider_base_url_rejects_http() {
        let result = validate_provider_base_url("http://api.deepseek.com");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("HTTPS"));
    }

    #[test]
    fn test_provider_base_url_rejects_malformed() {
        let result = validate_provider_base_url("not-a-url");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("malformed"));
    }

    #[test]
    fn test_provider_base_url_rejects_ipv6_loopback() {
        let result = validate_provider_base_url("https://[::1]:11434/v1");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("loopback"));
    }

    #[test]
    fn test_provider_base_url_rejects_bind_all() {
        let result = validate_provider_base_url("https://0.0.0.0:8080/v1");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("loopback"));

        let result = validate_provider_base_url("https://[::]:8080/v1");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("loopback"));
    }
}
