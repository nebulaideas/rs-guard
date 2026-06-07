//! DeepSeek LLM provider implementation.
//!
//! Communicates with the DeepSeek chat completions API using an
//! OpenAI-compatible request format.

use crate::error::DiffguardError;
use crate::llm::{ChatMessage, ChatRequest, ChatResponse, LlmError};
use reqwest::header::{self, HeaderMap, HeaderValue};

/// Default DeepSeek API base URL.
const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";

/// Default model identifier for DeepSeek.
const DEFAULT_MODEL: &str = "deepseek-v4-flash";

/// HTTP request timeout for LLM API calls.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Client for the DeepSeek chat completions API.
#[derive(Debug, Clone)]
pub struct DeepSeekClient {
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl DeepSeekClient {
    /// Creates a new DeepSeek client with the given API key.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key contains invalid header characters
    /// or if the HTTP client cannot be built.
    pub fn new(api_key: impl Into<String>) -> Result<Self, DiffguardError> {
        let api_key = api_key.into();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", api_key)).map_err(|e| {
                DiffguardError::Config(format!("Invalid DeepSeek API key format: {}", e))
            })?,
        );
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| DiffguardError::Config(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
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

    /// Sends a chat completion request to the DeepSeek API.
    ///
    /// # Arguments
    ///
    /// * `system_prompt` — The system instruction for the model.
    /// * `user_message` — The user message (typically the diff content).
    /// * `temperature` — Sampling temperature (0.0 to 2.0).
    ///
    /// # Errors
    ///
    /// Returns [`DiffguardError::LlmApi`] on API errors or response parsing failures.
    pub async fn chat_completion(
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
        };

        let url = format!("{}/chat/completions", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                let status = e.status().map(|s| s.as_u16()).unwrap_or(0);
                LlmError {
                    provider: "deepseek".to_string(),
                    status,
                    message: e.to_string(),
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError {
                provider: "deepseek".to_string(),
                status: status.as_u16(),
                message: body,
            }
            .into());
        }

        let chat_response: ChatResponse = response.json().await.map_err(|e| LlmError {
            provider: "deepseek".to_string(),
            status: 0,
            message: format!("Failed to parse response: {}", e),
        })?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| {
                LlmError {
                    provider: "deepseek".to_string(),
                    status: 0,
                    message: "Empty response from LLM".to_string(),
                }
                .into()
            })
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
                        "content": "This looks good.\n\n[DIFFGUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 0\nSecurityIssues: 0"
                    }
                }]
            })))
            .mount(&mock_server)
            .await;

        let client = DeepSeekClient::new("test-key")
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

        let client = DeepSeekClient::new("test-key")
            .unwrap()
            .with_base_url(mock_server.uri());
        let result = client
            .chat_completion("You are a reviewer.", "diff content", 0.1)
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("429"));
    }
}
