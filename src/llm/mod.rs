//! LLM provider abstraction and shared types.
//!
//! Defines the [`LlmProvider`] async trait for dispatching chat completion
//! requests to supported LLM backends, along with shared request/response types
//! and a common HTTP helper for provider implementations.

use crate::error::DiffguardError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod deepseek;
pub mod factory;
pub mod kimi;
pub mod openai;
pub mod openrouter;
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
/// provider factory to customise base URLs, attribution headers, etc.
#[derive(Debug, Clone, Default)]
pub struct ProviderConfig {
    /// Custom API base URL override.
    pub base_url: Option<String>,
    /// HTTP referer for attribution (OpenRouter only).
    pub http_referer: Option<String>,
    /// Maximum tokens for LLM completions.
    pub max_tokens: Option<u32>,
}

/// Sends a chat completion HTTP request and parses the response.
///
/// Shared implementation used by all provider modules to avoid duplication
/// in HTTP error handling, response deserialization, and content extraction.
/// Logs `reasoning_content` at debug level when present.
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
    let response = client.post(url).json(request).send().await.map_err(|e| {
        let status = e.status().map(|s| s.as_u16()).unwrap_or(0);
        LlmError {
            provider: provider_name.to_string(),
            status,
            message: e.to_string(),
        }
    })?;

    let status = response.status();
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

    if let Some(reasoning) = &choice.message.reasoning_content {
        log::debug!(
            "[{}] reasoning_content ({} chars): {}...",
            provider_name,
            reasoning.len(),
            &reasoning[..reasoning.len().min(200)]
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
