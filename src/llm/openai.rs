//! Generic OpenAI-compatible LLM provider implementation.
//!
//! Catch-all for any OpenAI-compatible endpoint. Configurable base URL
//! and model identifier.

use crate::error::DiffguardError;
use crate::llm::{build_llm_client, chat_messages, send_chat_request, ChatRequest, LlmProvider};
use async_trait::async_trait;

/// Default OpenAI API base URL.
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// Default model identifier for OpenAI.
const DEFAULT_MODEL: &str = "gpt-4o-mini";

/// Client for generic OpenAI-compatible chat completions APIs.
#[derive(Debug, Clone)]
pub struct OpenAiClient {
    base_url: String,
    model: String,
    max_tokens: Option<u32>,
    client: reqwest::Client,
}

impl OpenAiClient {
    /// Creates a new OpenAI-compatible client with the given API key.
    pub fn new(api_key: impl Into<String>) -> Result<Self, DiffguardError> {
        let client = build_llm_client("openai", &api_key.into(), &[])?;
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
impl LlmProvider for OpenAiClient {
    fn name(&self) -> &'static str {
        "openai"
    }

    async fn chat_completion(
        &self,
        system_prompt: &str,
        user_message: &str,
        temperature: f32,
    ) -> Result<String, DiffguardError> {
        let request = ChatRequest {
            model: self.model.clone(),
            messages: chat_messages(system_prompt, user_message),
            temperature,
            max_tokens: self.max_tokens,
        };

        let url = format!("{}/chat/completions", self.base_url);
        send_chat_request(&self.client, &url, &request, "openai").await
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

        let client = OpenAiClient::new("test-key")
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

        let client = OpenAiClient::new("test-key")
            .unwrap()
            .with_base_url(mock_server.uri());
        let result = client
            .chat_completion("You are a reviewer.", "diff content", 0.1)
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("429"));
        assert!(err.contains("openai"));
    }

    #[tokio::test]
    async fn test_custom_base_url() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": { "content": "Custom endpoint works." }
                }]
            })))
            .mount(&mock_server)
            .await;

        let client = OpenAiClient::new("test-key")
            .unwrap()
            .with_base_url(mock_server.uri())
            .with_model("custom-model");
        let result = client
            .chat_completion("You are a reviewer.", "diff content", 0.1)
            .await;

        assert!(result.is_ok());
        assert!(result.unwrap().contains("Custom endpoint works"));
    }
}
