//! Configuration resolution from environment variables, TOML file, and CLI arguments.
//!
//! Handles detection of CI vs local mode, API key resolution, and
//! validation of required fields for GitHub PR review submission.
//!
//! Configuration resolution order: CLI flags > Environment variables > TOML file > Defaults

use crate::cli::Args;
use crate::error::DiffguardError;
use crate::http::validate_github_base_url;
use crate::llm::ProviderConfig;
use serde::Deserialize;
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
    pub providers: Option<std::collections::HashMap<String, ProviderTomlConfig>>,
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

/// Known provider identifiers.
const KNOWN_PROVIDERS: &[&str] = &["deepseek", "kimi", "qwen", "openrouter", "openai"];

/// Returns the standard environment variable name for the API key of the given provider.
///
/// # Errors
///
/// Returns [`DiffguardError::Config`] if the provider name is not recognized.
fn standard_api_key_env_var(provider: &str) -> Result<&'static str, DiffguardError> {
    match provider {
        "deepseek" => Ok("DEEPSEEK_API_KEY"),
        "kimi" => Ok("KIMI_API_KEY"),
        "qwen" => Ok("DASHSCOPE_API_KEY"),
        "openrouter" => Ok("OPENROUTER_API_KEY"),
        "openai" => Ok("OPENAI_API_KEY"),
        _ => Err(DiffguardError::Config(format!(
            "Unknown provider: '{}'. Supported: {}",
            provider,
            KNOWN_PROVIDERS.join(", ")
        ))),
    }
}

/// Returns the default model for the given known provider.
fn default_model(provider: &str) -> &'static str {
    match provider {
        "deepseek" => "deepseek-v4-flash",
        "kimi" => "kimi-k2.5",
        "qwen" => "qwen-plus",
        "openrouter" => "openai/gpt-4o-mini",
        "openai" => "gpt-4o-mini",
        _ => "deepseek-v4-flash",
    }
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
    /// Maximum tokens for LLM completions.
    pub max_tokens: Option<u32>,
    /// Provider-specific configuration overrides from TOML.
    pub provider_config: ProviderConfig,
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

        // Provider: env > toml > default
        let provider = std::env::var("DIFFGUARD_PROVIDER")
            .ok()
            .or_else(|| toml.as_ref().and_then(|t| t.provider.clone()))
            .unwrap_or_else(|| "deepseek".to_string());

        // Validate provider is known
        standard_api_key_env_var(&provider)?;

        // Resolve API key env var: TOML override > standard mapping
        let toml_provider = toml
            .as_ref()
            .and_then(|t| t.providers.as_ref())
            .and_then(|p| p.get(&provider));

        let api_key_env = toml_provider
            .and_then(|p| p.api_key_env.as_deref())
            .map(Ok)
            .unwrap_or_else(|| standard_api_key_env_var(&provider))?;

        let api_key = std::env::var(api_key_env).map_err(|_| {
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
                let parts: Vec<&str> = full.split('/').collect();
                if parts.len() == 2 {
                    (Some(parts[0].to_string()), Some(parts[1].to_string()))
                } else {
                    (None, None)
                }
            }
            None => (None, None),
        };

        let github_base_url = std::env::var("GITHUB_API_URL")
            .unwrap_or_else(|_| "https://api.github.com".to_string());

        // Model: env > toml > provider default
        let model = std::env::var("DIFFGUARD_MODEL")
            .ok()
            .or_else(|| toml.as_ref().and_then(|t| t.model.clone()))
            .unwrap_or_else(|| default_model(&provider).to_string());

        // Temperature: env > toml > default
        let temperature = std::env::var("DIFFGUARD_TEMPERATURE")
            .ok()
            .and_then(|s| s.parse().ok())
            .or(toml.as_ref().and_then(|t| t.temperature))
            .unwrap_or(0.1);

        // Max tokens: env > toml > none
        let max_tokens = std::env::var("DIFFGUARD_MAX_TOKENS")
            .ok()
            .and_then(|s| s.parse().ok())
            .or(toml.as_ref().and_then(|t| t.max_tokens));

        // Provider config from TOML
        let provider_config = ProviderConfig {
            base_url: toml_provider.and_then(|p| p.base_url.clone()),
            http_referer: toml_provider.and_then(|p| p.http_referer.clone()),
            max_tokens,
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
            max_tokens,
            provider_config,
        })
    }

    /// Applies CLI argument overrides to the configuration.
    ///
    /// CLI flags take precedence over environment variables and TOML for `model`,
    /// `temperature`, `provider`, and `max_tokens`. If the provider changes, the
    /// API key is re-resolved from the corresponding environment variable.
    ///
    /// # Errors
    ///
    /// Returns [`DiffguardError::Config`] if the provider changes and the
    /// new provider's API key environment variable is not set.
    pub fn apply_args(&mut self, args: &Args) -> Result<(), DiffguardError> {
        if let Some(ref model) = args.model {
            self.model = model.clone();
        }
        if let Some(temp) = args.temperature {
            self.temperature = temp;
        }
        if let Some(max_tokens) = args.max_tokens {
            self.max_tokens = Some(max_tokens);
            self.provider_config.max_tokens = Some(max_tokens);
        }
        if let Some(ref provider) = args.provider {
            if *provider != self.provider {
                let new_env = standard_api_key_env_var(provider)?;
                let new_key = std::env::var(new_env).map_err(|_| {
                    DiffguardError::Config(format!(
                        "API key not found. Set {} for provider '{}'",
                        new_env, provider
                    ))
                })?;
                self.api_key = new_key;
                self.provider = provider.clone();
            }
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
        assert_eq!(default_model("deepseek"), "deepseek-v4-flash");
        assert_eq!(default_model("kimi"), "kimi-k2.5");
        assert_eq!(default_model("qwen"), "qwen-plus");
        assert_eq!(default_model("openrouter"), "openai/gpt-4o-mini");
        assert_eq!(default_model("openai"), "gpt-4o-mini");
    }
}
