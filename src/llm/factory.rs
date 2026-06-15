//! Provider factory for creating LLM provider instances by name.
//!
//! # Tech Debt
//!
//! The five provider implementations (deepseek, kimi, qwen, openrouter, openai)
//! share ~500 lines of near-identical struct fields, builder methods, and
//! `chat_completion` logic. Only `OpenRouterClient::with_http_referer` and
//! `QwenChatRequest::result_format` differ. These should be consolidated into
//! a single `GenericOpenAiClient` parameterized by [`crate::llm::providers::ProviderMeta`],
//! with per-provider customisation hooks for headers and request schemas.

use crate::error::RsGuardError;
use crate::llm::{
    deepseek::DeepSeekClient, kimi::KimiClient, openai::OpenAiClient, openrouter::OpenRouterClient,
    providers, qwen::QwenClient, Provider, ProviderConfig,
};

/// Creates an LLM provider instance based on the given provider name.
///
/// # Arguments
///
/// * `provider_name` — Provider identifier (e.g. `"deepseek"`).
/// * `api_key` — API key for authenticating with the provider.
/// * `config` — Provider configuration overrides from `.reviewer.toml` and CLI.
///
/// # Errors
///
/// Returns [`RsGuardError::Config`] if the provider name is unknown
/// or if the API key contains invalid characters.
pub fn create_provider(
    provider_name: &str,
    api_key: &str,
    config: &ProviderConfig,
) -> Result<Provider, RsGuardError> {
    match provider_name {
        "deepseek" => {
            let mut client = DeepSeekClient::new(api_key)?;
            if let Some(ref url) = config.base_url {
                client = client.with_base_url(url.clone());
            }
            client = client
                .with_model(config.model.clone())
                .with_variant(config.variant.clone())
                .with_max_tokens(config.max_tokens);
            Ok(Box::new(client))
        }
        "kimi" => {
            let mut client = KimiClient::new(api_key)?;
            if let Some(ref url) = config.base_url {
                client = client.with_base_url(url.clone());
            }
            client = client
                .with_model(config.model.clone())
                .with_variant(config.variant.clone())
                .with_max_tokens(config.max_tokens);
            Ok(Box::new(client))
        }
        "qwen" => {
            let mut client = QwenClient::new(api_key)?;
            if let Some(ref url) = config.base_url {
                client = client.with_base_url(url.clone());
            }
            client = client
                .with_model(config.model.clone())
                .with_variant(config.variant.clone())
                .with_max_tokens(config.max_tokens);
            Ok(Box::new(client))
        }
        "openrouter" => {
            let mut client = OpenRouterClient::new(api_key)?;
            if let Some(ref url) = config.base_url {
                client = client.with_base_url(url.clone());
            }
            if let Some(ref referer) = config.http_referer {
                client = client.with_http_referer(referer, api_key)?;
            }
            client = client
                .with_model(config.model.clone())
                .with_variant(config.variant.clone())
                .with_max_tokens(config.max_tokens);
            Ok(Box::new(client))
        }
        "openai" => {
            let mut client = OpenAiClient::new(api_key)?;
            if let Some(ref url) = config.base_url {
                client = client.with_base_url(url.clone());
            }
            client = client
                .with_model(config.model.clone())
                .with_variant(config.variant.clone())
                .with_max_tokens(config.max_tokens);
            Ok(Box::new(client))
        }
        other => {
            let names = providers::known_provider_names().join(", ");
            Err(RsGuardError::Config(format!(
                "Unknown provider: '{}'. Supported: {}",
                other, names
            )))
        }
    }
}
