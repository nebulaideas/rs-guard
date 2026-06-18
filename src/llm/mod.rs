//! LLM provider abstraction and shared types.
//!
//! Defines the [`LlmProvider`] async trait for dispatching chat completion
//! requests to supported LLM backends, along with shared request/response types
//! and a common HTTP helper for provider implementations.

use crate::error::RsGuardError;
use async_trait::async_trait;
use reqwest::header::{self, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// HTTP request timeout for LLM API calls.
const LLM_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

pub mod factory;
mod generic_client;
pub mod providers;

pub use providers::VariantEffect;

/// A single message in a chat conversation.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    /// The role of the message sender (e.g. `"system"`, `"user"`).
    pub role: String,
    /// The message content.
    pub content: String,
}

/// Request body for a chat completion API call.
#[derive(Debug, Serialize)]
pub struct ChatRequest {
    /// Model identifier to use for completion.
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<ChatMessage>,
    /// Sampling temperature (0.0 to 2.0).
    pub temperature: f32,
    /// Maximum tokens in the response (provider-agnostic).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Extra top-level fields contributed by `VariantEffect::ExtraBody`
    /// (e.g. "reasoning_effort" or provider-specific thinking toggles).
    ///
    /// Serialized via `#[serde(flatten)]` so they appear at the same level as the
    /// standard fields (`model`, `messages`, `temperature`, `max_tokens`).
    ///
    /// **Important:** Keys provided via `ExtraBody` **must not** collide with the
    /// standard top-level `ChatRequest` fields. A colliding key will silently
    /// overwrite the corresponding field during serialization (e.g. overriding
    /// the chosen `model` or `temperature`).
    ///
    /// Uses `default` so that deserialization (or custom provider code following
    /// older examples) does not require the field when it is empty.
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra_body: HashMap<String, serde_json::Value>,
}

/// A single choice in a chat completion response.
#[derive(Debug, Deserialize)]
pub struct ChatChoice {
    /// The message content of this choice.
    pub message: ChatMessageResponse,
}

/// Message content within a chat completion response choice.
#[derive(Debug, Deserialize)]
pub struct ChatMessageResponse {
    /// The generated text content.
    pub content: String,
    /// Optional reasoning content (e.g. Kimi/Moonshot AI chain-of-thought).
    #[serde(default)]
    pub reasoning_content: Option<String>,
}

/// Parsed response from a chat completion API call.
#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    /// List of completion choices returned by the model.
    pub choices: Vec<ChatChoice>,
}

/// Async trait for LLM provider implementations.
///
/// All providers must implement this trait to participate in the rs-guard
/// pipeline. Implementations are expected to handle HTTP communication,
/// authentication, and response parsing.
#[async_trait]
pub trait LlmProvider: Send + Sync + std::fmt::Debug {
    /// Returns the provider's display name (e.g. `"deepseek"`).
    fn name(&self) -> &'static str;

    /// Sends a chat completion request to the provider.
    ///
    /// # Arguments
    ///
    /// * `system_prompt` — The system instruction for the model.
    /// * `user_message` — The user message (typically the diff content).
    /// * `temperature` — Sampling temperature.
    async fn chat_completion(
        &self,
        system_prompt: &str,
        user_message: &str,
        temperature: f32,
    ) -> Result<String, RsGuardError>;
}

/// Dynamic-dispatch handle for an LLM provider.
///
/// Uses a trait object so the factory can return heterogeneous providers
/// without enum match arms at every call site.
pub type Provider = Box<dyn LlmProvider>;

/// Provider-specific configuration overrides from `.reviewer.toml`.
///
/// These are resolved by [`crate::config::Config`] and passed to the
/// provider factory to customise base URLs, model, attribution headers, etc.
#[derive(Debug, Clone, Default)]
pub struct ProviderConfig {
    /// Custom API base URL override.
    pub base_url: Option<String>,
    /// HTTP referer for attribution (OpenRouter only).
    pub http_referer: Option<String>,
    /// Maximum tokens for LLM completions.
    pub max_tokens: Option<u32>,
    /// Model identifier to use (overrides provider default).
    pub model: String,
    /// Provider-specific model variant (e.g. "flash", "thinking-on").
    ///
    /// Resolved (together with any `ExtraBody` fields) when the client
    /// performs a completion. See [`providers`] and the per-provider
    /// tables in `docs/PROVIDERS.md`.
    pub variant: Option<String>,
}

/// Sends a chat completion HTTP request and parses the response.
///
/// Shared implementation used by all provider modules to avoid duplication
/// in HTTP error handling, response deserialization, and content extraction.
///
/// # Arguments
///
/// * `client` — Pre-configured reqwest client with auth headers.
/// * `url` — Full endpoint URL.
/// * `request` — Serializable request body.
/// * `provider_name` — Provider name for error reporting.
///
/// # Errors
///
/// Returns [`RsGuardError::LlmApi`] on network errors, non-success HTTP
/// status codes, or response parsing failures.
pub(crate) async fn send_chat_request<B: Serialize + Send>(
    client: &reqwest::Client,
    url: &str,
    request: &B,
    provider_name: &str,
) -> Result<String, RsGuardError> {
    log::debug!(
        "[{}] POST {} (effective params logged at debug level)",
        provider_name,
        url
    );

    let response = client.post(url).json(request).send().await.map_err(|e| {
        let status = e.status().map(|s| s.as_u16()).unwrap_or(0);
        LlmError {
            provider: provider_name.to_string(),
            status,
            message: e.to_string(),
        }
    })?;

    let status = response.status();

    // Log sanitized response headers at debug level for observability.
    // Only safe, non-sensitive headers are logged.
    if log::log_enabled!(log::Level::Debug) {
        let headers = response.headers();
        let safe_headers: Vec<String> = headers
            .iter()
            .filter_map(|(name, value)| {
                let name_str = name.as_str();
                // Skip potentially sensitive headers
                if name_str == "authorization"
                    || name_str == "set-cookie"
                    || name_str.contains("token")
                    || name_str.contains("key")
                {
                    return None;
                }
                let val = value.to_str().unwrap_or("<binary>");
                // Truncate long values (use char-aware truncation to avoid panics on multi-byte UTF-8)
                let val_display = if val.len() > 80 {
                    let truncated: String = val.chars().take(80).collect();
                    format!("{}...", truncated)
                } else {
                    val.to_string()
                };
                Some(format!("{}: {}", name_str, val_display))
            })
            .collect();
        log::debug!(
            "[{}] Response status: {} — headers: [{}]",
            provider_name,
            status.as_u16(),
            safe_headers.join(", ")
        );
    }

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(LlmError {
            provider: provider_name.to_string(),
            status: status.as_u16(),
            message: body,
        }
        .into());
    }

    let chat_response: ChatResponse = response.json().await.map_err(|e| LlmError {
        provider: provider_name.to_string(),
        status: 0,
        message: format!("Failed to parse response: {}", e),
    })?;

    let choice = chat_response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| LlmError {
            provider: provider_name.to_string(),
            status: 0,
            message: "Empty response from LLM".to_string(),
        })?;

    if let Some(ref reasoning) = choice.message.reasoning_content {
        log::debug!(
            "[{}] reasoning_content present ({} chars, content not logged)",
            provider_name,
            reasoning.len()
        );
    }

    Ok(choice.message.content)
}

/// Provider-specific error information.
#[derive(Debug, Clone)]
pub struct LlmError {
    /// Name of the provider that produced the error.
    pub provider: String,
    /// HTTP status code, or 0 for non-HTTP failures.
    pub status: u16,
    /// Human-readable error description.
    pub message: String,
}

impl From<LlmError> for RsGuardError {
    fn from(err: LlmError) -> Self {
        RsGuardError::LlmApi {
            provider: err.provider,
            status: err.status,
            message: err.message,
        }
    }
}

/// Creates a system + user message pair for a chat completion request.
///
/// Shared helper to avoid duplicating message construction across providers.
pub(crate) fn chat_messages(system_prompt: &str, user_message: &str) -> Vec<ChatMessage> {
    vec![
        ChatMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: user_message.to_string(),
        },
    ]
}

/// Builds a [`reqwest::Client`] with standard LLM provider headers.
///
/// Sets `Authorization: Bearer {api_key}`, `Content-Type: application/json`,
/// and any additional headers. Uses [`LLM_REQUEST_TIMEOUT`].
///
/// # Arguments
///
/// * `provider_name` — Provider name for error messages.
/// * `api_key` — API key for Bearer authentication.
/// * `extra_headers` — Additional headers to include (e.g. `HTTP-Referer`).
///
/// # Errors
///
/// Returns [`RsGuardError::Config`] if the API key or extra header values
/// contain invalid HTTP header characters.
pub(crate) fn build_llm_client(
    provider_name: &str,
    api_key: &str,
    extra_headers: &[(&str, &str)],
) -> Result<reqwest::Client, RsGuardError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", api_key)).map_err(|e| {
            RsGuardError::Config(format!("Invalid {} API key format: {}", provider_name, e))
        })?,
    );
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    for &(name, value) in extra_headers {
        let h_name = header::HeaderName::from_bytes(name.as_bytes()).map_err(|e| {
            RsGuardError::Config(format!(
                "Invalid header name '{}' for {}: {}",
                name, provider_name, e
            ))
        })?;
        headers.insert(
            h_name,
            HeaderValue::from_str(value).map_err(|e| {
                RsGuardError::Config(format!(
                    "Invalid header '{}' value for {}: {}",
                    name, provider_name, e
                ))
            })?,
        );
    }

    reqwest::Client::builder()
        .default_headers(headers)
        .timeout(LLM_REQUEST_TIMEOUT)
        .build()
        .map_err(|e| RsGuardError::Config(format!("Failed to build HTTP client: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_llm_client_rejects_invalid_api_key() {
        let result = build_llm_client("deepseek", "key\x00with\x01control", &[]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Invalid deepseek API key format"),
            "Expected API key format error, got: {}",
            err
        );
    }

    #[test]
    fn test_build_llm_client_rejects_invalid_extra_header_name() {
        let result = build_llm_client("testprov", "valid-key", &[("inv@lid header name", "value")]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Invalid header name"),
            "Expected header name error, got: {}",
            err
        );
    }

    #[test]
    fn test_build_llm_client_rejects_invalid_extra_header_value() {
        let result = build_llm_client("testprov", "valid-key", &[("X-Custom", "val\x00ue")]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Invalid header"),
            "Expected header value error, got: {}",
            err
        );
    }

    #[test]
    fn test_build_llm_client_succeeds_with_valid_inputs() {
        let result = build_llm_client("deepseek", "valid-key-123", &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_llm_client_succeeds_with_extra_headers() {
        let result = build_llm_client(
            "openrouter",
            "valid-key",
            &[("HTTP-Referer", "https://example.com"), ("X-Title", "test")],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_chat_messages_ordering() {
        let messages = chat_messages("system prompt", "user diff");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[0].content, "system prompt");
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[1].content, "user diff");
    }

    #[tokio::test]
    async fn test_send_chat_request_empty_choices() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": []
            })))
            .mount(&mock_server)
            .await;

        let client = build_llm_client("testprov", "key", &[]).unwrap();
        let request = ChatRequest {
            model: "test-model".to_string(),
            messages: chat_messages("system", "user"),
            temperature: 0.1,
            max_tokens: None,
            extra_body: HashMap::new(),
        };
        let result = send_chat_request(
            &client,
            &format!("{}/chat/completions", mock_server.uri()),
            &request,
            "testprov",
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Empty response from LLM"),
            "Expected empty choices error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_send_chat_request_malformed_json() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string("this is not json"))
            .mount(&mock_server)
            .await;

        let client = build_llm_client("testprov", "key", &[]).unwrap();
        let request = ChatRequest {
            model: "test-model".to_string(),
            messages: chat_messages("system", "user"),
            temperature: 0.1,
            max_tokens: None,
            extra_body: HashMap::new(),
        };
        let result = send_chat_request(
            &client,
            &format!("{}/chat/completions", mock_server.uri()),
            &request,
            "testprov",
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to parse response"),
            "Expected parse error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_send_chat_request_http_error() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&mock_server)
            .await;

        let client = build_llm_client("testprov", "key", &[]).unwrap();
        let request = ChatRequest {
            model: "test-model".to_string(),
            messages: chat_messages("system", "user"),
            temperature: 0.1,
            max_tokens: None,
            extra_body: HashMap::new(),
        };
        let result = send_chat_request(
            &client,
            &format!("{}/chat/completions", mock_server.uri()),
            &request,
            "testprov",
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("500"), "Expected 500 error, got: {}", err);
    }

    #[tokio::test]
    async fn test_send_chat_request_reasoning_content_ignored() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": "Review text",
                        "reasoning_content": "Internal reasoning that should not appear in output"
                    }
                }]
            })))
            .mount(&mock_server)
            .await;

        let client = build_llm_client("testprov", "key", &[]).unwrap();
        let request = ChatRequest {
            model: "test-model".to_string(),
            messages: chat_messages("system", "user"),
            temperature: 0.1,
            max_tokens: None,
            extra_body: HashMap::new(),
        };
        let result = send_chat_request(
            &client,
            &format!("{}/chat/completions", mock_server.uri()),
            &request,
            "testprov",
        )
        .await;

        assert!(result.is_ok());
        let content = result.unwrap();
        assert_eq!(content, "Review text");
        assert!(!content.contains("Internal reasoning"));
    }
}
