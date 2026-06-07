//! Kimi (Moonshot AI) LLM provider implementation.
//!
//! Communicates with the Kimi chat completions API using an
//! OpenAI-compatible request format with `reasoning_content` support.

use crate::error::DiffguardError;
use crate::llm::{build_llm_client, chat_messages, send_chat_request, ChatRequest, LlmProvider};
use async_trait::async_trait;

/// Default Kimi API base URL.
const DEFAULT_BASE_URL: &str = "https://api.moonshot.ai/v1";

/// Default model identifier for Kimi.
const DEFAULT_MODEL: &str = "kimi-k2.5";

/// Client for the Kimi chat completions API.
#[derive(Debug, Clone)]
pub struct KimiClient {
    base_url: String,
    model: String,
    max_tokens: Option<u32>,
    client: reqwest::Client,
}

impl KimiClient {
    /// Creates a new Kimi client with the given API key.
    pub fn new(api_key: impl Into<String>) -> Result<Self, DiffguardError> {
        let client = build_llm_client("kimi", &api_key.into(), &[])?;
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
impl LlmProvider for KimiClient {
    fn name(&self) -> &'static str {
        "kimi"
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
        send_chat_request(&self.client, &url, &request, "kimi").await
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

        let client = KimiClient::new("test-key")
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

        let client = KimiClient::new("test-key")
            .unwrap()
            .with_base_url(mock_server.uri());
        let result = client
            .chat_completion("You are a reviewer.", "diff content", 0.1)
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("429"));
        assert!(err.contains("kimi"));
    }

    #[tokio::test]
    async fn test_reasoning_content_parsed() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": "Final answer.",
                        "reasoning_content": "Let me think..."
                    }
                }]
            })))
            .mount(&mock_server)
            .await;

        let client = KimiClient::new("test-key")
            .unwrap()
            .with_base_url(mock_server.uri());
        let result = client
            .chat_completion("You are a reviewer.", "diff content", 0.1)
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Final answer.");
    }
}
