//! Provider factory for creating LLM provider instances by name.

use crate::error::DiffguardError;
use crate::llm::{
    deepseek::DeepSeekClient, kimi::KimiClient, openai::OpenAiClient, openrouter::OpenRouterClient,
    qwen::QwenClient, Provider, ProviderConfig,
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
/// Returns [`DiffguardError::Config`] if the provider name is unknown
/// or if the API key contains invalid characters.
pub fn create_provider(
    provider_name: &str,
    api_key: &str,
    config: &ProviderConfig,
) -> Result<Provider, DiffguardError> {
    match provider_name {
        "deepseek" => {
            let mut client = DeepSeekClient::new(api_key)?;
            if let Some(ref url) = config.base_url {
                client = client.with_base_url(url.clone());
            }
            client = client
                .with_model(config.model.clone())
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
                .with_max_tokens(config.max_tokens);
            Ok(Box::new(client))
        }
        other => Err(DiffguardError::Config(format!(
            "Unknown provider: {}. Supported: deepseek, kimi, qwen, openrouter, openai",
            other
        ))),
    }
}
