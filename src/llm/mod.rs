//! LLM provider abstraction and shared types.
//!
//! Defines the [`LlmProvider`] async trait for dispatching chat completion
//! requests to supported LLM backends, along with shared request/response types
//! and a common HTTP helper for provider implementations.

use crate::error::DiffguardError;
use async_trait::async_trait;
use reqwest::header::{self, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

/// HTTP request timeout for LLM API calls.
const LLM_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

pub mod deepseek;
pub mod factory;
pub mod kimi;
pub mod openai;
pub mod openrouter;
pub mod providers;
pub mod qwen;

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
/// All providers must implement this trait to participate in the diffguard
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
    ) -> Result<String, DiffguardError>;
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
/// Returns [`DiffguardError::LlmApi`] on network errors, non-success HTTP
/// status codes, or response parsing failures.
pub(crate) async fn send_chat_request<B: Serialize + Send>(
    client: &reqwest::Client,
    url: &str,
    request: &B,
    provider_name: &str,
) -> Result<String, DiffguardError> {
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

impl From<LlmError> for DiffguardError {
    fn from(err: LlmError) -> Self {
        DiffguardError::LlmApi {
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
/// Returns [`DiffguardError::Config`] if the API key or extra header values
/// contain invalid HTTP header characters.
pub(crate) fn build_llm_client(
    provider_name: &str,
    api_key: &str,
    extra_headers: &[(&str, &str)],
) -> Result<reqwest::Client, DiffguardError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", api_key)).map_err(|e| {
            DiffguardError::Config(format!("Invalid {} API key format: {}", provider_name, e))
        })?,
    );
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    for &(name, value) in extra_headers {
        let h_name = header::HeaderName::from_bytes(name.as_bytes()).map_err(|e| {
            DiffguardError::Config(format!(
                "Invalid header name '{}' for {}: {}",
                name, provider_name, e
            ))
        })?;
        headers.insert(
            h_name,
            HeaderValue::from_str(value).map_err(|e| {
                DiffguardError::Config(format!(
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
        .map_err(|e| DiffguardError::Config(format!("Failed to build HTTP client: {}", e)))
}
