//! Provider factory for creating LLM provider instances by name.
//!
//! Every OpenAI-compatible provider is backed by a single
//! [`GenericOpenAiCompatibleClient`], configured entirely from the provider's
//! [`ProviderMeta`] entry plus the resolved [`ProviderConfig`]. Adding a new
//! provider therefore requires only a metadata entry (and, optionally, tests
//! and documentation) — no per-provider client code.

use crate::error::RsGuardError;
use crate::llm::{
    generic_client::GenericOpenAiCompatibleClient, providers, Provider, ProviderConfig,
};

/// Creates an LLM provider instance based on the given provider name.
///
/// The provider is constructed from its [`providers::ProviderMeta`] defaults,
/// then the supplied `config` overrides (base URL, model, variant, max tokens,
/// and — for OpenRouter — a custom HTTP referer) are applied on top.
///
/// # Arguments
///
/// * `provider_name` — Provider identifier (e.g. `"deepseek"`, `"grok"`).
/// * `api_key` — API key for authenticating with the provider.
/// * `config` — Provider configuration overrides from `.reviewer.toml` and CLI.
///
/// # Errors
///
/// Returns [`RsGuardError::Config`] if the provider name is unknown
/// or if the API key or any header value contains invalid HTTP characters.
pub fn create_provider(
    provider_name: &str,
    api_key: &str,
    config: &ProviderConfig,
) -> Result<Provider, RsGuardError> {
    let meta = providers::find_provider(provider_name).ok_or_else(|| {
        let names = providers::known_provider_names().join(", ");
        RsGuardError::Config(format!(
            "Unknown provider: '{}'. Supported: {}",
            provider_name, names
        ))
    })?;

    // OpenRouter allows a custom HTTP-Referer override; other providers ignore it.
    let header_overrides: Vec<(&str, &str)> = match (provider_name, &config.http_referer) {
        ("openrouter", Some(referer)) => vec![("HTTP-Referer", referer.as_str())],
        (_, Some(_)) => {
            eprintln!(
                "⚠️  Warning: http_referer is set but ignored for provider '{}' (only OpenRouter uses it)",
                provider_name
            );
            Vec::new()
        }
        _ => Vec::new(),
    };

    let mut client = GenericOpenAiCompatibleClient::new(meta, api_key, &header_overrides)?;

    if let Some(ref url) = config.base_url {
        client = client.with_base_url(url.clone());
    }
    client = client
        .with_model(config.model.clone())
        .with_variant(config.variant.clone())
        .with_max_tokens(config.max_tokens);

    Ok(Box::new(client))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ProviderConfig {
        ProviderConfig {
            base_url: None,
            http_referer: None,
            max_tokens: None,
            model: "test-model".to_string(),
            variant: None,
        }
    }

    #[test]
    fn test_factory_creates_deepseek() {
        let p = create_provider("deepseek", "k", &default_config()).unwrap();
        assert_eq!(p.name(), "deepseek");
    }

    #[test]
    fn test_factory_creates_grok() {
        let p = create_provider("grok", "k", &default_config()).unwrap();
        assert_eq!(p.name(), "grok");
    }

    #[test]
    fn test_factory_creates_glm() {
        let p = create_provider("glm", "k", &default_config()).unwrap();
        assert_eq!(p.name(), "glm");
    }

    #[test]
    fn test_factory_unknown_provider() {
        assert!(create_provider("nope", "k", &default_config()).is_err());
    }
}
