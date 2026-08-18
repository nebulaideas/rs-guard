//! Provider factory for creating LLM provider instances by name.
//!
//! The factory routes each provider to the appropriate client implementation:
//!
//! - **7 standard providers** (deepseek, openai, grok, glm, ollama, gemini,
//!   openrouter) are backed by [`KernelBackedClient`], which wraps
//!   `llm_kernel::llm::OpenAIClient`. These providers need only the standard
//!   OpenAI chat completions fields (`model`, `messages`, `temperature`,
//!   `max_tokens`).
//!
//! - **2 custom-field providers** (qwen, kimi) are backed by
//!   [`GenericOpenAiCompatibleClient`], which supports `result_format`
//!   (Qwen/DashScope) and `extra_body` (Kimi thinking variants) — fields that
//!   `llm_kernel::LLMRequest` does not expose.
//!
//! Adding a new standard provider requires only a metadata entry in
//! [`providers`] (and, optionally, tests and documentation). Adding a provider
//! that needs custom request fields requires using `GenericOpenAiCompatibleClient`.

use crate::error::RsGuardError;
use crate::llm::{
    generic_client::GenericOpenAiCompatibleClient, kernel_client::KernelBackedClient, providers,
    Provider, ProviderConfig,
};

/// Returns `true` if the provider requires custom request fields (`result_format`
/// or `extra_body`) that `llm_kernel::LLMRequest` does not expose, and therefore
/// must use [`GenericOpenAiCompatibleClient`] instead of [`KernelBackedClient`].
///
/// Currently: `qwen` (needs `result_format`) and `kimi` (needs `extra_body` for
/// thinking variants).
fn needs_custom_client(meta: &providers::ProviderMeta) -> bool {
    // Providers with a non-None result_format need the generic client.
    if meta.result_format.is_some() {
        return true;
    }
    // Providers with ExtraBody variants need the generic client.
    if meta
        .variants
        .iter()
        .any(|v| matches!(v.effect, providers::VariantEffect::ExtraBody(..)))
    {
        return true;
    }
    false
}

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

    // Route to the appropriate client based on whether the provider needs
    // custom request fields (result_format or extra_body). A config-level
    // result_format override also forces the generic client, since
    // KernelBackedClient cannot send result_format.
    let needs_custom = needs_custom_client(meta) || config.result_format.is_some();

    if needs_custom {
        // Qwen, Kimi — use GenericOpenAiCompatibleClient.
        let mut client = GenericOpenAiCompatibleClient::new(
            meta,
            api_key,
            &header_overrides,
            config.timeout_secs,
        )?;

        if let Some(ref url) = config.base_url {
            client = client.with_base_url(url.clone());
        }
        client = client
            .with_model(config.model.clone())
            .with_variant(config.variant.clone())
            .with_max_tokens(config.max_tokens)
            .with_result_format(config.result_format.clone());

        Ok(Box::new(client))
    } else {
        // deepseek, openai, grok, glm, ollama, gemini, openrouter — use
        // KernelBackedClient (wraps llm_kernel::OpenAIClient).
        let base_url = config.base_url.as_deref().unwrap_or(meta.default_base_url);

        let client = KernelBackedClient::new(
            meta,
            api_key,
            base_url,
            &config.model,
            &header_overrides,
            config.timeout_secs,
        )?
        .with_variant(config.variant.clone())
        .with_max_tokens(config.max_tokens);

        Ok(Box::new(client))
    }
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
            result_format: None,
            timeout_secs: None,
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
    fn test_factory_creates_qwen() {
        let p = create_provider("qwen", "k", &default_config()).unwrap();
        assert_eq!(p.name(), "qwen");
    }

    #[test]
    fn test_factory_creates_kimi() {
        let p = create_provider("kimi", "k", &default_config()).unwrap();
        assert_eq!(p.name(), "kimi");
    }

    #[test]
    fn test_factory_creates_openrouter() {
        let p = create_provider("openrouter", "k", &default_config()).unwrap();
        assert_eq!(p.name(), "openrouter");
    }

    #[test]
    fn test_factory_creates_ollama() {
        let p = create_provider("ollama", "", &default_config()).unwrap();
        assert_eq!(p.name(), "ollama");
    }

    #[test]
    fn test_factory_creates_gemini() {
        let p = create_provider("gemini", "k", &default_config()).unwrap();
        assert_eq!(p.name(), "gemini");
    }

    #[test]
    fn test_factory_creates_openai() {
        let p = create_provider("openai", "k", &default_config()).unwrap();
        assert_eq!(p.name(), "openai");
    }

    #[test]
    fn test_factory_unknown_provider() {
        assert!(create_provider("nope", "k", &default_config()).is_err());
    }

    #[test]
    fn test_needs_custom_client_qwen() {
        let meta = providers::find_provider("qwen").unwrap();
        assert!(needs_custom_client(meta));
    }

    #[test]
    fn test_needs_custom_client_kimi() {
        let meta = providers::find_provider("kimi").unwrap();
        assert!(needs_custom_client(meta));
    }

    #[test]
    fn test_needs_custom_client_deepseek() {
        let meta = providers::find_provider("deepseek").unwrap();
        assert!(!needs_custom_client(meta));
    }

    #[test]
    fn test_needs_custom_client_openrouter() {
        let meta = providers::find_provider("openrouter").unwrap();
        assert!(!needs_custom_client(meta));
    }
}
