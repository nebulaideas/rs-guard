//! Configuration resolution from environment variables, TOML file, and CLI arguments.
//!
//! Handles detection of CI vs local mode, API key resolution, and
//! validation of required fields for GitHub PR review submission.
//!
//! Configuration resolution order: CLI flags > Environment variables > TOML file > Defaults

use crate::cli::Args;
use crate::error::DiffguardError;
use crate::http::{validate_github_base_url, validate_provider_base_url};
use crate::llm::providers::{self, find_provider};
use crate::llm::ProviderConfig;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Default system prompt embedded in the binary.
///
/// Used when no `--prompt-file` is specified or the file does not exist.
pub const DEFAULT_PROMPT: &str = r#"You are a senior software engineer performing a code review on a Pull Request diff.

Review the provided diff carefully. Identify:
- Critical bugs: issues that would cause runtime errors, data loss, or incorrect behavior
- Security issues: vulnerabilities, injection risks, auth flaws, secrets exposure

For each finding, explain the problem and suggest a fix.

At the end of your response, include exactly this metadata block (do not modify the format):

[DIFFGUARD_VERDICT_METADATA]
Verdict: POSITIVE or NEGATIVE
CriticalBugs: <count>
SecurityIssues: <count>

Guidelines:
- Verdict is POSITIVE if the code is fundamentally sound and ready to merge
- Verdict is NEGATIVE if there are serious issues that should block merging
- CriticalBugs: count of bugs that would cause incorrect behavior in production
- SecurityIssues: count of security vulnerabilities or risks
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
    /// Per-provider configuration sections.
    pub providers: Option<HashMap<String, ProviderTomlConfig>>,
}

/// Parses a `.reviewer.toml` configuration file.
///
/// Returns `Ok(None)` if the file does not exist, allowing callers to
/// proceed with environment variable defaults.
///
/// # Errors
///
/// Returns [`DiffguardError::Config`] if the file exists but cannot be read
/// or parsed.
pub fn load_toml_config(path: &Path) -> Result<Option<TomlConfig>, DiffguardError> {
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(path).map_err(|e| {
        DiffguardError::Config(format!(
            "Failed to read config file '{}': {}",
            path.display(),
            e
        ))
    })?;

    let config: TomlConfig = toml::from_str(&content).map_err(|e| {
        DiffguardError::Config(format!(
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
/// Returns [`DiffguardError::Config`] if the provider name is not recognized.
fn standard_api_key_env_var(provider: &str) -> Result<&'static str, DiffguardError> {
    find_provider(provider)
        .map(|m| m.api_key_env)
        .ok_or_else(|| {
            let names: Vec<&str> = crate::llm::providers::known_provider_names();
            DiffguardError::Config(format!(
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
/// Returns [`DiffguardError::Config`] if the provider name is not recognized.
fn default_model(provider: &str) -> Result<&'static str, DiffguardError> {
    find_provider(provider)
        .map(|m| m.default_model)
        .ok_or_else(|| {
            let names: Vec<&str> = crate::llm::providers::known_provider_names();
            DiffguardError::Config(format!(
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
/// Returns [`DiffguardError::Config`] if the URL is malformed.
fn validate_local_provider_base_url(base_url: &str) -> Result<(), DiffguardError> {
    let parsed = url::Url::parse(base_url).map_err(|_| {
        DiffguardError::Config(format!(
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
) -> Result<String, DiffguardError> {
    if let Some(providers) = toml_providers {
        if let Some(toml_provider) = providers.get(provider) {
            if let Some(ref env_var) = toml_provider.api_key_env {
                return Ok(env_var.clone());
            }
        }
    }
    standard_api_key_env_var(provider).map(|s| s.to_string())
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
}

impl Config {
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
    /// Returns [`DiffguardError::Config`] if the required API key is not set
    /// or if the provider name is not recognized.
    pub fn from_env(toml: Option<TomlConfig>) -> Result<Self, DiffguardError> {
        let is_ci = std::env::var("GITHUB_ACTIONS").is_ok();

        let toml_providers = toml
            .as_ref()
            .and_then(|t| t.providers.clone())
            .unwrap_or_default();

        // Provider: env > toml > default
        let provider = std::env::var("DIFFGUARD_PROVIDER")
            .ok()
            .or_else(|| toml.as_ref().and_then(|t| t.provider.clone()))
            .unwrap_or_else(|| "deepseek".to_string());

        // Validate provider is known
        standard_api_key_env_var(&provider)?;

        // Resolve API key env var: TOML override > standard mapping
        let api_key_env = resolve_api_key_env_var(&provider, Some(&toml_providers))?;

        let api_key = std::env::var(&api_key_env).map_err(|_| {
            DiffguardError::Config(format!(
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
                    return Err(DiffguardError::Config(format!(
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
        let env_model = std::env::var("DIFFGUARD_MODEL").ok();
        let toml_model = toml.as_ref().and_then(|t| t.model.clone());
        let model = env_model.or(toml_model).unwrap_or_else(|| {
            default_model(&provider)
                .unwrap_or("deepseek-v4-flash")
                .to_string()
        });

        // Temperature: env > toml > default
        let temperature = std::env::var("DIFFGUARD_TEMPERATURE")
            .ok()
            .and_then(|s| s.parse().ok())
            .or(toml.as_ref().and_then(|t| t.temperature))
            .unwrap_or(0.1);

        // Max tokens: env > toml > none (single source of truth in provider_config)
        let max_tokens = std::env::var("DIFFGUARD_MAX_TOKENS")
            .ok()
            .and_then(|s| s.parse().ok())
            .or(toml.as_ref().and_then(|t| t.max_tokens));

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
    /// Returns [`DiffguardError::Config`] if the provider changes and the
    /// new provider's API key environment variable is not set.
    pub fn apply_args(&mut self, args: &Args) -> Result<(), DiffguardError> {
        if let Some(ref provider) = args.provider {
            if *provider != self.provider {
                let new_env = resolve_api_key_env_var(provider, Some(&self.toml_providers))?;
                let new_key = std::env::var(&new_env).map_err(|_| {
                    DiffguardError::Config(format!(
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
                        .unwrap_or("deepseek-v4-flash")
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

        Ok(())
    }

    /// Loads the system prompt from a file, falling back to the default.
    ///
    /// If the file does not exist, the embedded [`DEFAULT_PROMPT`] is used.
    ///
    /// # Errors
    ///
    /// Returns [`DiffguardError::Config`] if the file exists but cannot be read.
    pub fn load_prompt_file(&mut self, path: &Path) -> Result<(), DiffguardError> {
        if path.exists() {
            let content = std::fs::read_to_string(path).map_err(|e| {
                DiffguardError::Config(format!("Failed to read prompt file: {}", e))
            })?;
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
    /// Returns [`DiffguardError::Config`] if validation fails.
    pub fn validate_for_ci(&self) -> Result<(), DiffguardError> {
        validate_github_base_url(&self.github_base_url)?;

        if self.is_ci {
            if self.github_token.is_none() {
                return Err(DiffguardError::Config(
                    "GITHUB_TOKEN is required in CI mode".to_string(),
                ));
            }
            if self.pr_number.is_none() {
                return Err(DiffguardError::Config(
                    "PR_NUMBER is required in CI mode".to_string(),
                ));
            }
            if self.repo_owner.is_none() || self.repo_name.is_none() {
                return Err(DiffguardError::Config(
                    "REPO_FULL_NAME is required in CI mode (format: owner/repo)".to_string(),
                ));
            }
        }
        Ok(())
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
}
