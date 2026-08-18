//! llm-kernel-backed LLM provider client.
//!
//! Wraps [`llm_kernel::llm::OpenAIClient`] for the 7 standard OpenAI-compatible
//! providers that do not need `result_format` (Qwen) or `extra_body` (Kimi
//! thinking variants). Per-provider differences (base URL, model, attribution
//! headers, timeout) are expressed through the pre-built [`reqwest::Client`]
//! passed to the underlying `OpenAIClient`.
//!
//! This type is `pub(crate)` — it is not part of the public API. Provider
//! instances are constructed exclusively via [`super::factory::create_provider`].

use crate::error::RsGuardError;
use crate::llm::{providers, ChatCompletionResult, LlmError, LlmProvider, TokenUsage};
use async_trait::async_trait;
use llm_kernel::error::KernelError;
use llm_kernel::llm::{LLMClient, LLMRequest, OpenAIClient};

use super::providers::ProviderMeta;

/// llm-kernel-backed client for standard OpenAI-compatible providers.
///
/// Holds a [`ProviderMeta`] reference for variant resolution and an
/// [`OpenAIClient`] that handles HTTP transport. The `OpenAIClient` is
/// constructed with a shared `reqwest::Client` that carries provider-specific
/// headers (e.g. OpenRouter attribution) and timeout settings.
///
/// Providers that require `result_format` (Qwen) or `extra_body` (Kimi thinking
/// variants) are served by [`super::generic_client::GenericOpenAiCompatibleClient`]
/// instead, because `LLMRequest` does not expose those fields.
pub(crate) struct KernelBackedClient {
    /// Static provider metadata (name, defaults, variants).
    meta: &'static ProviderMeta,
    /// Effective model identifier (pre-variant-resolution).
    model: String,
    /// Optional provider-specific variant.
    variant: Option<String>,
    /// Optional maximum tokens cap.
    max_tokens: Option<u32>,
    /// The llm-kernel OpenAI-compatible client.
    client: OpenAIClient,
}

impl std::fmt::Debug for KernelBackedClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KernelBackedClient")
            .field("meta", &self.meta.name)
            .field("model", &self.model)
            .field("variant", &self.variant)
            .finish_non_exhaustive()
    }
}

impl KernelBackedClient {
    /// Creates a new kernel-backed client for the given provider.
    ///
    /// The `reqwest::Client` is built once with the provider's default headers
    /// and any config-supplied overrides, then passed to [`OpenAIClient`].
    ///
    /// # Arguments
    ///
    /// * `meta` — Static provider metadata.
    /// * `api_key` — API key for Bearer authentication (empty for Ollama).
    /// * `base_url` — API base URL (e.g. `https://api.deepseek.com`).
    /// * `model` — Model identifier to use.
    /// * `extra_header_overrides` — Additional headers merged on top of the
    ///   provider's defaults (e.g. custom OpenRouter referer).
    /// * `timeout_secs` — Optional request timeout override.
    ///
    /// # Errors
    ///
    /// Returns [`RsGuardError::Config`] if the HTTP client cannot be built.
    pub(crate) fn new(
        meta: &'static ProviderMeta,
        api_key: &str,
        base_url: &str,
        model: &str,
        extra_header_overrides: &[(&str, &str)],
        timeout_secs: Option<u64>,
    ) -> Result<Self, RsGuardError> {
        // Merge the provider's default headers with config-supplied overrides.
        let mut headers: Vec<(&str, &str)> = meta.default_extra_headers.to_vec();
        for &(ov_name, ov_value) in extra_header_overrides {
            if let Some(slot) = headers.iter_mut().find(|(n, _)| *n == ov_name) {
                slot.1 = ov_value;
            } else {
                headers.push((ov_name, ov_value));
            }
        }

        let timeout = timeout_secs.map(std::time::Duration::from_secs).unwrap_or(
            std::time::Duration::from_secs(crate::config::DEFAULT_LLM_TIMEOUT_SECS),
        );

        let http_client = crate::llm::build_llm_client(meta.name, api_key, &headers, timeout)?;

        // OpenAIClient::from_key_with_base_url takes the model, key, base URL,
        // and a shared reqwest::Client. The model stored here is the
        // pre-variant-resolution default; variant resolution happens at
        // chat_completion time and overrides the model in the LLMRequest.
        let client = OpenAIClient::from_key_with_base_url(
            model.to_string(),
            api_key.to_string(),
            base_url.to_string(),
            http_client,
        );

        Ok(Self {
            meta,
            model: model.to_string(),
            variant: None,
            max_tokens: None,
            client,
        })
    }

    /// Sets a provider-specific model variant.
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
impl LlmProvider for KernelBackedClient {
    fn name(&self) -> &'static str {
        self.meta.name
    }

    async fn chat_completion(
        &self,
        system_prompt: &str,
        user_message: &str,
        temperature: f32,
    ) -> Result<ChatCompletionResult, RsGuardError> {
        // Resolve variant (ModelAlias only — ExtraBody providers use GenericClient).
        let (effective_model, _extra_body) =
            providers::apply_variant(self.meta.name, &self.model, self.variant.as_deref())?;

        // Build the LLMRequest using llm-kernel's types.
        let request = LLMRequest {
            system: Some(system_prompt.to_string()),
            messages: vec![llm_kernel::llm::ChatMessage::user(user_message.to_string())],
            temperature,
            max_tokens: self.max_tokens,
            model: Some(effective_model),
            response_format: None,
            tools: None,
        };

        // Delegate to llm-kernel's OpenAIClient.
        let response = self
            .client
            .complete(request)
            .await
            .map_err(|e| map_kernel_error(e, self.meta.name))?;

        // Map llm-kernel's LLMResponse → rs-guard's ChatCompletionResult.
        //
        // llm-kernel's OpenAIClient promotes reasoning_content into content
        // when the original content is empty (for GLM-4.7, DeepSeek-R1, etc.).
        // However, rs-guard's pipeline treats "empty content with reasoning"
        // as a retryable budget-exhaustion failure (the model spent its token
        // budget on chain-of-thought and never produced a final answer). We
        // detect the promotion by checking if content equals reasoning, and
        // preserve the old behavior by returning an error with the
        // REASONING_BUDGET_EXHAUSTED_MARKER.
        if let Some(ref reasoning) = response.reasoning {
            if response.content == *reasoning && !reasoning.is_empty() {
                // Content was promoted from reasoning — the model exhausted
                // its token budget on chain-of-thought and never produced a
                // final answer. Treat as a retryable failure.
                let message = format!(
                    "Empty assistant content from LLM (reasoning_content: {} chars; {})",
                    reasoning.len(),
                    crate::error::REASONING_BUDGET_EXHAUSTED_MARKER
                );
                return Err(LlmError {
                    provider: self.meta.name.to_string(),
                    status: 0,
                    message,
                }
                .into());
            }
        }

        if response.content.trim().is_empty() {
            let reasoning_len = response.reasoning.as_ref().map(|r| r.len()).unwrap_or(0);
            let message = if reasoning_len > 0 {
                format!(
                    "Empty assistant content from LLM (reasoning_content: {reasoning_len} chars; {})",
                    crate::error::REASONING_BUDGET_EXHAUSTED_MARKER
                )
            } else {
                "Empty assistant content from LLM (no reasoning content returned)".to_string()
            };
            return Err(LlmError {
                provider: self.meta.name.to_string(),
                status: 0,
                message,
            }
            .into());
        }

        // Map llm-kernel's TokenUsage (u32 fields, default to 0) to rs-guard's
        // TokenUsage (Option<u64> fields). When the provider doesn't return
        // usage data, llm-kernel defaults all fields to 0 — we treat all-zero
        // as absent (None) so the pipeline falls back to character-based
        // estimation instead of reporting misleading zero-token metrics.
        let usage = if response.usage.prompt_tokens == 0
            && response.usage.completion_tokens == 0
            && response.usage.total_tokens == 0
        {
            None
        } else {
            Some(TokenUsage {
                prompt_tokens: Some(response.usage.prompt_tokens as u64),
                completion_tokens: Some(response.usage.completion_tokens as u64),
                total_tokens: Some(response.usage.total_tokens as u64),
            })
        };

        Ok(ChatCompletionResult {
            content: response.content,
            usage,
        })
    }
}

/// Maps a `llm_kernel::KernelError` to `RsGuardError::LlmApi`, preserving the
/// HTTP status code so that rs-guard's retry logic (`is_retryable`) works
/// correctly.
///
/// - `KernelError::Http { status, .. }` → preserves the HTTP status code
///   (5xx and 429 are retryable; 4xx are not)
/// - `KernelError::RateLimited(_)` → status 429 (retryable)
/// - `KernelError::Timeout(_)` → status 0 (retryable as a connection error)
/// - `KernelError::Config(_)` → status 400 (not retryable — permanent failure)
/// - `KernelError::Serialization(_)` → status 400 (not retryable — bad response)
/// - All other variants → status 0 (retryable as a transient error)
fn map_kernel_error(e: KernelError, provider_name: &str) -> RsGuardError {
    let (status, message) = match e {
        KernelError::Http { status, message } => (status, message),
        KernelError::RateLimited(retry_after) => {
            (429, format!("Rate limited (retry after {retry_after}s)"))
        }
        KernelError::Timeout(secs) => (0, format!("Request timed out after {secs}s")),
        KernelError::Config(msg) => (400, format!("Config error: {msg}")),
        KernelError::Serialization(e) => (400, format!("Serialization error: {e}")),
        other => (0, other.to_string()),
    };
    RsGuardError::LlmApi {
        provider: provider_name.to_string(),
        status,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_kernel_error_http_preserves_status() {
        let err = map_kernel_error(
            KernelError::Http {
                status: 500,
                message: "server error".to_string(),
            },
            "deepseek",
        );
        match err {
            RsGuardError::LlmApi { status, .. } => assert_eq!(status, 500),
            _ => panic!("expected LlmApi"),
        }
    }

    #[test]
    fn test_map_kernel_error_rate_limited_is_retryable() {
        let err = map_kernel_error(KernelError::RateLimited(30), "deepseek");
        assert!(err.is_retryable());
        match err {
            RsGuardError::LlmApi { status, .. } => assert_eq!(status, 429),
            _ => panic!("expected LlmApi"),
        }
    }

    #[test]
    fn test_map_kernel_error_timeout_is_retryable() {
        let err = map_kernel_error(KernelError::Timeout(60), "deepseek");
        assert!(err.is_retryable());
    }

    #[test]
    fn test_map_kernel_error_config_not_retryable() {
        let err = map_kernel_error(KernelError::Config("missing field".to_string()), "deepseek");
        assert!(!err.is_retryable());
        match err {
            RsGuardError::LlmApi { status, .. } => assert_eq!(status, 400),
            _ => panic!("expected LlmApi"),
        }
    }

    #[test]
    fn test_map_kernel_error_serialization_not_retryable() {
        let serde_err = serde_json::from_str::<serde_json::Value>("bad json").unwrap_err();
        let err = map_kernel_error(KernelError::Serialization(serde_err), "deepseek");
        assert!(!err.is_retryable());
    }
}
