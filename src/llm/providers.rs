//! Centralized provider metadata.
//!
//! Single source of truth for provider names, default base URLs, default
//! models, API key environment variables, and CI-mode allowed hosts.
//! Every other module that needs provider metadata should import from here
//! instead of duplicating constants.

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
        },
        ProviderMeta {
            name: "kimi",
            default_base_url: "https://api.moonshot.ai/v1",
            default_model: "kimi-k2.5",
            api_key_env: "KIMI_API_KEY",
            ci_allowed_hosts: &[("https", "api.moonshot.ai")],
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
        },
        ProviderMeta {
            name: "openrouter",
            default_base_url: "https://openrouter.ai/api/v1",
            default_model: "openai/gpt-4o-mini",
            api_key_env: "OPENROUTER_API_KEY",
            ci_allowed_hosts: &[("https", "openrouter.ai")],
        },
        ProviderMeta {
            name: "openai",
            default_base_url: "https://api.openai.com/v1",
            default_model: "gpt-4o-mini",
            api_key_env: "OPENAI_API_KEY",
            ci_allowed_hosts: &[("https", "api.openai.com")],
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
}
