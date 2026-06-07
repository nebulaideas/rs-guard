//! Configuration resolution from environment variables and CLI arguments.
//!
//! Handles detection of CI vs local mode, API key resolution, and
//! validation of required fields for GitHub PR review submission.

use crate::cli::Args;
use crate::error::DiffguardError;
use crate::http::validate_github_base_url;
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

/// Returns the environment variable name for the API key of the given provider.
fn api_key_env_var(provider: &str) -> &'static str {
    match provider {
        "deepseek" => "DEEPSEEK_API_KEY",
        _ => "DEEPSEEK_API_KEY",
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
}

impl Config {
    /// Builds configuration from environment variables.
    ///
    /// Detects CI mode via the `GITHUB_ACTIONS` environment variable and
    /// resolves the appropriate API key based on the selected provider.
    ///
    /// # Errors
    ///
    /// Returns [`DiffguardError::Config`] if the required API key is not set.
    pub fn from_env() -> Result<Self, DiffguardError> {
        let is_ci = std::env::var("GITHUB_ACTIONS").is_ok();

        let provider =
            std::env::var("DIFFGUARD_PROVIDER").unwrap_or_else(|_| "deepseek".to_string());

        let api_key_env = api_key_env_var(&provider);

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

        Ok(Config {
            provider,
            model: std::env::var("DIFFGUARD_MODEL")
                .unwrap_or_else(|_| "deepseek-v4-flash".to_string()),
            temperature: std::env::var("DIFFGUARD_TEMPERATURE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.1),
            api_key,
            github_token,
            pr_number,
            repo_owner,
            repo_name,
            prompt: DEFAULT_PROMPT.to_string(),
            is_ci,
            github_base_url,
        })
    }

    /// Applies CLI argument overrides to the configuration.
    ///
    /// CLI flags take precedence over environment variables for `model`,
    /// `temperature`, and `provider`. If the provider changes, the API key
    /// is re-resolved from the corresponding environment variable.
    ///
    /// # Errors
    ///
    /// Returns [`DiffguardError::Config`] if the provider changes and the
    /// new provider's API key environment variable is not set.
    pub fn apply_args(&mut self, args: &Args) -> Result<(), DiffguardError> {
        self.model = args.model.clone();
        self.temperature = args.temperature;

        if args.provider != self.provider {
            let new_env = api_key_env_var(&args.provider);
            let new_key = std::env::var(new_env).map_err(|_| {
                DiffguardError::Config(format!(
                    "API key not found. Set {} for provider '{}'",
                    new_env, args.provider
                ))
            })?;
            self.api_key = new_key;
            self.provider = args.provider.clone();
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
