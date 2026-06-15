//! Centralized provider metadata.
//!
//! Single source of truth for provider names, default base URLs, default
//! models, API key environment variables, and CI-mode allowed hosts.
//! Every other module that needs provider metadata should import from here
//! instead of duplicating constants.

/// Effect that a model variant has on an LLM request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariantEffect {
    /// Variant maps to a concrete model identifier.
    ModelAlias(&'static str),
    /// Variant injects a provider-specific key/value into the request body.
    ExtraBody(&'static str, serde_json::Value),
}

/// Metadata for a single supported model variant.
pub struct ProviderVariant {
    /// Canonical variant identifier (e.g. `"flash"`).
    pub name: &'static str,
    /// Human-readable description.
    pub description: &'static str,
    /// How this variant changes the outgoing request.
    pub effect: VariantEffect,
}

/// Metadata for a single LLM provider.
pub struct ProviderMeta {
    /// Canonical provider identifier (e.g. `"deepseek"`).
    pub name: &'static str,
    /// Default API base URL.
    pub default_base_url: &'static str,
    /// Default model identifier.
    pub default_model: &'static str,
    /// Environment variable name for the API key.
    pub api_key_env: &'static str,
    /// (scheme, host) pairs allowed in CI mode for SSRF prevention.
    pub ci_allowed_hosts: &'static [(&'static str, &'static str)],
    /// Context window size in tokens.
    pub context_window: usize,
    /// Supported model variants for this provider.
    pub variants: &'static [ProviderVariant],
}

/// Returns the metadata for all known providers, in registration order.
pub fn all_providers() -> &'static [ProviderMeta] {
    &[
        ProviderMeta {
            name: "deepseek",
            default_base_url: "https://api.deepseek.com",
            default_model: "deepseek-v4-flash",
            api_key_env: "DEEPSEEK_API_KEY",
            ci_allowed_hosts: &[("https", "api.deepseek.com")],
            context_window: 64_000,
            variants: &[],
        },
        ProviderMeta {
            name: "kimi",
            default_base_url: "https://api.moonshot.ai/v1",
            default_model: "kimi-k2.5",
            api_key_env: "KIMI_API_KEY",
            ci_allowed_hosts: &[("https", "api.moonshot.ai")],
            context_window: 128_000,
            variants: &[],
        },
        ProviderMeta {
            name: "qwen",
            default_base_url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
            default_model: "qwen-plus",
            api_key_env: "DASHSCOPE_API_KEY",
            ci_allowed_hosts: &[
                ("https", "dashscope-intl.aliyuncs.com"),
                ("https", "dashscope.aliyuncs.com"),
            ],
            context_window: 128_000,
            variants: &[],
        },
        ProviderMeta {
            name: "openrouter",
            default_base_url: "https://openrouter.ai/api/v1",
            default_model: "openai/gpt-4o-mini",
            api_key_env: "OPENROUTER_API_KEY",
            ci_allowed_hosts: &[("https", "openrouter.ai")],
            context_window: 128_000,
            variants: &[],
        },
        ProviderMeta {
            name: "openai",
            default_base_url: "https://api.openai.com/v1",
            default_model: "gpt-4o-mini",
            api_key_env: "OPENAI_API_KEY",
            ci_allowed_hosts: &[("https", "api.openai.com")],
            context_window: 128_000,
            variants: &[],
        },
    ]
}

/// Looks up a provider by name and returns its metadata.
///
/// # Errors
///
/// Returns `None` if the provider name is not recognized.
pub fn find_provider(name: &str) -> Option<&'static ProviderMeta> {
    all_providers().iter().find(|p| p.name == name)
}

/// Returns the context window size for a given provider.
///
/// Returns `None` if the provider is not recognized.
pub fn get_provider_context_window(name: &str) -> Option<usize> {
    find_provider(name).map(|p| p.context_window)
}

/// Looks up a provider's variant by name.
///
/// Returns `None` if the provider or variant is not recognized.
pub fn find_provider_variant(
    provider_name: &str,
    variant_name: &str,
) -> Option<&'static ProviderVariant> {
    find_provider(provider_name).and_then(|p| {
        p.variants
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case(variant_name))
    })
}

/// Returns the names of all variants supported by a provider.
///
/// Returns an empty vec if the provider is not recognized.
pub fn provider_variant_names(provider_name: &str) -> Vec<&'static str> {
    find_provider(provider_name)
        .map(|p| p.variants.iter().map(|v| v.name).collect())
        .unwrap_or_default()
}

/// Returns a formatted string of all known provider names.
pub fn known_provider_names() -> Vec<&'static str> {
    all_providers().iter().map(|p| p.name).collect()
}

/// Aggregates all CI-allowed hosts across every provider into a single list.
///
/// Dynamically derived from [`all_providers`] so that adding a new provider
/// automatically includes its hosts in the SSRF allowlist.
///
/// Used by [`crate::http::validate_provider_base_url`] to build the SSRF
/// allowlist.
pub fn all_ci_allowed_hosts() -> Vec<(&'static str, &'static str)> {
    all_providers()
        .iter()
        .flat_map(|p| p.ci_allowed_hosts.iter().copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_all_providers_have_unique_names() {
        let names: Vec<&str> = all_providers().iter().map(|p| p.name).collect();
        let unique: HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "duplicate provider names found");
    }

    #[test]
    fn test_all_providers_have_non_empty_defaults() {
        for p in all_providers() {
            assert!(
                !p.default_base_url.is_empty(),
                "{} missing base_url",
                p.name
            );
            assert!(!p.default_model.is_empty(), "{} missing model", p.name);
            assert!(!p.api_key_env.is_empty(), "{} missing api_key_env", p.name);
            assert!(
                !p.ci_allowed_hosts.is_empty(),
                "{} missing ci_allowed_hosts",
                p.name
            );
        }
    }

    #[test]
    fn test_find_provider_existing() {
        let ds = find_provider("deepseek").unwrap();
        assert_eq!(ds.name, "deepseek");
        assert_eq!(ds.default_model, "deepseek-v4-flash");
    }

    #[test]
    fn test_find_provider_unknown() {
        assert!(find_provider("nonexistent").is_none());
    }

    #[test]
    fn test_known_provider_names_count() {
        assert_eq!(known_provider_names().len(), 5);
    }

    #[test]
    fn test_all_providers_have_context_window() {
        for p in all_providers() {
            assert!(p.context_window > 0, "{} missing context_window", p.name);
        }
    }

    #[test]
    fn test_get_provider_context_window_known() {
        assert_eq!(get_provider_context_window("deepseek"), Some(64_000));
        assert_eq!(get_provider_context_window("kimi"), Some(128_000));
        assert_eq!(get_provider_context_window("openai"), Some(128_000));
    }

    #[test]
    fn test_get_provider_context_window_unknown() {
        assert_eq!(get_provider_context_window("nonexistent"), None);
    }

    #[test]
    fn test_all_ci_allowed_hosts_returns_entries() {
        let hosts = all_ci_allowed_hosts();
        assert!(!hosts.is_empty(), "CI allowed hosts should not be empty");
    }

    #[test]
    fn test_each_provider_default_url_matches_allowed_host() {
        for p in all_providers() {
            let parsed = url::Url::parse(p.default_base_url)
                .unwrap_or_else(|_| panic!("{} default_base_url should be a valid URL", p.name));
            let host = parsed
                .host_str()
                .unwrap_or_else(|| panic!("{} default_base_url should have a host", p.name));
            let scheme = parsed.scheme();
            let allowed = p.ci_allowed_hosts.to_vec();
            assert!(
                allowed.contains(&(scheme, host)),
                "{} default_base_url host ({}) not in its ci_allowed_hosts: {:?}",
                p.name,
                host,
                allowed
            );
        }
    }
}
