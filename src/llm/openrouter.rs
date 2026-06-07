//! OpenRouter LLM gateway provider implementation.
//!
//! Communicates with the OpenRouter unified API, routing to any supported
//! model. Requires `HTTP-Referer` and `X-Title` headers for attribution.

use crate::error::DiffguardError;
use crate::llm::{build_llm_client, chat_messages, send_chat_request, ChatRequest, LlmProvider};
use async_trait::async_trait;

/// Default OpenRouter API base URL.
const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// Default model identifier for OpenRouter.
const DEFAULT_MODEL: &str = "openai/gpt-4o-mini";

/// Default HTTP referer for attribution.
const DEFAULT_HTTP_REFERER: &str = "https://github.com/nebulaideas/diffguard-rs";

/// Client for the OpenRouter chat completions API.
#[derive(Debug, Clone)]
pub struct OpenRouterClient {
    base_url: String,
    model: String,
    max_tokens: Option<u32>,
    client: reqwest::Client,
}

impl OpenRouterClient {
    /// Creates a new OpenRouter client with the given API key.
    pub fn new(api_key: impl Into<String>) -> Result<Self, DiffguardError> {
        let api_key_str = api_key.into();
        let extra_headers = &[
            ("HTTP-Referer", DEFAULT_HTTP_REFERER),
            ("X-Title", "diffguard"),
        ];
        let client = build_llm_client("openrouter", &api_key_str, extra_headers)?;
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

    /// Rebuilds the HTTP client with a custom HTTP referer header.
    ///
    /// # Errors
    ///
    /// Returns [`DiffguardError::Config`] if the referer value contains
    /// invalid header characters.
    pub fn with_http_referer(self, referer: &str, api_key: &str) -> Result<Self, DiffguardError> {
        let extra_headers = &[("HTTP-Referer", referer), ("X-Title", "diffguard")];
        let client = build_llm_client("openrouter", api_key, extra_headers)?;
        Ok(Self { client, ..self })
    }
}

#[async_trait]
impl LlmProvider for OpenRouterClient {
    fn name(&self) -> &'static str {
        "openrouter"
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
        send_chat_request(&self.client, &url, &request, "openrouter").await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
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

        let client = OpenRouterClient::new("test-key")
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

        let client = OpenRouterClient::new("test-key")
            .unwrap()
            .with_base_url(mock_server.uri());
        let result = client
            .chat_completion("You are a reviewer.", "diff content", 0.1)
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("429"));
        assert!(err.contains("openrouter"));
    }

    #[tokio::test]
    async fn test_referer_header_sent() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("HTTP-Referer", "https://example.com"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": { "content": "OK" }
                }]
            })))
            .mount(&mock_server)
            .await;

        let client = OpenRouterClient::new("test-key")
            .unwrap()
            .with_base_url(mock_server.uri())
            .with_http_referer("https://example.com", "test-key")
            .unwrap();
        let result = client
            .chat_completion("You are a reviewer.", "diff content", 0.1)
            .await;

        assert!(result.is_ok());
    }
}
