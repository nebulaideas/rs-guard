//! DeepSeek LLM provider implementation.
//!
//! Communicates with the DeepSeek chat completions API using an
//! OpenAI-compatible request format.

use crate::error::RsGuardError;
use crate::llm::{
    build_llm_client, chat_messages, providers, send_chat_request, ChatRequest, LlmProvider,
    VariantEffect,
};
use async_trait::async_trait;

/// Default DeepSeek API base URL.
const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";

/// Default model identifier for DeepSeek.
const DEFAULT_MODEL: &str = "deepseek-v4-flash";

/// Client for the DeepSeek chat completions API.
#[derive(Debug, Clone)]
pub struct DeepSeekClient {
    base_url: String,
    model: String,
    variant: Option<String>,
    max_tokens: Option<u32>,
    client: reqwest::Client,
}

impl DeepSeekClient {
    /// Creates a new DeepSeek client with the given API key.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key contains invalid header characters
    /// or if the HTTP client cannot be built.
    pub fn new(api_key: impl Into<String>) -> Result<Self, RsGuardError> {
        let client = build_llm_client("deepseek", &api_key.into(), &[])?;
        Ok(Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            variant: None,
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

    /// Sets a provider-specific model variant.
    pub fn with_variant(mut self, variant: Option<String>) -> Self {
        self.variant = variant;
        self
    }

    /// Sets the maximum tokens for completions.
    pub fn with_max_tokens(mut self, max_tokens: Option<u32>) -> Self {
        self.max_tokens = max_tokens;
        self
    }
}

#[async_trait]
impl LlmProvider for DeepSeekClient {
    fn name(&self) -> &'static str {
        "deepseek"
    }

    async fn chat_completion(
        &self,
        system_prompt: &str,
        user_message: &str,
        temperature: f32,
    ) -> Result<String, RsGuardError> {
        let effective_model = if let Some(ref variant) = self.variant {
            match providers::find_provider_variant("deepseek", variant) {
                Some(v) => match &v.effect {
                    VariantEffect::ModelAlias(model) => model.to_string(),
                    VariantEffect::ExtraBody(_, _) => self.model.clone(),
                },
                None => {
                    let known = providers::provider_variant_names("deepseek").join(", ");
                    return Err(RsGuardError::Config(format!(
                        "Unknown variant '{}' for provider 'deepseek'. Supported variants: {}",
                        variant, known
                    )));
                }
            }
        } else {
            self.model.clone()
        };

        let request = ChatRequest {
            model: effective_model,
            messages: chat_messages(system_prompt, user_message),
            temperature,
            max_tokens: self.max_tokens,
        };

        let url = format!("{}/chat/completions", self.base_url);
        send_chat_request(&self.client, &url, &request, "deepseek").await
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
                        "content": "This looks good.\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 0\nSecurityIssues: 0"
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
