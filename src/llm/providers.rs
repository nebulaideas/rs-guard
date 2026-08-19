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
    /// Optional temperature override for this variant.
    ///
    /// Some providers restrict which temperature values are valid for specific
    /// models or modes (e.g. Kimi k2.5 requires `1.0` for thinking-on and
    /// `0.6` for thinking-off). When `Some`, this value replaces the
    /// caller-supplied temperature in the outgoing request. When `None`, the
    /// caller's temperature is used as-is.
    pub temperature_override: Option<f32>,
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
    /// Whether an API key is required. `false` for local providers like Ollama
    /// that don't require authentication. When `false`, a missing env var is
    /// treated as an empty string rather than an error.
    pub api_key_required: bool,
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
            api_key_required: true,

            ci_allowed_hosts: &[("https", "api.deepseek.com")],
            context_window: 64_000,
            variants: &[
                ProviderVariant {
                    name: "flash",
                    description: "Fast, cost-effective DeepSeek V4 model",
                    effect: VariantEffect::ModelAlias("deepseek-v4-flash"),
                    temperature_override: None,
                },
                ProviderVariant {
                    name: "pro",
                    description: "Most capable DeepSeek V4 model for complex reasoning",
                    effect: VariantEffect::ModelAlias("deepseek-v4-pro"),
                    temperature_override: None,
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
            api_key_required: true,

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
                    // Kimi k2.5 only accepts temperature=1.0 when thinking mode is enabled.
                    temperature_override: Some(1.0),
                },
                ProviderVariant {
                    name: "thinking-off",
                    description: "Disable Kimi thinking mode (default)",
                    effect: VariantEffect::ExtraBody("thinking", r#"{"type":"disabled"}"#),
                    // Kimi k2.5 only accepts temperature=0.6 when thinking mode is disabled.
                    temperature_override: Some(0.6),
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
            api_key_required: true,

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
            api_key_required: true,

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
            api_key_required: true,

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
            api_key_required: true,

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
            api_key_required: true,

            ci_allowed_hosts: &[("https", "open.bigmodel.cn")],
            context_window: 128_000,
            variants: &[],
            result_format: None,
            default_extra_headers: &[],
        },
        ProviderMeta {
            name: "ollama",
            default_base_url: "http://127.0.0.1:11434/v1",
            default_model: "llama3.2",
            // Ollama does not require an API key by default; use a placeholder
            // env var that users can set if they enable Ollama's auth proxy.
            api_key_env: "OLLAMA_API_KEY",
            api_key_required: false,
            // Loopback only — rejected in CI mode by validate_provider_base_url.
            // Users must run in local mode (unset GITHUB_ACTIONS) to use Ollama.
            ci_allowed_hosts: &[],
            context_window: 32_000,
            variants: &[],
            result_format: None,
            default_extra_headers: &[],
        },
        ProviderMeta {
            name: "gemini",
            default_base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
            default_model: "gemini-2.5-flash",
            api_key_env: "GEMINI_API_KEY",
            api_key_required: true,

            ci_allowed_hosts: &[("https", "generativelanguage.googleapis.com")],
            context_window: 1_000_000,
            variants: &[
                ProviderVariant {
                    name: "flash",
                    description: "Fast, cost-effective Gemini 2.5 Flash",
                    effect: VariantEffect::ModelAlias("gemini-2.5-flash"),
                    temperature_override: None,
                },
                ProviderVariant {
                    name: "pro",
                    description: "Most capable Gemini 2.5 Pro for complex reasoning",
                    effect: VariantEffect::ModelAlias("gemini-2.5-pro"),
                    temperature_override: None,
                },
            ],
            result_format: None,
            default_extra_headers: &[],
        },
        #[cfg(test)]
        ProviderMeta {
            name: "test-collision",
            default_base_url: "https://test.example.com",
            default_model: "test-model",
            api_key_env: "TEST_API_KEY",
            api_key_required: true,

            ci_allowed_hosts: &[("https", "test.example.com")],
            context_window: 128_000,
            variants: &[ProviderVariant {
                name: "bad-variant",
                description: "Variant with reserved key (for testing collision guard)",
                effect: VariantEffect::ExtraBody("model", r#""bad-model""#),
                temperature_override: None,
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

/// Resolves the effective temperature for a provider + variant combination.
///
/// When the variant has a `temperature_override`, that value replaces the
/// configured temperature. Otherwise the configured temperature is returned
/// as-is. This is used by the cache layer to ensure cache keys reflect the
/// actual temperature sent to the LLM, not the pre-override configured value.
///
/// Returns the configured temperature when the variant is unknown or the
/// provider has no variants.
pub(crate) fn effective_temperature(
    provider_name: &str,
    variant: Option<&str>,
    configured: f32,
) -> f32 {
    let Some(vname) = variant else {
        return configured;
    };
    find_provider_variant(provider_name, vname)
        .and_then(|v| v.temperature_override)
        .unwrap_or(configured)
}

/// The result of variant resolution: the effective model identifier, any
/// extra top-level body fields, and an optional temperature override.
pub(crate) type VariantResolution = (String, HashMap<String, serde_json::Value>, Option<f32>);

/// Resolves a (possibly variant-adjusted) model identifier, extra top-level
/// body fields, and an optional temperature override contributed by the variant.
///
/// This is the single shared implementation used by all LLM provider clients
/// so that `ModelAlias`, `ExtraBody`, and `temperature_override` effects work
/// uniformly.
///
/// Returns a tuple of `(effective_model, extra_body, temperature_override)`.
/// When `temperature_override` is `Some`, the caller should replace the
/// configured temperature with this value in the outgoing request.
///
/// See the detailed resolution rules in the implementation below.
pub(crate) fn apply_variant(
    provider_name: &str,
    configured_model: &str,
    variant: Option<&str>,
) -> Result<VariantResolution, RsGuardError> {
    // Resolution rules (preserve documented "no effect" / silent-ignore behaviour):
    // * No variant supplied → (configured_model, empty map, None)
    // * Variant matches a ModelAlias → (aliased model id, empty map, variant.temperature_override)
    // * Variant matches an ExtraBody(k, v) → (configured_model, {k: v}, variant.temperature_override)
    // * Variant unknown **and** provider declares ≥1 variants → RsGuardError::Config listing supported names
    // * Variant unknown **and** provider declares 0 variants → (configured_model, empty map, None)  // silently ignored
    let Some(vname) = variant else {
        return Ok((configured_model.to_string(), HashMap::new(), None));
    };

    match find_provider_variant(provider_name, vname) {
        Some(v) => match &v.effect {
            VariantEffect::ModelAlias(alias) => {
                Ok((alias.to_string(), HashMap::new(), v.temperature_override))
            }
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

                // NOTE: This parses JSON on every variant use. Could be optimized by
                // caching the serde_json::Value in ProviderVariant, but the strings are
                // small and hardcoded so overhead is minimal (microseconds). Caching
                // would require changing ProviderVariant from &'static str to
                // serde_json::Value, which has lifetime implications for the static
                // all_providers() table — not worth the complexity.
                let val: serde_json::Value = serde_json::from_str(json).map_err(|e| {
                    RsGuardError::Config(format!(
                        "Invalid hardcoded variant JSON for key '{}': {}",
                        key, e
                    ))
                })?;
                let mut map = HashMap::new();
                map.insert((*key).to_string(), val);
                Ok((configured_model.to_string(), map, v.temperature_override))
            }
        },
        None => {
            let declared = provider_variant_names(provider_name);
            if declared.is_empty() {
                // No variants registered → "has no effect" per CLI help and PROVIDERS.md
                Ok((configured_model.to_string(), HashMap::new(), None))
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

// ---------------------------------------------------------------------------
// llm-kernel catalog cross-reference (issue #142, Phase 2)
// ---------------------------------------------------------------------------
//
// [`all_providers`] above remains the authoritative source of the connection
// metadata rs-guard actually uses (OpenAI-compatible base URLs, default models,
// env vars, SSRF allowlists, variants). llm-kernel's embedded catalog is
// layered on as a supplementary metadata source: it offers pricing, model
// lists, capabilities and 20 providers, but it is Anthropic-oriented and does
// not cover every rs-guard endpoint (notably openrouter and grok/xai are
// absent, and base URLs for deepseek/kimi/qwen/glm point at Anthropic-coding
// endpoints rather than OpenAI-compatible ones). The functions below map
// rs-guard's provider ids to llm-kernel's catalog ids and expose the matching
// [`ServiceDescriptor`] for enrichment (e.g. pricing, model lists).

use llm_kernel::provider::{ProviderIndex, ServiceDescriptor};

/// Returns the embedded llm-kernel provider catalog (20 providers, models.dev
/// schema). Shared by every rs-guard module that wants llm-kernel metadata.
pub fn kernel_catalog() -> &'static ProviderIndex {
    ProviderIndex::embedded()
}

/// Maps an rs-guard provider name to its llm-kernel catalog id, or `None` when
/// llm-kernel has no equivalent entry (currently `openrouter` and `grok`).
pub fn kernel_provider_id(provider_name: &str) -> Option<&'static str> {
    match provider_name {
        "deepseek" => Some("deepseek"),
        "kimi" => Some("kimi"),
        "qwen" => Some("alibaba"),
        "openai" => Some("openai"),
        "glm" => Some("zai-cn"),
        "ollama" => Some("ollama"),
        "gemini" => Some("gemini"),
        _ => None,
    }
}

/// Returns the llm-kernel [`ServiceDescriptor`] for an rs-guard provider, if a
/// mapping exists. The returned descriptor's base URL / default model reflect
/// llm-kernel's (Anthropic-oriented) catalog and must **not** be used to
/// override rs-guard's [`ProviderMeta`]; it is a source for capabilities,
/// pricing and model lists only.
pub fn kernel_provider(provider_name: &str) -> Option<&'static ServiceDescriptor> {
    kernel_provider_id(provider_name).and_then(|id| kernel_catalog().get(id))
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
            // When ci_allowed_hosts is empty, the provider's default base URL
            // must be loopback (local-only, rejected in CI mode). This is the
            // actual security invariant — not api_key_required.
            if p.ci_allowed_hosts.is_empty() {
                let parsed = url::Url::parse(p.default_base_url)
                    .unwrap_or_else(|_| panic!("{} default_base_url should be valid", p.name));
                let host = parsed.host_str().unwrap_or("");
                assert!(
                    host == "127.0.0.1"
                        || host == "localhost"
                        || host == "[::1]"
                        || host == "0.0.0.0"
                        || host == "[::]",
                    "{} has empty ci_allowed_hosts but default_base_url host '{}' is not loopback — \
                     this would be an SSRF risk if accidentally allowed in CI",
                    p.name,
                    host
                );
            } else {
                // Providers with CI allowed hosts must have at least one entry.
                // (Already verified by the if-condition; no further assertion needed.)
            }
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
        // 5 original + grok + glm + ollama + gemini + test-collision (test-only).
        assert_eq!(known_provider_names().len(), 10);
    }

    #[test]
    fn test_known_provider_names_includes_grok_and_glm() {
        let names = known_provider_names();
        assert!(names.contains(&"grok"), "grok must be a known provider");
        assert!(names.contains(&"glm"), "glm must be a known provider");
    }

    #[test]
    fn test_known_provider_names_includes_ollama_and_gemini() {
        let names = known_provider_names();
        assert!(names.contains(&"ollama"), "ollama must be a known provider");
        assert!(names.contains(&"gemini"), "gemini must be a known provider");
    }

    #[test]
    fn test_ollama_api_key_not_required() {
        let meta = find_provider("ollama").expect("ollama must be registered");
        assert!(
            !meta.api_key_required,
            "ollama should not require an API key"
        );
    }

    #[test]
    fn test_gemini_api_key_required() {
        let meta = find_provider("gemini").expect("gemini must be registered");
        assert!(meta.api_key_required, "gemini should require an API key");
    }

    #[test]
    fn test_gemini_context_window() {
        assert_eq!(get_provider_context_window("gemini"), Some(1_000_000));
    }

    #[test]
    fn test_ollama_context_window() {
        assert_eq!(get_provider_context_window("ollama"), Some(32_000));
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
            // Skip providers with empty ci_allowed_hosts (local-only, e.g. Ollama).
            // Their loopback URL is intentionally not in the allowlist and is
            // rejected in CI mode by validate_provider_base_url.
            if p.ci_allowed_hosts.is_empty() {
                continue;
            }
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
        let (m, extra, _) = apply_variant("deepseek", "deepseek-v4-flash", None).unwrap();
        assert_eq!(m, "deepseek-v4-flash");
        assert!(extra.is_empty());
    }

    #[test]
    fn test_apply_variant_model_alias_deepseek_flash() {
        let (m, extra, _) = apply_variant("deepseek", "ignored-base", Some("flash")).unwrap();
        assert_eq!(m, "deepseek-v4-flash");
        assert!(extra.is_empty());
    }

    #[test]
    fn test_apply_variant_model_alias_deepseek_pro() {
        let (m, extra, _) = apply_variant("deepseek", "ignored-base", Some("pro")).unwrap();
        assert_eq!(m, "deepseek-v4-pro");
        assert!(extra.is_empty());
    }

    #[test]
    fn test_apply_variant_case_insensitive() {
        let (m, _, _) = apply_variant("deepseek", "base", Some("FLASH")).unwrap();
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
        let (m, extra, _) =
            apply_variant("test-no-variants", "some-model", Some("anything")).unwrap();
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
        let (m, extra, _) = apply_variant("kimi", "kimi-k2.5", Some("thinking-on")).unwrap();
        assert_eq!(m, "kimi-k2.5");
        assert_eq!(
            extra.get("thinking"),
            Some(&serde_json::json!({"type": "enabled"}))
        );

        let (m2, extra2, _) = apply_variant("kimi", "kimi-k2.5", Some("thinking-off")).unwrap();
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

    // --- temperature_override tests (per-variant temperature constraints) ---

    #[test]
    fn test_apply_variant_no_variant_returns_none_temperature() {
        let (_m, _extra, temp_override) =
            apply_variant("deepseek", "deepseek-v4-flash", None).unwrap();
        assert_eq!(
            temp_override, None,
            "no variant should not override temperature"
        );
    }

    #[test]
    fn test_apply_variant_model_alias_returns_none_temperature() {
        let (_m, _extra, temp_override) = apply_variant("deepseek", "base", Some("flash")).unwrap();
        assert_eq!(
            temp_override, None,
            "ModelAlias variants without temperature_override should return None"
        );
    }

    #[test]
    fn test_apply_variant_kimi_thinking_on_returns_temperature_override() {
        let (_m, _extra, temp_override) =
            apply_variant("kimi", "kimi-k2.5", Some("thinking-on")).unwrap();
        assert_eq!(
            temp_override,
            Some(1.0),
            "kimi thinking-on should override temperature to 1.0"
        );
    }

    #[test]
    fn test_apply_variant_kimi_thinking_off_returns_temperature_override() {
        let (_m, _extra, temp_override) =
            apply_variant("kimi", "kimi-k2.5", Some("thinking-off")).unwrap();
        assert_eq!(
            temp_override,
            Some(0.6),
            "kimi thinking-off should override temperature to 0.6"
        );
    }

    // --- effective_temperature tests (cache key correctness) ---

    #[test]
    fn test_effective_temperature_no_variant_returns_configured() {
        assert_eq!(
            effective_temperature("deepseek", None, 0.1),
            0.1,
            "no variant should return configured temperature"
        );
    }

    #[test]
    fn test_effective_temperature_variant_without_override_returns_configured() {
        assert_eq!(
            effective_temperature("deepseek", Some("flash"), 0.1),
            0.1,
            "variant without temperature_override should return configured temperature"
        );
    }

    #[test]
    fn test_effective_temperature_kimi_thinking_on_returns_override() {
        assert_eq!(
            effective_temperature("kimi", Some("thinking-on"), 0.1),
            1.0,
            "kimi thinking-on should return 1.0 regardless of configured temperature"
        );
    }

    #[test]
    fn test_effective_temperature_kimi_thinking_off_returns_override() {
        assert_eq!(
            effective_temperature("kimi", Some("thinking-off"), 0.1),
            0.6,
            "kimi thinking-off should return 0.6 regardless of configured temperature"
        );
    }

    #[test]
    fn test_effective_temperature_unknown_variant_returns_configured() {
        assert_eq!(
            effective_temperature("deepseek", Some("nonexistent"), 0.1),
            0.1,
            "unknown variant should return configured temperature"
        );
    }

    // --- llm-kernel catalog cross-reference tests (issue #142, Phase 2) ---

    #[test]
    fn test_kernel_catalog_provides_embedded_index() {
        // ProviderIndex::embedded() must load llm-kernel's catalog and expose ids.
        let catalog = kernel_catalog();
        let ids = catalog.ids();
        assert!(!ids.is_empty(), "llm-kernel catalog must not be empty");
        assert!(
            ids.contains(&"openai".to_string()),
            "catalog should contain openai"
        );
        assert!(
            ids.contains(&"ollama".to_string()),
            "catalog should contain ollama"
        );
    }

    #[test]
    fn test_kernel_provider_mapping_resolves_in_catalog() {
        // Every mapped rs-guard provider must resolve to a ServiceDescriptor.
        for name in [
            "deepseek", "kimi", "qwen", "openai", "glm", "ollama", "gemini",
        ] {
            let id = kernel_provider_id(name)
                .unwrap_or_else(|| panic!("no llm-kernel id mapped for '{}'", name));
            assert!(
                kernel_catalog().get(id).is_some(),
                "llm-kernel catalog should contain id '{}' for provider '{}'",
                id,
                name
            );
            assert!(
                kernel_provider(name).is_some(),
                "kernel_provider('{}') should resolve",
                name
            );
        }
    }

    #[test]
    fn test_kernel_mapping_documents_unsupported_providers() {
        // These rs-guard providers have no llm-kernel catalog entry (documented gap).
        assert_eq!(kernel_provider_id("openrouter"), None);
        assert_eq!(kernel_provider_id("grok"), None);
        assert_eq!(kernel_provider("openrouter"), None);
        assert_eq!(kernel_provider("grok"), None);
    }

    #[test]
    fn test_every_provider_has_kernel_mapping_or_is_documented() {
        // Every non-test rs-guard provider either maps into llm-kernel or is a
        // known gap (openrouter/grok). Guards against adding a provider without
        // considering the llm-kernel cross-reference.
        let documented_gaps = ["openrouter", "grok"];
        for p in all_providers() {
            if p.name == "test-collision" {
                continue;
            }
            assert!(
                kernel_provider_id(p.name).is_some() || documented_gaps.contains(&p.name),
                "provider '{}' has neither a llm-kernel mapping nor is a documented gap",
                p.name
            );
        }
    }
}
