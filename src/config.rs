//! Configuration resolution from environment variables, TOML file, and CLI arguments.
//!
//! Handles detection of CI vs local mode, API key resolution, and
//! validation of required fields for GitHub PR review submission.
//!
//! Configuration resolution order: CLI flags > Environment variables > TOML file > Defaults

use crate::cli::Args;
use crate::error::RsGuardError;
use crate::http::{validate_github_base_url, validate_provider_base_url};
use crate::llm::providers::{self, find_provider};
use crate::llm::ProviderConfig;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Default maximum tokens for LLM responses.
///
/// Ensures the verdict metadata block is never truncated. 4096 tokens is
/// sufficient for a thorough structured review while fitting within the
/// output limits of every supported provider.
pub const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Default system prompt embedded in the binary.
///
/// Used when no `--prompt-file` is specified or the file does not exist.
/// Customize by creating `.github/review-prompt.md` in your repository root.
/// See [docs/USAGE.md](https://github.com/nebulaideas/rs-guard/blob/main/docs/USAGE.md#customizing-the-review-prompt)
/// for project-specific templates.
pub const DEFAULT_PROMPT: &str = r#"You are a senior software engineer performing a code review on a Pull Request diff.
Review each change as if it will deploy directly to production.

## Focus Areas (in priority order)
1. **Correctness:** logic errors, broken control flow, missing edge cases, off-by-one
2. **Security:** injection vectors, missing auth checks, exposed secrets, unsafe input handling
3. **Error handling:** swallowed errors, missing propagation, unhandled failure modes
4. **API contracts:** breaking changes, missing validation, inconsistent responses
5. **Resource management:** leaks, unbounded allocations, connections not released

## Severity Guidelines
- **Critical Bug:** would cause runtime error, data loss, or incorrect behavior in production
- **Security Issue:** vulnerability that exposes data, grants unauthorized access, or enables injection

## Verdict Guidelines
- **POSITIVE** if the diff is fundamentally sound and ready to merge
- **NEGATIVE** if there are Critical Bugs or Security Issues that should block merging

For each finding, explain the problem and suggest a fix.

At the end of your response, include exactly this metadata block (do not modify the format):

[RS_GUARD_VERDICT_METADATA]
Verdict: POSITIVE or NEGATIVE
CriticalBugs: <count>
SecurityIssues: <count>
"#;

/// Provider-specific settings in the TOML configuration file.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct ProviderTomlConfig {
    /// Environment variable name for the API key.
    pub api_key_env: Option<String>,
    /// Custom base URL for the provider API.
    pub base_url: Option<String>,
    /// HTTP referer for attribution (OpenRouter only).
    pub http_referer: Option<String>,
}

/// Circuit breaker configuration in the TOML file.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct CircuitBreakerTomlConfig {
    /// Whether the circuit breaker is enabled.
    pub enabled: bool,
    /// Consecutive failures before opening the circuit.
    pub threshold: Option<u32>,
    /// Cooldown period in seconds before auto-reset.
    pub cooldown_secs: Option<u64>,
}

/// Per-provider pricing configuration in the TOML file.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct PricingTomlConfig {
    /// Input price in cents per million tokens.
    pub input_per_million: u64,
    /// Output price in cents per million tokens.
    pub output_per_million: u64,
}

/// Top-level TOML configuration structure.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct TomlConfig {
    /// Default LLM provider name.
    pub provider: Option<String>,
    /// Default model identifier.
    pub model: Option<String>,
    /// Default sampling temperature.
    pub temperature: Option<f32>,
    /// Maximum tokens for LLM completions.
    pub max_tokens: Option<u32>,
    /// Lines to preserve from the start of the diff when chunking.
    ///
    /// Overrides [`crate::diff::DEFAULT_CHUNK_HEAD_LINES`].
    pub chunk_head_lines: Option<usize>,
    /// Lines to preserve from the end of the diff when chunking.
    ///
    /// Overrides [`crate::diff::DEFAULT_CHUNK_TAIL_LINES`].
    pub chunk_tail_lines: Option<usize>,
    /// Per-provider configuration sections.
    pub providers: Option<HashMap<String, ProviderTomlConfig>>,
    /// Custom cache directory path (default: git-root/.rs-guard/cache or cwd/.rs-guard/cache).
    pub cache_dir: Option<String>,
    /// Circuit breaker configuration.
    pub circuit_breaker: Option<CircuitBreakerTomlConfig>,
    /// Per-provider pricing overrides.
    pub pricing: Option<HashMap<String, PricingTomlConfig>>,
    /// Whether to automatically add the cache directory to `.gitignore`.
    pub auto_gitignore: Option<bool>,
}

/// Parses a `.reviewer.toml` configuration file.
///
/// Returns `Ok(None)` if the file does not exist, allowing callers to
/// proceed with environment variable defaults.
///
/// # Errors
///
/// Returns [`RsGuardError::Config`] if the file exists but cannot be read
/// or parsed.
pub fn load_toml_config(path: &Path) -> Result<Option<TomlConfig>, RsGuardError> {
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(path).map_err(|e| {
        RsGuardError::Config(format!(
            "Failed to read config file '{}': {}",
            path.display(),
            e
        ))
    })?;

    let config: TomlConfig = toml::from_str(&content).map_err(|e| {
        RsGuardError::Config(format!(
            "Failed to parse config file '{}': {}",
            path.display(),
            e
        ))
    })?;

    Ok(Some(config))
}

/// Returns the standard environment variable name for the API key of the given provider.
///
/// # Errors
///
/// Returns [`RsGuardError::Config`] if the provider name is not recognized.
fn standard_api_key_env_var(provider: &str) -> Result<&'static str, RsGuardError> {
    find_provider(provider)
        .map(|m| m.api_key_env)
        .ok_or_else(|| {
            let names: Vec<&str> = crate::llm::providers::known_provider_names();
            RsGuardError::Config(format!(
                "Unknown provider: '{}'. Supported: {}",
                provider,
                names.join(", ")
            ))
        })
}

/// Returns the default model for the given known provider.
///
/// # Errors
///
/// Returns [`RsGuardError::Config`] if the provider name is not recognized.
fn default_model(provider: &str) -> Result<&'static str, RsGuardError> {
    find_provider(provider)
        .map(|m| m.default_model)
        .ok_or_else(|| {
            let names: Vec<&str> = crate::llm::providers::known_provider_names();
            RsGuardError::Config(format!(
                "Unknown provider: '{}'. Supported: {}",
                provider,
                names.join(", ")
            ))
        })
}

/// Validates a provider base URL in local mode with guardrails.
///
/// Unlike CI mode (which rejects non-allowlisted hosts), local mode only
/// **warns** about potentially dangerous configurations that could lead to
/// accidental token exfiltration. The URL is still accepted, because the
/// user controls their own machine — but the warning ensures they are
/// aware of the risk.
///
/// Guardrails:
/// - Non-HTTPS URLs: warn about plaintext token transmission.
/// - Loopback addresses: warn about tokens being sent to local servers
///   (common with Ollama/LM Studio, but risky if unintentional).
/// - Non-allowlisted hosts: warn that the API key will be sent to an
///   unrecognized endpoint.
///
/// # Errors
///
/// Returns [`RsGuardError::Config`] if the URL is malformed.
fn validate_local_provider_base_url(base_url: &str) -> Result<(), RsGuardError> {
    let parsed = url::Url::parse(base_url).map_err(|_| {
        RsGuardError::Config(format!(
            "Provider base URL is malformed: '{}'. Expected format: https://host/path",
            base_url
        ))
    })?;

    let host = parsed.host_str().unwrap_or("");

    if parsed.scheme() != "https" {
        log::warn!(
            "Provider base URL '{}' uses {} (not HTTPS). API keys will be transmitted in plaintext. \
             This is risky if the traffic leaves your machine.",
            base_url,
            parsed.scheme()
        );
    } else if host == "127.0.0.1"
        || host == "localhost"
        || host == "[::1]"
        || host == "0.0.0.0"
        || host == "[::]"
    {
        log::warn!(
            "Provider base URL '{}' points to a loopback address. \
             Your API key will be sent to a local server. \
             Ensure this is intentional (e.g. Ollama, LM Studio).",
            base_url
        );
    } else if !providers::all_ci_allowed_hosts()
        .iter()
        .any(|&(s, h)| parsed.scheme() == s && host == h)
    {
        log::warn!(
            "Provider base URL '{}' (host: {}) is not a recognized LLM provider endpoint. \
             Your API key will be sent to a third-party server. \
             Verify this is intentional.",
            base_url,
            host
        );
    }

    Ok(())
}

/// Resolves the API key environment variable for a provider, checking
/// TOML overrides first, then falling back to the standard mapping.
fn resolve_api_key_env_var(
    provider: &str,
    toml_providers: Option<&HashMap<String, ProviderTomlConfig>>,
) -> Result<String, RsGuardError> {
    if let Some(providers) = toml_providers {
        if let Some(toml_provider) = providers.get(provider) {
            if let Some(ref env_var) = toml_provider.api_key_env {
                return Ok(env_var.clone());
            }
        }
    }
    standard_api_key_env_var(provider).map(|s| s.to_string())
}

/// Validated CI configuration with all required fields present.
///
/// Created by [`Config::validate_for_ci()`] when running in CI mode.
/// This struct guarantees that all CI-required fields are present,
/// eliminating the need for `.expect()` calls in the pipeline.
#[derive(Debug, Clone)]
pub struct CiConfig {
    /// GitHub authentication token.
    pub github_token: String,
    /// Pull request number.
    pub pr_number: u64,
    /// Repository owner (e.g., "nebulaideas").
    pub repo_owner: String,
    /// Repository name (e.g., "rs-guard").
    pub repo_name: String,
    /// GitHub API base URL.
    pub github_base_url: String,
}

/// Resolved application configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// LLM provider name (e.g. `"deepseek"`).
    pub provider: String,
    /// Model identifier for the LLM provider.
    pub model: String,
    /// Sampling temperature for LLM completions.
    pub temperature: f32,
    /// API key for the selected LLM provider.
    pub api_key: String,
    /// GitHub authentication token (required in CI mode).
    pub github_token: Option<String>,
    /// Pull request number (required in CI mode).
    pub pr_number: Option<u64>,
    /// Repository owner (required in CI mode).
    pub repo_owner: Option<String>,
    /// Repository name (required in CI mode).
    pub repo_name: Option<String>,
    /// System prompt text sent to the LLM.
    pub prompt: String,
    /// Whether the tool is running in CI mode.
    pub is_ci: bool,
    /// GitHub API base URL.
    pub github_base_url: String,
    /// Provider-specific configuration overrides from TOML.
    pub provider_config: ProviderConfig,
    /// TOML per-provider sections (retained for `apply_args` provider switching).
    toml_providers: HashMap<String, ProviderTomlConfig>,
    /// Whether the model was explicitly set via CLI `--model` flag.
    /// When `false` and the provider changes, the model resets to the new provider's default.
    /// Env/TOML model values are NOT carried across provider changes.
    model_set_via_cli: bool,
    /// Bypass the response cache, forcing an LLM API call.
    pub no_cache: bool,
    /// Dry-run mode: run pipeline without submitting or blocking.
    pub dry_run: bool,
    /// Custom cache directory path.
    pub cache_dir: Option<String>,
    /// Optional circuit breaker configuration.
    pub circuit_breaker: Option<crate::retry::CircuitBreaker>,
    /// Optional per-provider pricing overrides.
    pub pricing: Option<HashMap<String, PricingTomlConfig>>,
    /// Whether to automatically add the cache directory to `.gitignore`.
    pub auto_gitignore: bool,
    /// Lines to preserve from the start of the diff when chunking.
    pub chunk_head_lines: usize,
    /// Lines to preserve from the end of the diff when chunking.
    pub chunk_tail_lines: usize,
}

impl Config {
    /// Creates a minimal config for integration tests.
    ///
    /// This constructor is marked `#[doc(hidden)]` and only intended for use
    /// in test code. It provides minimal defaults that satisfy the type's
    /// invariants but are not meaningful for production use.
    ///
    /// # Note
    ///
    /// This does NOT derive from `Default` to avoid accidental use in
    /// production code where proper configuration resolution is required.
    #[doc(hidden)]
    pub fn empty() -> Self {
        Self {
            provider: String::new(),
            model: String::new(),
            temperature: 0.1,
            api_key: String::new(),
            github_token: None,
            pr_number: None,
            repo_owner: None,
            repo_name: None,
            prompt: String::new(),
            is_ci: false,
            github_base_url: String::new(),
            provider_config: ProviderConfig::default(),
            toml_providers: HashMap::new(),
            model_set_via_cli: false,
            no_cache: false,
            dry_run: false,
            cache_dir: None,
            circuit_breaker: None,
            pricing: None,
            auto_gitignore: true,
            chunk_head_lines: crate::diff::DEFAULT_CHUNK_HEAD_LINES,
            chunk_tail_lines: crate::diff::DEFAULT_CHUNK_TAIL_LINES,
        }
    }

    /// Builds configuration from environment variables with optional TOML defaults.
    ///
    /// Resolution order: Environment variables > TOML file > Hardcoded defaults.
    ///
    /// # Arguments
    ///
    /// * `toml` — Optional TOML configuration loaded from `.reviewer.toml`.
    ///
    /// # Errors
    ///
    /// Returns [`RsGuardError::Config`] if the required API key is not set
    /// or if the provider name is not recognized.
    pub fn from_env(toml: Option<TomlConfig>) -> Result<Self, RsGuardError> {
        let is_ci = std::env::var("GITHUB_ACTIONS").is_ok();

        let toml_providers = toml
            .as_ref()
            .and_then(|t| t.providers.clone())
            .unwrap_or_default();

        // Provider: env > toml > default
        let provider = std::env::var("RS_GUARD_PROVIDER")
            .ok()
            .or_else(|| toml.as_ref().and_then(|t| t.provider.clone()))
            .unwrap_or_else(|| "deepseek".to_string());

        // Validate provider is known
        standard_api_key_env_var(&provider)?;

        // Resolve API key env var: TOML override > standard mapping
        let api_key_env = resolve_api_key_env_var(&provider, Some(&toml_providers))?;

        let api_key = std::env::var(&api_key_env).map_err(|_| {
            RsGuardError::Config(format!(
                "API key not found. Set {} for provider '{}'",
                api_key_env, provider
            ))
        })?;

        let github_token = std::env::var("GITHUB_TOKEN").ok();
        let pr_number = std::env::var("PR_NUMBER").ok().and_then(|s| s.parse().ok());
        let repo_full_name = std::env::var("REPO_FULL_NAME").ok();

        let (repo_owner, repo_name) = match repo_full_name {
            Some(full) => {
                let parts: Vec<&str> = full.splitn(2, '/').collect();
                if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
                    (Some(parts[0].to_string()), Some(parts[1].to_string()))
                } else {
                    return Err(RsGuardError::Config(format!(
                        "REPO_FULL_NAME must be in 'owner/repo' format, got: '{}'",
                        full
                    )));
                }
            }
            None => (None, None),
        };

        let github_base_url = std::env::var("GITHUB_API_URL")
            .unwrap_or_else(|_| "https://api.github.com".to_string());

        // Model: env > toml > provider default
        let env_model = std::env::var("RS_GUARD_MODEL").ok();
        let toml_model = toml.as_ref().and_then(|t| t.model.clone());
        let model = env_model.or(toml_model).unwrap_or_else(|| {
            default_model(&provider)
                .expect("provider already validated above")
                .to_string()
        });

        // Temperature: env > toml > default (validated to [0.0, 2.0])
        // An unparseable RS_GUARD_TEMPERATURE value is an explicit configuration
        // error — it is never silently ignored so that misconfiguration is caught
        // early rather than producing unexpected reviews.
        let temperature = match std::env::var("RS_GUARD_TEMPERATURE") {
            Ok(val) => val.parse::<f32>().map_err(|_| {
                RsGuardError::Config(format!(
                    "Invalid RS_GUARD_TEMPERATURE '{}': must be a number between 0.0 and 2.0",
                    val
                ))
            })?,
            Err(_) => toml.as_ref().and_then(|t| t.temperature).unwrap_or(0.1),
        };
        if !(0.0..=2.0).contains(&temperature) {
            return Err(RsGuardError::Config(format!(
                "Temperature must be between 0.0 and 2.0, got: {}",
                temperature
            )));
        }

        // Max tokens: env > toml > DEFAULT_MAX_TOKENS (4096)
        //
        // Never allow None here — a None max_tokens lets the provider truncate
        // the response before the [RS_GUARD_VERDICT_METADATA] block, which causes
        // the fallback tag-counting path to activate and may produce incorrect
        // APPROVE verdicts on clean diffs.
        let max_tokens: Option<u32> = std::env::var("RS_GUARD_MAX_TOKENS")
            .ok()
            .and_then(|s| s.parse().ok())
            .or(toml.as_ref().and_then(|t| t.max_tokens))
            .or(Some(DEFAULT_MAX_TOKENS));

        // Chunking thresholds: toml > default
        let chunk_head_lines = toml
            .as_ref()
            .and_then(|t| t.chunk_head_lines)
            .unwrap_or(crate::diff::DEFAULT_CHUNK_HEAD_LINES);
        let chunk_tail_lines = toml
            .as_ref()
            .and_then(|t| t.chunk_tail_lines)
            .unwrap_or(crate::diff::DEFAULT_CHUNK_TAIL_LINES);

        // Provider config from TOML — validate base_url against SSRF allowlist in CI
        // In local mode, warn about potentially dangerous URLs to prevent accidental token exfiltration
        let toml_provider = toml_providers.get(&provider);
        let base_url = toml_provider.and_then(|p| p.base_url.clone());
        if is_ci {
            if let Some(ref url) = base_url {
                validate_provider_base_url(url)?;
            }
        } else if let Some(ref url) = base_url {
            validate_local_provider_base_url(url)?;
        }

        let provider_config = ProviderConfig {
            base_url,
            http_referer: toml_provider.and_then(|p| p.http_referer.clone()),
            max_tokens,
            model: model.clone(),
        };

        let cache_dir = toml.as_ref().and_then(|t| t.cache_dir.clone());

        let circuit_breaker = toml
            .as_ref()
            .and_then(|t| t.circuit_breaker.as_ref())
            .and_then(|cb| {
                if cb.enabled {
                    Some(crate::retry::CircuitBreaker::new(
                        cb.threshold.unwrap_or(3),
                        cb.cooldown_secs.unwrap_or(60),
                    ))
                } else {
                    None
                }
            });

        let pricing = toml.as_ref().and_then(|t| t.pricing.clone());
        let auto_gitignore = toml.as_ref().and_then(|t| t.auto_gitignore).unwrap_or(true);

        Ok(Config {
            provider,
            model,
            temperature,
            api_key,
            github_token,
            pr_number,
            repo_owner,
            repo_name,
            prompt: DEFAULT_PROMPT.to_string(),
            is_ci,
            github_base_url,
            provider_config,
            toml_providers,
            model_set_via_cli: false,
            no_cache: false,
            dry_run: false,
            cache_dir,
            circuit_breaker,
            pricing,
            auto_gitignore,
            chunk_head_lines,
            chunk_tail_lines,
        })
    }

    /// Applies CLI argument overrides to the configuration.
    ///
    /// CLI flags take precedence over environment variables and TOML for `model`,
    /// `temperature`, `provider`, and `max_tokens`. If the provider changes, the
    /// API key is re-resolved (respecting TOML `api_key_env` overrides) and the
    /// model is reset to the new provider's default unless explicitly set via
    /// the CLI `--model` flag.
    ///
    /// # Errors
    ///
    /// Returns [`RsGuardError::Config`] if the provider changes and the
    /// new provider's API key environment variable is not set.
    pub fn apply_args(&mut self, args: &Args) -> Result<(), RsGuardError> {
        if let Some(ref provider) = args.provider {
            if *provider != self.provider {
                let new_env = resolve_api_key_env_var(provider, Some(&self.toml_providers))?;
                let new_key = std::env::var(&new_env).map_err(|_| {
                    RsGuardError::Config(format!(
                        "API key not found. Set {} for provider '{}'",
                        new_env, provider
                    ))
                })?;
                let old_provider = self.provider.clone();
                self.api_key = new_key;
                self.provider = provider.clone();

                // Update provider_config from TOML for the new provider
                let toml_provider = self.toml_providers.get(provider);
                let new_base_url = toml_provider.and_then(|p| p.base_url.clone());
                if self.is_ci {
                    if let Some(ref url) = new_base_url {
                        validate_provider_base_url(url)?;
                    }
                } else if let Some(ref url) = new_base_url {
                    validate_local_provider_base_url(url)?;
                }
                log::debug!(
                    "Provider switch '{} -> '{}': base_url={:?}, http_referer={:?}",
                    old_provider,
                    provider,
                    new_base_url,
                    toml_provider.and_then(|p| p.http_referer.as_deref())
                );
                self.provider_config.base_url = new_base_url;
                self.provider_config.http_referer =
                    toml_provider.and_then(|p| p.http_referer.clone());

                // Reset model to new provider's default unless CLI --model was used
                if !self.model_set_via_cli && args.model.is_none() {
                    self.model = default_model(provider)
                        .expect("provider already validated above")
                        .to_string();
                }

                // Always sync provider_config.model with self.model after provider change.
                // This prevents provider_config.model from becoming stale when
                // model_set_via_cli is true and no --model flag is passed.
                self.provider_config.model = self.model.clone();
            }
        }

        if let Some(ref model) = args.model {
            self.model = model.clone();
            self.provider_config.model = model.clone();
            self.model_set_via_cli = true;
        }
        if let Some(temp) = args.temperature {
            self.temperature = temp;
        }
        if let Some(max_tokens) = args.max_tokens {
            self.provider_config.max_tokens = Some(max_tokens);
        }
        if args.no_cache {
            self.no_cache = true;
        }
        if args.dry_run {
            self.dry_run = true;
        }

        Ok(())
    }

    /// Loads the system prompt from a file, falling back to the default.
    ///
    /// If the file does not exist, the embedded [`DEFAULT_PROMPT`] is used.
    ///
    /// # Errors
    ///
    /// Returns [`RsGuardError::Config`] if the file exists but cannot be read.
    pub fn load_prompt_file(&mut self, path: &Path) -> Result<(), RsGuardError> {
        if path.exists() {
            let content = std::fs::read_to_string(path)
                .map_err(|e| RsGuardError::Config(format!("Failed to read prompt file: {}", e)))?;
            self.prompt = content;
        }
        Ok(())
    }

    /// Validates that all required fields are present for CI mode.
    ///
    /// In local mode, validates the GitHub base URL only. In CI mode,
    /// additionally requires `GITHUB_TOKEN`, `PR_NUMBER`, and `REPO_FULL_NAME`.
    /// The `github_base_url` is always validated against the allowlist.
    ///
    /// # Errors
    ///
    /// Returns [`RsGuardError::Config`] if validation fails.
    pub fn validate_for_ci(&self) -> Result<CiConfig, RsGuardError> {
        validate_github_base_url(&self.github_base_url)?;

        if !self.is_ci {
            return Err(RsGuardError::Config(
                "validate_for_ci() called but not in CI mode".to_string(),
            ));
        }

        let github_token = self.github_token.clone().ok_or_else(|| {
            RsGuardError::Config("GITHUB_TOKEN is required in CI mode".to_string())
        })?;

        let pr_number = self
            .pr_number
            .ok_or_else(|| RsGuardError::Config("PR_NUMBER is required in CI mode".to_string()))?;

        let repo_owner = self.repo_owner.clone().ok_or_else(|| {
            RsGuardError::Config(
                "REPO_FULL_NAME is required in CI mode (format: owner/repo)".to_string(),
            )
        })?;

        let repo_name = self.repo_name.clone().ok_or_else(|| {
            RsGuardError::Config(
                "REPO_FULL_NAME is required in CI mode (format: owner/repo)".to_string(),
            )
        })?;

        Ok(CiConfig {
            github_token,
            pr_number,
            repo_owner,
            repo_name,
            github_base_url: self.github_base_url.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_api_key_env_var_mapping() {
        assert_eq!(
            standard_api_key_env_var("deepseek").unwrap(),
            "DEEPSEEK_API_KEY"
        );
        assert_eq!(standard_api_key_env_var("kimi").unwrap(), "KIMI_API_KEY");
        assert_eq!(
            standard_api_key_env_var("qwen").unwrap(),
            "DASHSCOPE_API_KEY"
        );
        assert_eq!(
            standard_api_key_env_var("openrouter").unwrap(),
            "OPENROUTER_API_KEY"
        );
        assert_eq!(
            standard_api_key_env_var("openai").unwrap(),
            "OPENAI_API_KEY"
        );
    }

    #[test]
    fn test_unknown_provider_returns_error() {
        let result = standard_api_key_env_var("unknown");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unknown provider"));
        assert!(err.contains("unknown"));
    }

    #[test]
    fn test_default_model_mapping() {
        assert_eq!(default_model("deepseek").unwrap(), "deepseek-v4-flash");
        assert_eq!(default_model("kimi").unwrap(), "kimi-k2.5");
        assert_eq!(default_model("qwen").unwrap(), "qwen-plus");
        assert_eq!(default_model("openrouter").unwrap(), "openai/gpt-4o-mini");
        assert_eq!(default_model("openai").unwrap(), "gpt-4o-mini");
    }

    #[test]
    fn test_default_model_unknown_provider_returns_error() {
        let result = default_model("unknown");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unknown provider"));
    }

    #[test]
    fn test_resolve_api_key_env_var_toml_override() {
        let mut providers = HashMap::new();
        providers.insert(
            "openai".to_string(),
            ProviderTomlConfig {
                api_key_env: Some("MY_CUSTOM_KEY".to_string()),
                base_url: None,
                http_referer: None,
            },
        );

        let result = resolve_api_key_env_var("openai", Some(&providers)).unwrap();
        assert_eq!(result, "MY_CUSTOM_KEY");
    }

    #[test]
    fn test_resolve_api_key_env_var_standard_fallback() {
        let providers = HashMap::new();
        let result = resolve_api_key_env_var("deepseek", Some(&providers)).unwrap();
        assert_eq!(result, "DEEPSEEK_API_KEY");
    }

    #[test]
    fn test_validate_local_provider_base_url_http_warns() {
        // This test verifies that HTTP URLs are warned about but still accepted in local mode
        // The function logs warnings but returns Ok()
        let result = validate_local_provider_base_url("http://api.example.com/v1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_local_provider_base_url_loopback_warns() {
        // This test verifies that loopback addresses are warned about but still accepted in local mode
        let result = validate_local_provider_base_url("http://127.0.0.1:11434/v1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_local_provider_base_url_unknown_host_warns() {
        // This test verifies that non-allowlisted hosts are warned about but still accepted in local mode
        let result = validate_local_provider_base_url("https://custom-llm.example.com/v1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_local_provider_base_url_malformed_errors() {
        let result = validate_local_provider_base_url("not-a-url");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("malformed"));
    }

    #[test]
    fn test_validate_local_provider_base_url_known_host_ok() {
        // Known hosts should not warn
        let result = validate_local_provider_base_url("https://api.deepseek.com/v1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_for_ci_local_mode_valid() {
        let mut config = Config::empty();
        config.is_ci = false;
        config.github_base_url = "https://api.github.com".to_string();
        // In local mode, validate_for_ci() should return an error
        // because CI-specific fields are not available
        assert!(config.validate_for_ci().is_err());
    }

    #[test]
    fn test_validate_for_ci_missing_github_token() {
        let mut config = Config::empty();
        config.is_ci = true;
        config.github_base_url = "https://api.github.com".to_string();
        config.github_token = None;
        config.pr_number = Some(1);
        config.repo_owner = Some("owner".to_string());
        config.repo_name = Some("repo".to_string());
        let result = config.validate_for_ci();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("GITHUB_TOKEN"));
    }

    #[test]
    fn test_validate_for_ci_missing_pr_number() {
        let mut config = Config::empty();
        config.is_ci = true;
        config.github_base_url = "https://api.github.com".to_string();
        config.github_token = Some("token".to_string());
        config.pr_number = None;
        config.repo_owner = Some("owner".to_string());
        config.repo_name = Some("repo".to_string());
        let result = config.validate_for_ci();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("PR_NUMBER"));
    }

    #[test]
    fn test_validate_for_ci_missing_repo_owner() {
        let mut config = Config::empty();
        config.is_ci = true;
        config.github_base_url = "https://api.github.com".to_string();
        config.github_token = Some("token".to_string());
        config.pr_number = Some(1);
        config.repo_owner = None;
        config.repo_name = Some("repo".to_string());
        let result = config.validate_for_ci();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("REPO_FULL_NAME"));
    }

    #[test]
    fn test_validate_for_ci_missing_repo_name() {
        let mut config = Config::empty();
        config.is_ci = true;
        config.github_base_url = "https://api.github.com".to_string();
        config.github_token = Some("token".to_string());
        config.pr_number = Some(1);
        config.repo_owner = Some("owner".to_string());
        config.repo_name = None;
        let result = config.validate_for_ci();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("REPO_FULL_NAME"));
    }

    #[test]
    fn test_validate_for_ci_all_fields_present() {
        let mut config = Config::empty();
        config.is_ci = true;
        config.github_base_url = "https://api.github.com".to_string();
        config.github_token = Some("token".to_string());
        config.pr_number = Some(42);
        config.repo_owner = Some("owner".to_string());
        config.repo_name = Some("repo".to_string());
        assert!(config.validate_for_ci().is_ok());
    }

    #[test]
    fn test_validate_for_ci_invalid_base_url() {
        let mut config = Config::empty();
        config.is_ci = true;
        config.github_base_url = "http://evil.com".to_string();
        config.github_token = Some("token".to_string());
        config.pr_number = Some(1);
        config.repo_owner = Some("owner".to_string());
        config.repo_name = Some("repo".to_string());
        let result = config.validate_for_ci();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_for_ci_returns_ci_config() {
        let mut config = Config::empty();
        config.is_ci = true;
        config.github_token = Some("test-token".to_string());
        config.pr_number = Some(42);
        config.repo_owner = Some("owner".to_string());
        config.repo_name = Some("repo".to_string());
        config.github_base_url = "https://api.github.com".to_string();

        let ci_config = config.validate_for_ci().expect("should validate");
        assert_eq!(ci_config.github_token, "test-token");
        assert_eq!(ci_config.pr_number, 42);
        assert_eq!(ci_config.repo_owner, "owner");
        assert_eq!(ci_config.repo_name, "repo");
        assert_eq!(ci_config.github_base_url, "https://api.github.com");
    }

    #[test]
    fn test_validate_for_ci_not_in_ci_mode_returns_error() {
        let mut config = Config::empty();
        config.is_ci = false;
        config.github_base_url = "https://api.github.com".to_string();

        let result = config.validate_for_ci();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not in CI mode"));
    }

    #[test]
    fn test_load_prompt_file_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let prompt_path = dir.path().join("prompt.md");
        std::fs::write(&prompt_path, "Custom review prompt").unwrap();

        let mut config = Config::empty();
        config.load_prompt_file(&prompt_path).unwrap();
        assert_eq!(config.prompt, "Custom review prompt");
    }

    #[test]
    fn test_load_prompt_file_missing_file_keeps_default() {
        let mut config = Config::empty();
        config.prompt = "default prompt".to_string();
        let result = config.load_prompt_file(std::path::Path::new("/nonexistent/prompt.md"));
        assert!(result.is_ok());
        assert_eq!(config.prompt, "default prompt");
    }

    #[test]
    fn test_load_prompt_file_unreadable_file() {
        let dir = tempfile::tempdir().unwrap();
        let prompt_path = dir.path().join("unreadable.md");
        std::fs::write(&prompt_path, "content").unwrap();
        // Make file unreadable by removing permissions
        #[cfg(unix)]
        std::fs::set_permissions(
            &prompt_path,
            std::os::unix::fs::PermissionsExt::from_mode(0o000),
        )
        .unwrap();

        let mut config = Config::empty();
        let result = config.load_prompt_file(&prompt_path);

        // Restore permissions for cleanup
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&prompt_path, PermissionsExt::from_mode(0o644)).ok();
        }

        // On Unix, unreadable files should error; on other platforms, may succeed
        #[cfg(unix)]
        assert!(result.is_err());
    }

    #[test]
    fn test_config_empty_has_dry_run_false() {
        let config = Config::empty();
        assert!(!config.dry_run);
    }

    #[test]
    fn test_config_empty_has_cache_dir_none() {
        let config = Config::empty();
        assert!(config.cache_dir.is_none());
    }

    #[test]
    fn test_apply_args_sets_dry_run() {
        use clap::Parser;
        let mut config = Config::empty();
        assert!(!config.dry_run);

        let args = crate::cli::Args::parse_from(["rs-guard", "--dry-run"]);
        config.apply_args(&args).unwrap();
        assert!(config.dry_run);
    }

    #[test]
    fn test_circuit_breaker_disabled_produces_none() {
        let toml = TomlConfig {
            circuit_breaker: Some(CircuitBreakerTomlConfig {
                enabled: false,
                threshold: Some(5),
                cooldown_secs: Some(120),
            }),
            ..Default::default()
        };

        let circuit_breaker = toml.circuit_breaker.as_ref().and_then(|cb| {
            if cb.enabled {
                Some(crate::retry::CircuitBreaker::new(
                    cb.threshold.unwrap_or(3),
                    cb.cooldown_secs.unwrap_or(60),
                ))
            } else {
                None
            }
        });

        assert!(
            circuit_breaker.is_none(),
            "circuit_breaker should be None when enabled=false"
        );
    }

    #[test]
    fn test_circuit_breaker_enabled_produces_some() {
        let toml = TomlConfig {
            circuit_breaker: Some(CircuitBreakerTomlConfig {
                enabled: true,
                threshold: Some(5),
                cooldown_secs: Some(120),
            }),
            ..Default::default()
        };

        let circuit_breaker = toml.circuit_breaker.as_ref().and_then(|cb| {
            if cb.enabled {
                Some(crate::retry::CircuitBreaker::new(
                    cb.threshold.unwrap_or(3),
                    cb.cooldown_secs.unwrap_or(60),
                ))
            } else {
                None
            }
        });

        assert!(
            circuit_breaker.is_some(),
            "circuit_breaker should be Some when enabled=true"
        );
    }
}
