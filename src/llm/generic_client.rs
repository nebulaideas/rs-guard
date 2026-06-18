//! Generic OpenAI-compatible LLM provider client.
//!
//! Single, data-driven implementation of the OpenAI `/chat/completions` flow
//! shared by every supported provider (deepseek, kimi, qwen, openrouter,
//! openai, grok, glm, and any future OpenAI-compatible endpoint).
//!
//! Per-provider differences are expressed purely through [`ProviderMeta`]:
//!
//! - `result_format` — injects a `result_format` field into the request body
//!   (Qwen/DashScope requires `"message"`).
//! - `default_extra_headers` — default HTTP headers attached to every request
//!   (OpenRouter attribution via `HTTP-Referer` + `X-Title`).
//!
//! All other behaviour (variant resolution, extra-body flattening,
//! `reasoning_content` handling, auth, timeouts, error reporting) is handled
//! by the shared infrastructure in [`super`]: [`super::apply_variant`],
//! [`super::build_llm_client`], [`super::send_chat_request`], and
//! [`super::chat_messages`].
//!
//! This type is `pub(crate)` — it is not part of the public API. Provider
//! instances are constructed exclusively via [`super::factory::create_provider`].

use crate::error::RsGuardError;
use crate::llm::{
    build_llm_client, chat_messages, providers, send_chat_request, ChatMessage, LlmProvider,
};
use async_trait::async_trait;
use serde::Serialize;
use std::collections::HashMap;

use super::providers::ProviderMeta;

/// Generic OpenAI-compatible chat-completions client.
///
/// Holds a reference to the provider's [`ProviderMeta`] plus the per-instance
/// configuration (base URL, model, variant, max tokens) resolved from CLI/TOML.
/// The HTTP client is built once at construction time with the provider's
/// default headers and any config-supplied overrides.
#[derive(Debug, Clone)]
pub(crate) struct GenericOpenAiCompatibleClient {
    /// Static provider metadata (name, defaults, hooks).
    meta: &'static ProviderMeta,
    /// Effective API base URL.
    base_url: String,
    /// Effective model identifier (pre-variant-resolution).
    model: String,
    /// Optional provider-specific variant.
    variant: Option<String>,
    /// Optional maximum tokens cap.
    max_tokens: Option<u32>,
    /// Pre-built reqwest client with auth + provider headers.
    client: reqwest::Client,
}

/// Serialisable chat request that optionally includes a `result_format` field.
///
/// Mirrors [`super::ChatRequest`] but adds the `result_format` field required
/// by some OpenAI-compatible providers (e.g. Qwen/DashScope). When
/// `result_format` is `None` the field is omitted from the serialized body,
/// producing a standard OpenAI request shape.
#[derive(Debug, Serialize)]
struct GenericChatRequest {
    /// Model identifier to use for completion.
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<ChatMessage>,
    /// Sampling temperature (0.0 to 2.0).
    pub temperature: f32,
    /// Optional result format (e.g. `"message"` for Qwen).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_format: Option<&'static str>,
    /// Maximum tokens in the response (provider-agnostic).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Extra top-level fields contributed by `VariantEffect::ExtraBody`,
    /// flattened into the request body.
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra_body: HashMap<String, serde_json::Value>,
}

impl GenericOpenAiCompatibleClient {
    /// Creates a new generic client bound to the given provider metadata.
    ///
    /// The HTTP client is built once with the provider's
    /// [`ProviderMeta::default_extra_headers`] merged with any
    /// `extra_header_overrides` supplied by the factory (e.g. a custom
    /// OpenRouter referer). Overrides take precedence over defaults for the
    /// same header name.
    ///
    /// # Arguments
    ///
    /// * `meta` — Static provider metadata.
    /// * `api_key` — API key for Bearer authentication.
    /// * `extra_header_overrides` — Additional headers merged on top of the
    ///   provider's defaults (later entries win on name collision).
    ///
    /// # Errors
    ///
    /// Returns [`RsGuardError::Config`] if the API key or any header value
    /// contains invalid HTTP header characters, or if the HTTP client cannot
    /// be built.
    pub(crate) fn new(
        meta: &'static ProviderMeta,
        api_key: &str,
        extra_header_overrides: &[(&str, &str)],
    ) -> Result<Self, RsGuardError> {
        // Merge the provider's default headers with config-supplied overrides.
        // Later entries (overrides) win on name collision: build an ordered
        // vec of defaults, then replace any matching name with the override
        // value, appending brand-new override names.
        let mut headers: Vec<(&str, &str)> = meta.default_extra_headers.to_vec();
        for &(ov_name, ov_value) in extra_header_overrides {
            if let Some(slot) = headers.iter_mut().find(|(n, _)| *n == ov_name) {
                slot.1 = ov_value;
            } else {
                headers.push((ov_name, ov_value));
            }
        }

        let client = build_llm_client(meta.name, api_key, &headers)?;
        Ok(Self {
            meta,
            base_url: meta.default_base_url.to_string(),
            model: meta.default_model.to_string(),
            variant: None,
            max_tokens: None,
            client,
        })
    }

    /// Sets a custom base URL for the API endpoint.
    pub(crate) fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Sets a custom model identifier.
    pub(crate) fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Sets a provider-specific model variant.
    ///
    /// Only has an effect for providers that declare variants in
    /// [`providers`]. See [`providers::VariantEffect`] and the provider
    /// metadata tables in `docs/PROVIDERS.md`.
    pub(crate) fn with_variant(mut self, variant: Option<String>) -> Self {
        self.variant = variant;
        self
    }

    /// Sets the maximum tokens for completions.
    pub(crate) fn with_max_tokens(mut self, max_tokens: Option<u32>) -> Self {
        self.max_tokens = max_tokens;
        self
    }
}

#[async_trait]
impl LlmProvider for GenericOpenAiCompatibleClient {
    fn name(&self) -> &'static str {
        self.meta.name
    }

    async fn chat_completion(
        &self,
        system_prompt: &str,
        user_message: &str,
        temperature: f32,
    ) -> Result<String, RsGuardError> {
        let (effective_model, extra_body) =
            providers::apply_variant(self.meta.name, &self.model, self.variant.as_deref())?;

        let request = GenericChatRequest {
            model: effective_model,
            messages: chat_messages(system_prompt, user_message),
            temperature,
            result_format: self.meta.result_format,
            max_tokens: self.max_tokens,
            extra_body,
        };

        let url = format!("{}/chat/completions", self.base_url);
        send_chat_request(&self.client, &url, &request, self.meta.name).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::providers::find_provider;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Helper: build a generic client pointed at the given mock server for a
    /// named provider, with no model/variant overrides.
    fn build(provider_name: &str, server_uri: &str) -> GenericOpenAiCompatibleClient {
        let meta = find_provider(provider_name)
            .unwrap_or_else(|| panic!("provider '{}' must be registered", provider_name));
        GenericOpenAiCompatibleClient::new(meta, "test-key", &[])
            .unwrap()
            .with_base_url(server_uri.to_string())
    }

    #[tokio::test]
    async fn test_generic_success_and_dynamic_name() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "content": "OK" } }]
            })))
            .mount(&mock_server)
            .await;

        // DeepSeek path: name() must come from metadata, never hardcoded.
        let client = build("deepseek", &mock_server.uri());
        assert_eq!(client.name(), "deepseek");
        let result = client.chat_completion("system", "user", 0.1).await.unwrap();
        assert_eq!(result, "OK");
    }

    #[tokio::test]
    async fn test_generic_name_not_hardcoded_openai_for_grok() {
        // Regression guard: the old OpenAiClient hardcoded "openai". Grok via
        // the generic must report its own name.
        let client = build("grok", "http://unused.invalid");
        assert_eq!(client.name(), "grok");
    }

    #[tokio::test]
    async fn test_generic_name_not_hardcoded_openai_for_glm() {
        let client = build("glm", "http://unused.invalid");
        assert_eq!(client.name(), "glm");
    }

    #[tokio::test]
    async fn test_qwen_result_format_injected_into_body() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(serde_json::json!({
                "result_format": "message"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "content": "qwen ok" } }]
            })))
            .mount(&mock_server)
            .await;

        let client = build("qwen", &mock_server.uri());
        let result = client.chat_completion("system", "user", 0.1).await.unwrap();
        assert_eq!(result, "qwen ok");
    }

    #[tokio::test]
    async fn test_standard_provider_omits_result_format() {
        // Standard OpenAI-shaped providers must NOT send result_format. We
        // inspect the recorded request body to assert the key's absence
        // (a positive body matcher cannot express "key absent").
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "content": "ok" } }]
            })))
            .mount(&mock_server)
            .await;

        let client = build("openai", &mock_server.uri());
        let _ = client.chat_completion("system", "user", 0.1).await.unwrap();

        let requests = mock_server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert!(
            body.get("result_format").is_none(),
            "standard provider must not send result_format; got: {}",
            body
        );
    }

    #[tokio::test]
    async fn test_openrouter_default_attribution_headers_sent() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header(
                "HTTP-Referer",
                "https://github.com/nebulaideas/rs-guard",
            ))
            .and(header("X-Title", "rs-guard"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "content": "openrouter ok" } }]
            })))
            .mount(&mock_server)
            .await;

        let client = build("openrouter", &mock_server.uri());
        let result = client.chat_completion("system", "user", 0.1).await.unwrap();
        assert_eq!(result, "openrouter ok");
    }

    #[tokio::test]
    async fn test_openrouter_referer_override_replaces_default() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("HTTP-Referer", "https://my-bot.example.com"))
            .and(header("X-Title", "rs-guard"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "content": "override ok" } }]
            })))
            .mount(&mock_server)
            .await;

        let meta = find_provider("openrouter").unwrap();
        let client = GenericOpenAiCompatibleClient::new(
            meta,
            "test-key",
            &[("HTTP-Referer", "https://my-bot.example.com")],
        )
        .unwrap()
        .with_base_url(mock_server.uri());
        let result = client.chat_completion("system", "user", 0.1).await.unwrap();
        assert_eq!(result, "override ok");
    }

    #[tokio::test]
    async fn test_kimi_thinking_on_extra_body_flows_through() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(serde_json::json!({
                "thinking": { "type": "enabled" }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "content": "kimi ok" } }]
            })))
            .mount(&mock_server)
            .await;

        let client =
            build("kimi", &mock_server.uri()).with_variant(Some("thinking-on".to_string()));
        let result = client.chat_completion("system", "user", 0.1).await.unwrap();
        assert_eq!(result, "kimi ok");
    }

    #[tokio::test]
    async fn test_deepseek_variant_flash_maps_to_model_alias() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(serde_json::json!({
                "model": "deepseek-v4-flash"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "content": "flash ok" } }]
            })))
            .mount(&mock_server)
            .await;

        let client = build("deepseek", &mock_server.uri())
            .with_model("ignored-base")
            .with_variant(Some("flash".to_string()));
        let result = client.chat_completion("system", "user", 0.1).await.unwrap();
        assert_eq!(result, "flash ok");
    }

    #[tokio::test]
    async fn test_grok_uses_xai_base_url() {
        // Smoke: grok client's default base URL must be the xAI endpoint.
        let meta = find_provider("grok").unwrap();
        let client = GenericOpenAiCompatibleClient::new(meta, "test-key", &[]).unwrap();
        assert_eq!(client.base_url, "https://api.x.ai/v1");
        assert_eq!(client.model, "grok-3");
    }

    #[tokio::test]
    async fn test_glm_uses_zhipu_base_url() {
        let meta = find_provider("glm").unwrap();
        let client = GenericOpenAiCompatibleClient::new(meta, "test-key", &[]).unwrap();
        assert_eq!(client.base_url, "https://open.bigmodel.cn/api/paas/v4");
        assert_eq!(client.model, "glm-4");
    }

    #[tokio::test]
    async fn test_generic_http_error_reports_provider_name() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&mock_server)
            .await;

        let client = build("deepseek", &mock_server.uri());
        let err = client
            .chat_completion("system", "user", 0.1)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("429"), "expected 429 in error, got: {}", msg);
        assert!(
            msg.contains("deepseek"),
            "error should name the provider, got: {}",
            msg
        );
    }
}
