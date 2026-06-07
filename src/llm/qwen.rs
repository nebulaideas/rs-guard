//! Qwen (Alibaba Cloud) LLM provider implementation.
//!
//! Communicates with the DashScope-compatible chat completions API.
//! Sends `result_format: "message"` as required by the DashScope API.

use crate::error::DiffguardError;
use crate::llm::{build_llm_client, chat_messages, send_chat_request, ChatMessage, LlmProvider};
use async_trait::async_trait;
use serde::Serialize;

/// Default Qwen API base URL.
const DEFAULT_BASE_URL: &str = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1";

/// Default model identifier for Qwen.
const DEFAULT_MODEL: &str = "qwen-plus";

/// Qwen-specific chat request with `result_format` field.
#[derive(Debug, Serialize)]
struct QwenChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    result_format: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

/// Client for the Qwen chat completions API.
#[derive(Debug, Clone)]
pub struct QwenClient {
    base_url: String,
    model: String,
    max_tokens: Option<u32>,
    client: reqwest::Client,
}

impl QwenClient {
    /// Creates a new Qwen client with the given API key.
    pub fn new(api_key: impl Into<String>) -> Result<Self, DiffguardError> {
        let client = build_llm_client("qwen", &api_key.into(), &[])?;
        Ok(Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            max_tokens: None,
            client,
        })
    }

    /// Sets a custom base URL for the API endpoint.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Sets a custom model identifier.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Sets the maximum tokens for completions.
    pub fn with_max_tokens(mut self, max_tokens: Option<u32>) -> Self {
        self.max_tokens = max_tokens;
        self
    }
}

#[async_trait]
impl LlmProvider for QwenClient {
    fn name(&self) -> &'static str {
        "qwen"
    }

    async fn chat_completion(
        &self,
        system_prompt: &str,
        user_message: &str,
        temperature: f32,
    ) -> Result<String, DiffguardError> {
        let request = QwenChatRequest {
            model: self.model.clone(),
            messages: chat_messages(system_prompt, user_message),
            temperature,
            result_format: "message",
            max_tokens: self.max_tokens,
        };

        let url = format!("{}/chat/completions", self.base_url);
        send_chat_request(&self.client, &url, &request, "qwen").await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_chat_completion_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": "Looks good.\n\n[DIFFGUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 0\nSecurityIssues: 0"
                    }
                }]
            })))
            .mount(&mock_server)
            .await;

        let client = QwenClient::new("test-key")
            .unwrap()
            .with_base_url(mock_server.uri());
        let result = client
            .chat_completion("You are a reviewer.", "diff content", 0.1)
            .await;

        assert!(result.is_ok());
        assert!(result.unwrap().contains("POSITIVE"));
    }

    #[tokio::test]
    async fn test_chat_completion_api_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(429).set_body_string("Rate limited"))
            .mount(&mock_server)
            .await;

        let client = QwenClient::new("test-key")
            .unwrap()
            .with_base_url(mock_server.uri());
        let result = client
            .chat_completion("You are a reviewer.", "diff content", 0.1)
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("429"));
        assert!(err.contains("qwen"));
    }
}
