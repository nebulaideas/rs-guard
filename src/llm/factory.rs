//! Provider factory for creating LLM provider instances by name.
//!
//! The factory matches on [`providers::ClientStrategy`], then still honours
//! config-level generic overrides (`force_generic_client`, `result_format`,
//! ExtraBody variants):
//!
//! - **[`providers::ClientStrategy::Kernel`]** (openai, grok, glm, ollama,
//!   gemini, openrouter) — [`KernelBackedClient`], wrapping
//!   `llm_kernel::llm::OpenAIClient`. Standard OpenAI chat completions fields
//!   only (`model`, `messages`, `temperature`, `max_tokens`).
//!
//! - **[`providers::ClientStrategy::Generic`]** (deepseek, qwen, kimi) —
//!   [`GenericOpenAiCompatibleClient`]. Qwen needs `result_format` and Kimi
//!   needs `extra_body` (thinking variants) — fields that
//!   `llm_kernel::LLMRequest` does not expose. DeepSeek sets
//!   `force_generic_client` because llm-kernel cannot deserialize V4 thinking
//!   responses with `"tool_calls": null` or multimodal content arrays.
//!
//! Adding a new standard provider requires only a metadata entry in
//! [`providers`] with `strategy: ClientStrategy::Kernel` (and, optionally,
//! tests and documentation). Adding a provider that needs custom request
//! fields requires `strategy: ClientStrategy::Generic`.

use crate::error::RsGuardError;
use crate::llm::{
    generic_client::GenericOpenAiCompatibleClient, kernel_client::KernelBackedClient, providers,
    Provider, ProviderConfig,
};

/// Config / metadata overrides that still force the generic client even when
/// [`providers::ProviderMeta::strategy`] is [`providers::ClientStrategy::Kernel`].
fn forces_generic_client(meta: &providers::ProviderMeta, config: &ProviderConfig) -> bool {
    meta.force_generic_client
        || meta.result_format.is_some()
        || config.result_format.is_some()
        || meta
            .variants
            .iter()
            .any(|v| matches!(v.effect, providers::VariantEffect::ExtraBody(..)))
}

/// Effective client strategy after applying config-level generic overrides.
fn effective_strategy(
    meta: &providers::ProviderMeta,
    config: &ProviderConfig,
) -> providers::ClientStrategy {
    if forces_generic_client(meta, config) {
        providers::ClientStrategy::Generic
    } else {
        meta.strategy
    }
}

/// Creates an LLM provider instance based on the given provider name.
///
/// Delegates to [`create_provider_with_max_tokens`] with `max_tokens_override`
/// set to `None` (uses `config.max_tokens`).
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
    create_provider_with_max_tokens(provider_name, api_key, config, None)
}

/// Creates an LLM provider, optionally overriding `config.max_tokens`.
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
/// * `max_tokens_override` — When `Some`, used instead of `config.max_tokens`
///   so callers can vary the output budget without cloning [`ProviderConfig`].
///   `None` keeps `config.max_tokens`.
///
/// # Errors
///
/// Returns [`RsGuardError::Config`] if the provider name is unknown
/// or if the API key or any header value contains invalid HTTP characters.
pub fn create_provider_with_max_tokens(
    provider_name: &str,
    api_key: &str,
    config: &ProviderConfig,
    max_tokens_override: Option<u32>,
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

    let effective_max_tokens = max_tokens_override.or(config.max_tokens);

    // 2-arm match on the declared strategy; config-level generic overrides
    // (`force_generic_client`, result_format, ExtraBody) still win because
    // KernelBackedClient cannot send those fields.
    match effective_strategy(meta, config) {
        providers::ClientStrategy::Generic => {
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
                .with_max_tokens(effective_max_tokens)
                .with_result_format(config.result_format.clone());

            Ok(Box::new(client))
        }
        providers::ClientStrategy::Kernel => {
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
            .with_max_tokens(effective_max_tokens);

            Ok(Box::new(client))
        }
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
    fn test_effective_strategy_qwen_is_generic() {
        let meta = providers::find_provider("qwen").unwrap();
        assert_eq!(meta.strategy, providers::ClientStrategy::Generic);
        assert_eq!(
            effective_strategy(meta, &default_config()),
            providers::ClientStrategy::Generic
        );
    }

    #[test]
    fn test_effective_strategy_kimi_is_generic() {
        let meta = providers::find_provider("kimi").unwrap();
        assert_eq!(meta.strategy, providers::ClientStrategy::Generic);
        assert_eq!(
            effective_strategy(meta, &default_config()),
            providers::ClientStrategy::Generic
        );
    }

    #[test]
    fn test_effective_strategy_deepseek_is_generic() {
        let meta = providers::find_provider("deepseek").unwrap();
        assert!(
            meta.force_generic_client,
            "DeepSeek must opt into the generic client via ProviderMeta"
        );
        assert_eq!(meta.strategy, providers::ClientStrategy::Generic);
        assert_eq!(
            effective_strategy(meta, &default_config()),
            providers::ClientStrategy::Generic
        );
    }

    #[test]
    fn test_create_provider_deepseek_uses_generic_client() {
        let deepseek = providers::find_provider("deepseek").unwrap();
        let qwen = providers::find_provider("qwen").unwrap();
        let kimi = providers::find_provider("kimi").unwrap();
        let openai = providers::find_provider("openai").unwrap();
        assert!(deepseek.force_generic_client);
        assert!(!openai.force_generic_client);
        assert_eq!(deepseek.strategy, providers::ClientStrategy::Generic);
        assert_eq!(qwen.strategy, providers::ClientStrategy::Generic);
        assert_eq!(kimi.strategy, providers::ClientStrategy::Generic);
        assert_eq!(openai.strategy, providers::ClientStrategy::Kernel);

        let p = create_provider("deepseek", "k", &default_config()).unwrap();
        assert_eq!(p.name(), "deepseek");
    }

    #[test]
    fn test_effective_strategy_openrouter_is_kernel() {
        let meta = providers::find_provider("openrouter").unwrap();
        assert_eq!(meta.strategy, providers::ClientStrategy::Kernel);
        assert_eq!(
            effective_strategy(meta, &default_config()),
            providers::ClientStrategy::Kernel
        );
    }

    #[test]
    fn test_config_result_format_forces_generic_even_for_kernel_provider() {
        let mut config = default_config();
        config.result_format = Some("message".to_string());
        let meta = providers::find_provider("openai").unwrap();
        assert_eq!(meta.strategy, providers::ClientStrategy::Kernel);
        assert_eq!(
            effective_strategy(meta, &config),
            providers::ClientStrategy::Generic
        );
    }

    fn kernel_meta_with(
        force_generic_client: bool,
        result_format: Option<std::borrow::Cow<'static, str>>,
        variants: &'static [providers::ProviderVariant],
    ) -> providers::ProviderMeta {
        providers::ProviderMeta {
            name: "override-test",
            default_base_url: "https://example.com",
            default_model: "m",
            api_key_env: "K",
            api_key_required: true,
            ci_allowed_hosts: &[],
            context_window: 1,
            variants,
            result_format,
            default_extra_headers: &[],
            force_generic_client,
            strategy: providers::ClientStrategy::Kernel,
        }
    }

    #[test]
    fn test_force_generic_client_override_wins_over_kernel_strategy() {
        let meta = kernel_meta_with(true, None, &[]);
        assert_eq!(meta.strategy, providers::ClientStrategy::Kernel);
        assert_eq!(
            effective_strategy(&meta, &default_config()),
            providers::ClientStrategy::Generic
        );
    }

    #[test]
    fn test_metadata_result_format_override_wins_over_kernel_strategy() {
        let meta = kernel_meta_with(false, Some(std::borrow::Cow::Borrowed("message")), &[]);
        assert_eq!(meta.strategy, providers::ClientStrategy::Kernel);
        assert_eq!(
            effective_strategy(&meta, &default_config()),
            providers::ClientStrategy::Generic
        );
    }

    #[test]
    fn test_extra_body_override_wins_over_kernel_strategy() {
        const VARIANTS: &[providers::ProviderVariant] = &[providers::ProviderVariant {
            name: "thinking-on",
            description: "test",
            effect: providers::VariantEffect::ExtraBody("thinking", r#"{"type":"enabled"}"#),
            temperature_override: None,
        }];
        let meta = kernel_meta_with(false, None, VARIANTS);
        assert_eq!(meta.strategy, providers::ClientStrategy::Kernel);
        assert_eq!(
            effective_strategy(&meta, &default_config()),
            providers::ClientStrategy::Generic
        );
    }

    #[test]
    fn test_provider_meta_self_documents_client_strategy() {
        use providers::ClientStrategy::{Generic, Kernel};
        let expected: &[(&str, providers::ClientStrategy)] = &[
            ("deepseek", Generic),
            ("kimi", Generic),
            ("qwen", Generic),
            ("openrouter", Kernel),
            ("openai", Kernel),
            ("grok", Kernel),
            ("glm", Kernel),
            ("ollama", Kernel),
            ("gemini", Kernel),
            ("test-collision", Generic),
        ];
        for (name, strategy) in expected {
            let meta = providers::find_provider(name).unwrap();
            assert_eq!(
                meta.strategy, *strategy,
                "{name} should declare ClientStrategy::{strategy:?}"
            );
        }
    }
}
