//! OpenRouter LLM gateway provider implementation.
//!
//! Communicates with the OpenRouter unified API, routing to any supported
//! model. Requires `HTTP-Referer` and `X-Title` headers for attribution.

use crate::error::DiffguardError;
use crate::llm::{send_chat_request, ChatMessage, ChatRequest, LlmProvider};
use async_trait::async_trait;
use reqwest::header::{self, HeaderMap, HeaderValue};

/// Default OpenRouter API base URL.
const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// Default model identifier for OpenRouter.
const DEFAULT_MODEL: &str = "openai/gpt-4o-mini";

/// Default HTTP referer for attribution.
const DEFAULT_HTTP_REFERER: &str = "https://github.com/nebulaideas/diffguard-rs";

/// HTTP request timeout for LLM API calls.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

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
        let client = Self::build_client(&api_key.into(), DEFAULT_HTTP_REFERER)?;
        Ok(Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            max_tokens: None,
            client,
        })
    }

    fn build_client(api_key: &str, referer: &str) -> Result<reqwest::Client, DiffguardError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", api_key)).map_err(|e| {
                DiffguardError::Config(format!("Invalid OpenRouter API key format: {}", e))
            })?,
        );
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            "HTTP-Referer",
            HeaderValue::from_str(referer).map_err(|e| {
                DiffguardError::Config(format!("Invalid HTTP-Referer value: {}", e))
            })?,
        );
        headers.insert("X-Title", HeaderValue::from_static("diffguard"));

        reqwest::Client::builder()
            .default_headers(headers)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| DiffguardError::Config(format!("Failed to build HTTP client: {}", e)))
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
    pub fn with_http_referer(
        mut self,
        referer: &str,
        api_key: &str,
    ) -> Result<Self, DiffguardError> {
        self.client = Self::build_client(api_key, referer)?;
        Ok(self)
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
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user_message.to_string(),
                },
            ],
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
