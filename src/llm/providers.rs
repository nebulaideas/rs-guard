//! Centralized provider metadata.
//!
//! Single source of truth for provider names, default base URLs, default
//! models, API key environment variables, and CI-mode allowed hosts.
//! Every other module that needs provider metadata should import from here
//! instead of duplicating constants.

use crate::error::RsGuardError;
use std::borrow::Cow;
use std::collections::HashMap;

/// Convenient alias for `&'static str` (used in `VariantEffect` arms for
/// consistency and to avoid repeating the verbose type in multiple places).
type StaticStr = &'static str;

/// Effect that a model variant has on an LLM request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariantEffect {
    /// Variant maps to a concrete model identifier.
    ModelAlias(StaticStr),
    /// Variant injects a provider-specific key + JSON value (as a source string) into the request body.
    ///
    /// The key/value is placed at the top level of the serialized request via
    /// `ChatRequest.extra_body` + `#[serde(flatten)]`.
    ///
    /// **Warning:** The key must not collide with standard `ChatRequest` fields
    /// (`model`, `messages`, `temperature`, `max_tokens`). See the documentation
    /// on [`super::ChatRequest`] for details.
    ///
    /// The JSON string is parsed at use time (cheap and the data is hardcoded/trusted).
    /// We keep the source as `&'static str` (instead of a direct `serde_json::Value`)
    /// to satisfy the `'static` lifetime requirements when storing the effects inside
    /// the static table returned by `all_providers()`.
    ExtraBody(StaticStr, StaticStr),
}

/// Metadata for a single supported model variant.
#[derive(Debug)]
pub struct ProviderVariant {
    /// Canonical variant identifier (e.g. `"flash"`).
    pub name: &'static str,
    /// Human-readable description.
    pub description: &'static str,
    /// How this variant changes the outgoing request.
    pub effect: VariantEffect,
}

/// Metadata for a single LLM provider.
#[derive(Debug)]
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
    /// Optional `result_format` field injected into the chat request body.
    ///
    /// Set to `Some(Cow::Borrowed("message"))` for providers whose
    /// OpenAI-compatible API requires an explicit result format (currently
    /// Qwen/DashScope). `None` for all other providers (standard OpenAI shape).
    ///
    /// `Cow<'static, str>` allows the static metadata table to remain
    /// zero-cost while still supporting dynamic per-provider overrides from
    /// `.reviewer.toml`.
    pub result_format: Option<Cow<'static, str>>,
    /// Default extra HTTP headers attached to every request for this provider
    /// (e.g. OpenRouter attribution headers `HTTP-Referer` + `X-Title`).
    ///
    /// Empty for providers that need no extra headers. The factory merges any
    /// config-supplied overrides (such as a custom OpenRouter referer) on top
    /// of these defaults at client construction time.
    pub default_extra_headers: &'static [(&'static str, &'static str)],
}

/// Returns the metadata for all known providers, in registration order.
///
/// This is the single source of truth used by the CLI, configuration,
/// and the variant resolution logic. Custom providers can be added by
/// extending this list (see the custom provider guide).
pub fn all_providers() -> &'static [ProviderMeta] {
    &[
        ProviderMeta {
            name: "deepseek",
            default_base_url: "https://api.deepseek.com",
            default_model: "deepseek-v4-flash",
            api_key_env: "DEEPSEEK_API_KEY",
            ci_allowed_hosts: &[("https", "api.deepseek.com")],
            context_window: 64_000,
            variants: &[
                ProviderVariant {
                    name: "flash",
                    description: "Fast, cost-effective DeepSeek V4 model",
                    effect: VariantEffect::ModelAlias("deepseek-v4-flash"),
                },
                ProviderVariant {
                    name: "pro",
                    description: "Most capable DeepSeek V4 model for complex reasoning",
                    effect: VariantEffect::ModelAlias("deepseek-v4-pro"),
                },
            ],
            result_format: None,
            default_extra_headers: &[],
        },
        ProviderMeta {
            name: "kimi",
            default_base_url: "https://api.moonshot.ai/v1",
            default_model: "kimi-k2.5",
            api_key_env: "KIMI_API_KEY",
            ci_allowed_hosts: &[("https", "api.moonshot.ai")],
            context_window: 128_000,
            variants: &[
                ProviderVariant {
                    name: "thinking-on",
                    description: "Enable Kimi thinking / chain-of-thought mode (response may include reasoning_content)",
                    // We use a raw string literal + runtime parse here (instead of
                    // `serde_json::json!(...)`) purely for 'static lifetime reasons inside
                    // the static provider metadata table. The json! form would be nicer
                    // (compile-time validation) but leads to borrow-checker errors when
                    // storing the resulting `&'static Value` in the array.
                    effect: VariantEffect::ExtraBody("thinking", r#"{"type":"enabled"}"#),
                },
                ProviderVariant {
                    name: "thinking-off",
                    description: "Disable Kimi thinking mode (default)",
                    effect: VariantEffect::ExtraBody("thinking", r#"{"type":"disabled"}"#),
                },
            ],
            result_format: None,
            default_extra_headers: &[],
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
            result_format: Some(Cow::Borrowed("message")),
            default_extra_headers: &[],
        },
        ProviderMeta {
            name: "openrouter",
            default_base_url: "https://openrouter.ai/api/v1",
            default_model: "openai/gpt-4o-mini",
            api_key_env: "OPENROUTER_API_KEY",
            ci_allowed_hosts: &[("https", "openrouter.ai")],
            context_window: 128_000,
            variants: &[],
            result_format: None,
            // OpenRouter requests attribution via HTTP-Referer + X-Title headers.
            default_extra_headers: &[
                ("HTTP-Referer", "https://github.com/nebulaideas/rs-guard"),
                ("X-Title", "rs-guard"),
            ],
        },
        ProviderMeta {
            name: "openai",
            default_base_url: "https://api.openai.com/v1",
            default_model: "gpt-4o-mini",
            api_key_env: "OPENAI_API_KEY",
            ci_allowed_hosts: &[("https", "api.openai.com")],
            context_window: 128_000,
            variants: &[],
            result_format: None,
            default_extra_headers: &[],
        },
        ProviderMeta {
            name: "grok",
            default_base_url: "https://api.x.ai/v1",
            default_model: "grok-3",
            api_key_env: "XAI_API_KEY",
            ci_allowed_hosts: &[("https", "api.x.ai")],
            context_window: 128_000,
            variants: &[],
            result_format: None,
            default_extra_headers: &[],
        },
        ProviderMeta {
            name: "glm",
            default_base_url: "https://open.bigmodel.cn/api/paas/v4",
            default_model: "glm-4",
            api_key_env: "ZHIPUAI_API_KEY",
            ci_allowed_hosts: &[("https", "open.bigmodel.cn")],
            context_window: 128_000,
            variants: &[],
            result_format: None,
            default_extra_headers: &[],
        },
        #[cfg(test)]
        ProviderMeta {
            name: "test-collision",
            default_base_url: "https://test.example.com",
            default_model: "test-model",
            api_key_env: "TEST_API_KEY",
            ci_allowed_hosts: &[("https", "test.example.com")],
            context_window: 128_000,
            variants: &[ProviderVariant {
                name: "bad-variant",
                description: "Variant with reserved key (for testing collision guard)",
                effect: VariantEffect::ExtraBody("model", r#""bad-model""#),
            }],
            result_format: None,
            default_extra_headers: &[],
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

/// Resolves a (possibly variant-adjusted) model identifier together with any
/// extra top-level body fields contributed by the variant.
///
/// This is the single shared implementation used by all LLM provider clients
/// so that `ModelAlias` and `ExtraBody` effects work uniformly.
///
/// See the detailed resolution rules in the implementation below.
pub(crate) fn apply_variant(
    provider_name: &str,
    configured_model: &str,
    variant: Option<&str>,
) -> Result<(String, HashMap<String, serde_json::Value>), RsGuardError> {
    // Resolution rules (preserve documented "no effect" / silent-ignore behaviour):
    // * No variant supplied → (configured_model, empty map)
    // * Variant matches a ModelAlias → (aliased model id, empty map)
    // * Variant matches an ExtraBody(k, v) → (configured_model, {k: v})
    // * Variant unknown **and** provider declares ≥1 variants → RsGuardError::Config listing supported names
    // * Variant unknown **and** provider declares 0 variants → (configured_model, empty map)  // silently ignored
    let Some(vname) = variant else {
        return Ok((configured_model.to_string(), HashMap::new()));
    };

    match find_provider_variant(provider_name, vname) {
        Some(v) => match &v.effect {
            VariantEffect::ModelAlias(alias) => Ok((alias.to_string(), HashMap::new())),
            VariantEffect::ExtraBody(key, json) => {
                // F7: Reject ExtraBody keys that collide with standard ChatRequest fields.
                // These would silently overwrite the corresponding field during serialization.
                const RESERVED_KEYS: &[&str] = &[
                    "model",
                    "messages",
                    "temperature",
                    "max_tokens",
                    "result_format",
                ];
                if RESERVED_KEYS.contains(key) {
                    return Err(RsGuardError::Config(format!(
                        "Variant '{}' for provider '{}' attempts to set ExtraBody key '{}' which collides with a standard ChatRequest field. This would silently overwrite the field. Use a different key name.",
                        vname, provider_name, key
                    )));
                }

                // TODO (R6, 1.3.0): Optimize by parsing JSON once at startup and caching
                // the serde_json::Value in ProviderVariant. Currently parses on every
                // variant use, but the strings are small and hardcoded, so the overhead
                // is minimal (microseconds). Would require changing ProviderVariant to
                // use serde_json::Value instead of &'static str, which has lifetime
                // implications for the static all_providers() table.
                let val: serde_json::Value = serde_json::from_str(json).map_err(|e| {
                    RsGuardError::Config(format!(
                        "Invalid hardcoded variant JSON for key '{}': {}",
                        key, e
                    ))
                })?;
                let mut map = HashMap::new();
                map.insert((*key).to_string(), val);
                Ok((configured_model.to_string(), map))
            }
        },
        None => {
            let declared = provider_variant_names(provider_name);
            if declared.is_empty() {
                // No variants registered → "has no effect" per CLI help and PROVIDERS.md
                Ok((configured_model.to_string(), HashMap::new()))
            } else {
                Err(RsGuardError::Config(format!(
                    "Unknown variant '{}' for provider '{}'. Supported variants: {}",
                    vname,
                    provider_name,
                    declared.join(", ")
                )))
            }
        }
    }
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
        // 5 original OpenAI-compatible providers + grok (xAI) + glm (Zhipu) + test-collision (test-only).
        assert_eq!(known_provider_names().len(), 8);
    }

    #[test]
    fn test_known_provider_names_includes_grok_and_glm() {
        let names = known_provider_names();
        assert!(names.contains(&"grok"), "grok must be a known provider");
        assert!(names.contains(&"glm"), "glm must be a known provider");
    }

    #[test]
    fn test_grok_metadata() {
        let m = find_provider("grok").expect("grok provider must be registered");
        assert_eq!(m.default_base_url, "https://api.x.ai/v1");
        assert_eq!(m.default_model, "grok-3");
        assert_eq!(m.api_key_env, "XAI_API_KEY");
        assert!(m.ci_allowed_hosts.contains(&("https", "api.x.ai")));
        assert!(m.result_format.is_none());
        assert!(m.default_extra_headers.is_empty());
    }

    #[test]
    fn test_glm_metadata() {
        let m = find_provider("glm").expect("glm provider must be registered");
        assert_eq!(m.default_base_url, "https://open.bigmodel.cn/api/paas/v4");
        assert_eq!(m.default_model, "glm-4");
        assert_eq!(m.api_key_env, "ZHIPUAI_API_KEY");
        assert!(m.ci_allowed_hosts.contains(&("https", "open.bigmodel.cn")));
        assert!(m.result_format.is_none());
        assert!(m.default_extra_headers.is_empty());
    }

    #[test]
    fn test_qwen_result_format_is_message() {
        let m = find_provider("qwen").unwrap();
        assert_eq!(m.result_format, Some(Cow::Borrowed("message")));
    }

    #[test]
    fn test_openrouter_default_extra_headers_present() {
        let m = find_provider("openrouter").unwrap();
        let header_names: Vec<&str> = m.default_extra_headers.iter().map(|(n, _)| *n).collect();
        assert!(header_names.contains(&"HTTP-Referer"));
        assert!(header_names.contains(&"X-Title"));
    }

    #[test]
    fn test_standard_providers_have_no_result_format() {
        for name in ["deepseek", "kimi", "openrouter", "openai", "grok", "glm"] {
            let m = find_provider(name).unwrap();
            assert!(
                m.result_format.is_none(),
                "{} should not declare result_format",
                name
            );
        }
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

    // --- apply_variant tests (core of model-variant-feature) ---

    #[test]
    fn test_apply_variant_none_returns_configured() {
        let (m, extra) = apply_variant("deepseek", "deepseek-v4-flash", None).unwrap();
        assert_eq!(m, "deepseek-v4-flash");
        assert!(extra.is_empty());
    }

    #[test]
    fn test_apply_variant_model_alias_deepseek_flash() {
        let (m, extra) = apply_variant("deepseek", "ignored-base", Some("flash")).unwrap();
        assert_eq!(m, "deepseek-v4-flash");
        assert!(extra.is_empty());
    }

    #[test]
    fn test_apply_variant_model_alias_deepseek_pro() {
        let (m, extra) = apply_variant("deepseek", "ignored-base", Some("pro")).unwrap();
        assert_eq!(m, "deepseek-v4-pro");
        assert!(extra.is_empty());
    }

    #[test]
    fn test_apply_variant_case_insensitive() {
        let (m, _) = apply_variant("deepseek", "base", Some("FLASH")).unwrap();
        assert_eq!(m, "deepseek-v4-flash");
    }

    #[test]
    fn test_apply_variant_unknown_for_provider_with_variants_errors() {
        let err = apply_variant("deepseek", "base", Some("nope")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Unknown variant 'nope'"));
        assert!(msg.contains("deepseek"));
        assert!(msg.contains("flash, pro"));
    }

    #[test]
    fn test_apply_variant_unknown_for_provider_without_variants_is_ignored() {
        // Uses a dedicated dummy provider name that is never expected to declare
        // any variants. This avoids fragility if a real provider (e.g. "openai")
        // later gains variants in all_providers().
        let (m, extra) = apply_variant("test-no-variants", "some-model", Some("anything")).unwrap();
        assert_eq!(m, "some-model");
        assert!(extra.is_empty());
    }

    #[test]
    fn test_apply_variant_unknown_kimi_reports_supported_variants() {
        // Provider that does declare variants: unknown name produces a clear error listing them.
        let err = apply_variant("kimi", "kimi-k2.5", Some("nonexistent-variant")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Unknown variant 'nonexistent-variant'"));
        assert!(msg.contains("kimi"));
        assert!(msg.contains("thinking-on, thinking-off"));
    }

    #[test]
    fn test_apply_variant_extra_body_populates_map() {
        // Now that Kimi registers real ExtraBody variants, exercise the arm
        // directly via apply_variant.
        let (m, extra) = apply_variant("kimi", "kimi-k2.5", Some("thinking-on")).unwrap();
        assert_eq!(m, "kimi-k2.5");
        assert_eq!(
            extra.get("thinking"),
            Some(&serde_json::json!({"type": "enabled"}))
        );

        let (m2, extra2) = apply_variant("kimi", "kimi-k2.5", Some("thinking-off")).unwrap();
        assert_eq!(m2, "kimi-k2.5");
        assert_eq!(
            extra2.get("thinking"),
            Some(&serde_json::json!({"type": "disabled"}))
        );
    }

    #[test]
    fn test_apply_variant_rejects_reserved_extra_body_keys() {
        // F7: ExtraBody keys that collide with standard ChatRequest fields must be rejected.
        // The test-collision provider has a variant with key "model", which is reserved.
        let err = apply_variant("test-collision", "test-model", Some("bad-variant")).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("collides with a standard ChatRequest field"),
            "expected collision error, got: {}",
            msg
        );
        assert!(
            msg.contains("model"),
            "error should mention the reserved key"
        );
        assert!(
            msg.contains("bad-variant"),
            "error should mention the variant name"
        );
    }
}
