//! LLM provider abstraction and shared types.
//!
//! Defines the [`LlmProvider`] async trait for dispatching chat completion
//! requests to supported LLM backends, along with shared request/response types
//! and a common HTTP helper for provider implementations.

use crate::error::RsGuardError;
use async_trait::async_trait;
use reqwest::header::{self, HeaderMap, HeaderValue};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashMap;

// Default timeout comes from crate::config::DEFAULT_LLM_TIMEOUT_SECS (120s).

pub mod factory;
mod generic_client;
mod kernel_client;
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
    /// Optional result format hint (e.g. `"message"` for Qwen/DashScope).
    ///
    /// Some providers require an explicit result format field. When `None`,
    /// the field is omitted from the serialized request.
    ///
    /// Uses `Cow<'static, str>` so known providers keep a zero-cost borrowed
    /// value while per-provider configuration overrides can supply an owned
    /// dynamic value for custom OpenAI-compatible endpoints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_format: Option<Cow<'static, str>>,
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

/// Deserializes a nullable JSON string as `String`, mapping `null` and absent
/// values to empty (DeepSeek/Kimi thinking models emit `"content": null`).
fn deserialize_nullable_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

/// Message content within a chat completion response choice.
#[derive(Debug, Deserialize)]
pub struct ChatMessageResponse {
    /// The generated text content.
    #[serde(default, deserialize_with = "deserialize_nullable_string")]
    pub content: String,
    /// Optional reasoning content (e.g. Kimi/Moonshot AI chain-of-thought).
    #[serde(default)]
    pub reasoning_content: Option<String>,
}

/// Token usage information returned by OpenAI-compatible APIs.
///
/// Most providers include a `usage` object in the response with
/// `prompt_tokens` and `completion_tokens`. When present, these are
/// preferred over character-based heuristics for metrics and cost
/// estimation (v1.8 #115).
#[derive(Debug, Default, Deserialize)]
pub struct TokenUsage {
    /// Number of tokens in the prompt (input).
    #[serde(default)]
    pub prompt_tokens: Option<u64>,
    /// Number of tokens in the completion (output).
    #[serde(default)]
    pub completion_tokens: Option<u64>,
    /// Total tokens (some providers include this; computed otherwise).
    #[serde(default)]
    pub total_tokens: Option<u64>,
}

impl TokenUsage {
    /// Returns `true` if at least one of `prompt_tokens` or `completion_tokens`
    /// is present. `total_tokens` alone does not count — the pipeline needs
    /// per-direction counts to be useful.
    #[must_use]
    pub fn has_any(&self) -> bool {
        self.prompt_tokens.is_some() || self.completion_tokens.is_some()
    }
}

/// Parsed response from a chat completion API call.
#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    /// List of completion choices returned by the model.
    pub choices: Vec<ChatChoice>,
    /// Optional token usage from the API (v1.8 #115).
    #[serde(default)]
    pub usage: Option<TokenUsage>,
}

/// Result of a chat completion call, including optional API-reported usage.
///
/// When the provider returns `usage` data (prompt/completion tokens), it is
/// captured here so the pipeline can prefer real token counts over character
/// heuristics for metrics and cost estimation (v1.8 #115).
#[derive(Debug)]
pub struct ChatCompletionResult {
    /// The generated text content from the LLM.
    pub content: String,
    /// Optional token usage from the API response.
    pub usage: Option<TokenUsage>,
}

impl ChatCompletionResult {
    /// Creates a result with content but no usage data (for cache hits or
    /// providers that don't report usage).
    #[must_use]
    pub fn from_content(content: String) -> Self {
        Self {
            content,
            usage: None,
        }
    }
}

impl From<String> for ChatCompletionResult {
    fn from(content: String) -> Self {
        Self::from_content(content)
    }
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
    ) -> Result<ChatCompletionResult, RsGuardError>;
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
    /// Optional per-provider `result_format` override.
    ///
    /// When set, this value is sent in the request body instead of the
    /// provider's static default. Useful for custom OpenAI-compatible
    /// endpoints that require a specific result format.
    pub result_format: Option<String>,
    /// LLM request timeout in seconds (total). When None, the client uses
    /// the crate default (120s as of v1.2.3).
    pub timeout_secs: Option<u64>,
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
) -> Result<ChatCompletionResult, RsGuardError> {
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
    // Only explicitly-allowed, non-sensitive headers are logged.
    const ALLOWED_HEADERS: &[&str] = &[
        "content-type",
        "content-length",
        "cache-control",
        "etag",
        "date",
        "server",
        "x-request-id",
        "x-ratelimit-limit",
        "x-ratelimit-remaining",
        "x-ratelimit-reset",
    ];
    if log::log_enabled!(log::Level::Debug) {
        let headers = response.headers();
        let safe_headers: Vec<String> = headers
            .iter()
            .filter_map(|(name, value)| {
                let name_str = name.as_str();
                if !ALLOWED_HEADERS.contains(&name_str) {
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

    let body = response.text().await.map_err(|e| LlmError {
        provider: provider_name.to_string(),
        status: 0,
        message: format!("Failed to read response body: {e}"),
    })?;

    if !status.is_success() {
        return Err(LlmError {
            provider: provider_name.to_string(),
            status: status.as_u16(),
            message: body,
        }
        .into());
    }

    parse_completion_response_body(&body, provider_name).map_err(Into::into)
}

/// Parses an OpenAI-compatible `/chat/completions` JSON body and extracts the
/// assistant's final `content` string.
///
/// Uses loose [`Value`] traversal instead of strict structs so provider-specific
/// shapes (nullable `content`, multimodal content arrays, extra choice fields)
/// do not fail deserialization.
fn parse_completion_response_body(
    body: &str,
    provider_name: &str,
) -> Result<ChatCompletionResult, LlmError> {
    let value: Value = serde_json::from_str(body).map_err(|e| LlmError {
        provider: provider_name.to_string(),
        status: 0,
        message: format!(
            "Failed to parse response JSON: {e} (body_len={})",
            body.len()
        ),
    })?;

    let choices = value
        .get("choices")
        .and_then(Value::as_array)
        .filter(|c| !c.is_empty())
        .ok_or_else(|| LlmError {
            provider: provider_name.to_string(),
            status: 0,
            message: "Empty response from LLM".to_string(),
        })?;

    let message = choices[0].get("message").ok_or_else(|| LlmError {
        provider: provider_name.to_string(),
        status: 0,
        message: "LLM response missing choices[0].message".to_string(),
    })?;

    let content = extract_text_field(message.get("content"));
    let reasoning_content = extract_optional_text_field(message.get("reasoning_content"));

    let resolved_content =
        resolve_assistant_content(&content, reasoning_content.as_deref(), provider_name)?;

    // Extract usage data if present (v1.8 #115).
    let usage = value.get("usage").map(|u| TokenUsage {
        prompt_tokens: u.get("prompt_tokens").and_then(Value::as_u64),
        completion_tokens: u.get("completion_tokens").and_then(Value::as_u64),
        total_tokens: u.get("total_tokens").and_then(Value::as_u64),
    });

    Ok(ChatCompletionResult {
        content: resolved_content,
        usage,
    })
}

/// Extracts text from a chat `content` or `reasoning_content` field.
///
/// Accepts `string`, `null`, absent, or OpenAI-style multimodal arrays
/// (`[{"type":"text","text":"..."}]`).
fn extract_text_field(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.as_str())
            })
            .collect(),
        _ => String::new(),
    }
}

/// Like [`extract_text_field`], but returns `None` when the field is absent or null.
fn extract_optional_text_field(value: Option<&Value>) -> Option<String> {
    match value {
        None | Some(Value::Null) => None,
        Some(other) => {
            let text = extract_text_field(Some(other));
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
    }
}

/// Extracts the assistant's final answer from a chat completion message.
///
/// Thinking models (DeepSeek v4, Kimi) may return chain-of-thought in
/// `reasoning_content` while leaving `content` empty or JSON-null when the
/// output token budget is exhausted. Empty `content` is a transient failure
/// (retryable via [`LlmError`] status 0).
fn resolve_assistant_content(
    content: &str,
    reasoning_content: Option<&str>,
    provider_name: &str,
) -> Result<String, LlmError> {
    if let Some(reasoning) = reasoning_content {
        log::debug!(
            "[{}] reasoning_content present ({} chars, content not logged)",
            provider_name,
            reasoning.len()
        );
    }

    if !content.trim().is_empty() {
        return Ok(content.to_string());
    }

    let reasoning_len = reasoning_content.map(|r| r.len()).unwrap_or(0);

    log::warn!(
        "[{}] Empty assistant content from LLM (reasoning_content: {} chars). \
         Reasoning may have consumed the max_tokens budget — consider raising max_tokens.",
        provider_name,
        reasoning_len
    );

    // Empty content WITH reasoning is a deterministic budget failure: the
    // marker suffix lets callers (pipeline retry escalation) distinguish it
    // from a transient empty response with no reasoning at all.
    let message = if reasoning_len > 0 {
        format!(
            "Empty assistant content from LLM (reasoning_content: {reasoning_len} chars; {})",
            crate::error::REASONING_BUDGET_EXHAUSTED_MARKER
        )
    } else {
        "Empty assistant content from LLM (no reasoning content returned)".to_string()
    };

    Err(LlmError {
        provider: provider_name.to_string(),
        status: 0,
        message,
    })
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
/// and any additional headers. Uses the provided `timeout`.
///
/// # Arguments
///
/// * `provider_name` — Provider name for error messages.
/// * `api_key` — API key for Bearer authentication.
/// * `extra_headers` — Additional headers to include (e.g. `HTTP-Referer`).
/// * `timeout` — Total request timeout. Prefer values >= 60s for thinking models.
///
/// # Errors
///
/// Returns [`RsGuardError::Config`] if the API key or extra header values
/// contain invalid HTTP header characters.
pub(crate) fn build_llm_client(
    provider_name: &str,
    api_key: &str,
    extra_headers: &[(&str, &str)],
    timeout: std::time::Duration,
) -> Result<reqwest::Client, RsGuardError> {
    let mut headers = HeaderMap::new();
    // Skip Authorization header when the API key is empty (e.g. Ollama local).
    if !api_key.is_empty() {
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", api_key)).map_err(|e| {
                RsGuardError::Config(format!("Invalid {} API key format: {}", provider_name, e))
            })?,
        );
    }
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
        .timeout(timeout)
        .build()
        .map_err(|e| RsGuardError::Config(format!("Failed to build HTTP client: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_llm_client_rejects_invalid_api_key() {
        let result = build_llm_client(
            "deepseek",
            "key\x00with\x01control",
            &[],
            std::time::Duration::from_secs(60),
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Invalid deepseek API key format"),
            "Expected API key format error, got: {}",
            err
        );
    }

    #[test]
    fn test_build_llm_client_allows_empty_api_key() {
        // Ollama and other local providers don't require an API key.
        // An empty key should succeed (no Authorization header is set).
        let result = build_llm_client("ollama", "", &[], std::time::Duration::from_secs(60));
        assert!(
            result.is_ok(),
            "empty API key should be allowed: {:?}",
            result
        );
    }

    #[test]
    fn test_build_llm_client_rejects_invalid_extra_header_name() {
        let result = build_llm_client(
            "testprov",
            "valid-key",
            &[("inv@lid header name", "value")],
            std::time::Duration::from_secs(60),
        );
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
        let result = build_llm_client(
            "testprov",
            "valid-key",
            &[("X-Custom", "val\x00ue")],
            std::time::Duration::from_secs(60),
        );
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
        let result = build_llm_client(
            "deepseek",
            "valid-key-123",
            &[],
            std::time::Duration::from_secs(60),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_llm_client_succeeds_with_extra_headers() {
        let result = build_llm_client(
            "openrouter",
            "valid-key",
            &[("HTTP-Referer", "https://example.com"), ("X-Title", "test")],
            std::time::Duration::from_secs(60),
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

        let client =
            build_llm_client("testprov", "key", &[], std::time::Duration::from_secs(60)).unwrap();
        let request = ChatRequest {
            model: "test-model".to_string(),
            messages: chat_messages("system", "user"),
            temperature: 0.1,
            max_tokens: None,
            result_format: None,
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

        let client =
            build_llm_client("testprov", "key", &[], std::time::Duration::from_secs(60)).unwrap();
        let request = ChatRequest {
            model: "test-model".to_string(),
            messages: chat_messages("system", "user"),
            temperature: 0.1,
            max_tokens: None,
            result_format: None,
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

        let client =
            build_llm_client("testprov", "key", &[], std::time::Duration::from_secs(60)).unwrap();
        let request = ChatRequest {
            model: "test-model".to_string(),
            messages: chat_messages("system", "user"),
            temperature: 0.1,
            max_tokens: None,
            result_format: None,
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
    async fn test_send_chat_request_null_content_deepseek_shape_is_retryable() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;
        // DeepSeek thinking-mode shape: content null, reasoning_content present
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "test-id",
                "object": "chat.completion",
                "created": 1705651092,
                "model": "deepseek-v4-pro",
                "choices": [{
                    "index": 0,
                    "finish_reason": "length",
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "reasoning_content": "long internal reasoning"
                    }
                }]
            })))
            .mount(&mock_server)
            .await;

        let client =
            build_llm_client("deepseek", "key", &[], std::time::Duration::from_secs(60)).unwrap();
        let request = ChatRequest {
            model: "deepseek-v4-pro".to_string(),
            messages: chat_messages("system", "user"),
            temperature: 0.1,
            max_tokens: Some(4096),
            result_format: None,
            extra_body: HashMap::new(),
        };
        let result = send_chat_request(
            &client,
            &format!("{}/chat/completions", mock_server.uri()),
            &request,
            "deepseek",
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, RsGuardError::LlmApi { status: 0, .. }));
        assert!(err.is_retryable());
        let msg = err.to_string();
        assert!(
            msg.contains("Empty assistant content"),
            "expected empty content error, got: {}",
            msg
        );
        assert!(msg.contains("reasoning_content: 23 chars"));
    }

    #[tokio::test]
    async fn test_send_chat_request_empty_content_with_reasoning_is_retryable() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": "",
                        "reasoning_content": "Internal reasoning consumed the token budget"
                    }
                }]
            })))
            .mount(&mock_server)
            .await;

        let client =
            build_llm_client("deepseek", "key", &[], std::time::Duration::from_secs(60)).unwrap();
        let request = ChatRequest {
            model: "deepseek-v4-pro".to_string(),
            messages: chat_messages("system", "user"),
            temperature: 0.1,
            max_tokens: Some(4096),
            result_format: None,
            extra_body: HashMap::new(),
        };
        let result = send_chat_request(
            &client,
            &format!("{}/chat/completions", mock_server.uri()),
            &request,
            "deepseek",
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.is_retryable());
    }

    #[tokio::test]
    async fn test_send_chat_request_returns_only_content_reasoning_not_included() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;
        // Response contains both reasoning_content (internal) and final content.
        // The value returned by chat_completion must be ONLY the final content.
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": "Final review verdict here.",
                        "reasoning_content": "SECRET REASONING that must not leak to caller or verdict"
                    }
                }]
            })))
            .mount(&mock_server)
            .await;

        let client =
            build_llm_client("deepseek", "key", &[], std::time::Duration::from_secs(60)).unwrap();
        let request = ChatRequest {
            model: "deepseek-v4-pro".to_string(),
            messages: chat_messages("system", "user"),
            temperature: 0.1,
            max_tokens: Some(4096),
            result_format: None,
            extra_body: HashMap::new(),
        };
        let result = send_chat_request(
            &client,
            &format!("{}/chat/completions", mock_server.uri()),
            &request,
            "deepseek",
        )
        .await
        .unwrap();

        assert_eq!(result.content, "Final review verdict here.");
        assert!(
            !result.content.contains("SECRET REASONING"),
            "reasoning_content must not appear in the content returned to pipeline"
        );
    }

    #[test]
    fn test_extract_text_field_multimodal_array() {
        let value = serde_json::json!([
            {"type": "text", "text": "Hello "},
            {"type": "text", "text": "world"}
        ]);
        assert_eq!(extract_text_field(Some(&value)), "Hello world");
    }

    #[test]
    fn test_parse_completion_response_body_content_array() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": "Review OK"}]
                }
            }]
        })
        .to_string();

        let result = parse_completion_response_body(&body, "deepseek").unwrap();
        assert_eq!(result.content, "Review OK");
    }

    #[tokio::test]
    async fn test_send_chat_request_content_array_shape() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": [{"type": "text", "text": "Array content OK"}]
                    }
                }]
            })))
            .mount(&mock_server)
            .await;

        let client =
            build_llm_client("deepseek", "key", &[], std::time::Duration::from_secs(60)).unwrap();
        let request = ChatRequest {
            model: "deepseek-v4-pro".to_string(),
            messages: chat_messages("system", "user"),
            temperature: 0.1,
            max_tokens: None,
            result_format: None,
            extra_body: HashMap::new(),
        };
        let result = send_chat_request(
            &client,
            &format!("{}/chat/completions", mock_server.uri()),
            &request,
            "deepseek",
        )
        .await
        .unwrap();
        assert_eq!(result.content, "Array content OK");
    }

    #[test]
    fn test_parse_usage_from_response() {
        let body = serde_json::json!({
            "choices": [{ "message": { "content": "Review OK" } }],
            "usage": {
                "prompt_tokens": 1234,
                "completion_tokens": 567,
                "total_tokens": 1801
            }
        })
        .to_string();

        let result = parse_completion_response_body(&body, "deepseek").unwrap();
        assert_eq!(result.content, "Review OK");
        let usage = result.usage.expect("usage should be present");
        assert_eq!(usage.prompt_tokens, Some(1234));
        assert_eq!(usage.completion_tokens, Some(567));
        assert_eq!(usage.total_tokens, Some(1801));
        assert!(usage.has_any());
    }

    #[test]
    fn test_parse_usage_absent_when_not_in_response() {
        let body = serde_json::json!({
            "choices": [{ "message": { "content": "Review OK" } }]
        })
        .to_string();

        let result = parse_completion_response_body(&body, "deepseek").unwrap();
        assert_eq!(result.content, "Review OK");
        assert!(result.usage.is_none(), "usage should be None when absent");
    }

    #[test]
    fn test_parse_usage_partial_fields() {
        let body = serde_json::json!({
            "choices": [{ "message": { "content": "Review OK" } }],
            "usage": {
                "prompt_tokens": 100
            }
        })
        .to_string();

        let result = parse_completion_response_body(&body, "deepseek").unwrap();
        let usage = result.usage.expect("usage should be present");
        assert_eq!(usage.prompt_tokens, Some(100));
        assert_eq!(usage.completion_tokens, None);
        assert_eq!(usage.total_tokens, None);
        assert!(usage.has_any());
    }

    #[test]
    fn test_token_usage_has_any_false_when_empty() {
        let usage = TokenUsage {
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
        };
        assert!(!usage.has_any());
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

        let client =
            build_llm_client("testprov", "key", &[], std::time::Duration::from_secs(60)).unwrap();
        let request = ChatRequest {
            model: "test-model".to_string(),
            messages: chat_messages("system", "user"),
            temperature: 0.1,
            max_tokens: None,
            result_format: None,
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
        let result = result.unwrap();
        assert_eq!(result.content, "Review text");
        assert!(!result.content.contains("Internal reasoning"));
    }
}
