//! LLM provider abstraction and shared types.
//!
//! Defines the [`Provider`] enum for dispatching chat completion requests
//! to supported LLM backends, along with shared request/response types.

use crate::error::DiffguardError;
use serde::{Deserialize, Serialize};

pub mod deepseek;
pub mod factory;

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
}

/// Parsed response from a chat completion API call.
#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    /// List of completion choices returned by the model.
    pub choices: Vec<ChatChoice>,
}

/// Supported LLM provider variants.
#[derive(Debug, Clone)]
pub enum Provider {
    /// DeepSeek provider.
    DeepSeek(deepseek::DeepSeekClient),
}

impl Provider {
    /// Sends a chat completion request to the underlying provider.
    ///
    /// # Arguments
    ///
    /// * `system_prompt` — The system instruction for the model.
    /// * `user_message` — The user message (typically the diff content).
    /// * `temperature` — Sampling temperature.
    pub async fn chat_completion(
        &self,
        system_prompt: &str,
        user_message: &str,
        temperature: f32,
    ) -> Result<String, DiffguardError> {
        match self {
            Provider::DeepSeek(client) => {
                client
                    .chat_completion(system_prompt, user_message, temperature)
                    .await
            }
        }
    }
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
