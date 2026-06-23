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

/// Minimum `max_tokens` for providers whose models use chain-of-thought thinking
/// by default (DeepSeek v4, Kimi).
///
/// Thinking models share the output budget between `reasoning_content` and
/// `content`. Provider docs recommend >= 16k to avoid empty final answers.
pub const THINKING_MIN_MAX_TOKENS: u32 = 16_384;

/// Default timeout for LLM HTTP requests in seconds.
///
/// Raised to 120s (from 60s) in v1.2.3 to accommodate thinking models
/// (DeepSeek, Kimi) where chain-of-thought reasoning can take substantial time
/// before the final `content` is produced.
pub const DEFAULT_LLM_TIMEOUT_SECS: u64 = 120;

/// Minimum LLM timeout (seconds) for providers that use heavy chain-of-thought
/// reasoning by default (DeepSeek V4 "pro", Kimi with thinking-on, etc.).
///
/// These models can take substantially longer to produce the final `content`
/// because they spend tokens on internal reasoning first.
pub const THINKING_MIN_LLM_TIMEOUT_SECS: u64 = 180;

/// Default system prompt embedded in the binary.
///
/// Used when no `--prompt-file` is specified or the file does not exist.
/// Customize by creating `.github/review-prompt.md` in your repository root.
///
/// The default prompt implements a **five-axis review** with a **four-level severity taxonomy**:
///
/// | Label | Meaning | Blocks merge? |
/// |---|---|---|
/// | `[Critical]` | Data loss, broken functionality, incorrect production behavior | Yes |
/// | `[Security]` | Vulnerability, unauthorized access, injection risk, exposed secret | Yes |
/// | `[Important]` | Missing test, wrong abstraction, poor error handling, significant tech debt | Conditional |
/// | `[Suggestion]` | Optional improvement: naming, style, minor optimization | No |
///
/// See [docs/USAGE.md](https://github.com/nebulaideas/rs-guard/blob/main/docs/USAGE.md#customizing-the-review-prompt)
/// for the full severity guide and project-specific prompt templates.
pub const DEFAULT_PROMPT: &str = r#"You are a Staff Engineer conducting a thorough code review. Your role is to evaluate
the proposed changes and provide actionable, categorized feedback across five dimensions.

## Approval Standard
Approve a change when it definitely improves overall code health, even if it is not perfect.
The goal is continuous improvement — do not block a change because it is not exactly how
you would have written it. If it improves the codebase and follows project conventions, approve it.

## Five Review Axes (evaluate every change across all five)

### 1. Correctness
- Does the code do what it claims to do? Does it match the spec or task requirements?
- Are edge cases handled (null, empty, boundary values, off-by-one)?
- Are error paths handled (not just the happy path)?
- Are there race conditions, state inconsistencies, or incorrect control flow?

### 2. Security
- Is user input validated and sanitized at system boundaries?
- Are secrets kept out of code, logs, and version control?
- Is authentication/authorization checked where needed?
- Are queries parameterized? Is output encoded to prevent injection?
- Are dependencies from trusted sources with no known vulnerabilities?
- Is data from external sources treated as untrusted?

### 3. Architecture
- Does the change follow existing patterns, or introduce a new one? If new, is it justified?
- Are module boundaries maintained? Any circular dependencies or unwanted coupling?
- Is there code duplication that should be shared?
- Is the abstraction level appropriate — not over-engineered, not too coupled?

### 4. Readability & Simplicity
- Can another engineer understand this code without the author explaining it?
- Are names descriptive and consistent with project conventions?
- Is the control flow straightforward (avoid deeply nested logic)?
- Is there dead code, no-op variables, or over-complicated logic that could be simplified?
- Are abstractions earning their complexity?

### 5. Performance
- Any N+1 query patterns or unbounded loops?
- Any synchronous operations that should be async?
- Any unconstrained data fetching or missing pagination?
- Any large objects created in hot paths?

## Severity Taxonomy
Label every finding with its severity:

- `[Critical]` — Must fix before merge: data loss risk, broken functionality, incorrect behavior in production
- `[Security]` — Must fix before merge: vulnerability, unauthorized access, injection risk, exposed secret
- `[Important]` — Should fix before merge: missing test, wrong abstraction, poor error handling, significant tech debt
- `[Suggestion]` — Optional improvement: naming, style, minor optimization (author may ignore)

## Output Format

### Critical Issues
List each `[Critical]` finding with file/location, description, and a concrete fix recommendation.

### Security Issues
List each `[Security]` finding with file/location, description, and a concrete fix recommendation.

### Important Issues
List each `[Important]` finding with file/location and description.

### Suggestions
List each `[Suggestion]` briefly.

### What's Done Well
Always include at least one specific positive observation. Specific praise motivates good practices.

## Verdict Guidelines
- **POSITIVE** if the diff improves overall code health and is ready to merge
- **NEGATIVE** if there are `[Critical]` or `[Security]` findings that must block merging

At the end of your response, include exactly this metadata block (do not modify the format):

[RS_GUARD_VERDICT_METADATA]
Verdict: POSITIVE or NEGATIVE
CriticalIssues: <count>
SecurityIssues: <count>
ImportantIssues: <count>
Suggestions: <count>
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
    /// Provider-specific model variant override.
    pub variant: Option<String>,
    /// Optional `result_format` override for this provider.
    ///
    /// Overrides the static default (e.g. Qwen's `"message"`) when set.
    pub result_format: Option<String>,
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
    /// LLM request timeout in seconds (total time for the HTTP call).
    pub llm_timeout_secs: Option<u64>,
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
    /// Provider-specific model variant (e.g. "flash", "thinking-on").
    pub variant: Option<String>,
    /// Custom cache directory path (default: git-root/.rs-guard/cache or cwd/.rs-guard/cache).
    pub cache_dir: Option<String>,
    /// Circuit breaker configuration.
    pub circuit_breaker: Option<CircuitBreakerTomlConfig>,
    /// Per-provider pricing overrides.
    pub pricing: Option<HashMap<String, PricingTomlConfig>>,
    /// Whether to automatically add the cache directory to `.gitignore`.
    pub auto_gitignore: Option<bool>,
}

/// Known top-level keys in `.reviewer.toml`, used to detect typos and
/// provide suggestions when an unknown key is encountered.
const KNOWN_TOP_LEVEL_KEYS: &[&str] = &[
    "provider",
    "model",
    "variant",
    "temperature",
    "max_tokens",
    "llm_timeout_secs",
    "chunk_head_lines",
    "chunk_tail_lines",
    "providers",
    "cache_dir",
    "circuit_breaker",
    "pricing",
    "auto_gitignore",
];

/// Returns the closest known top-level key to `unknown`, or `None` if no
/// reasonable match exists. Uses a simple Levenshtein distance threshold.
fn suggest_key(unknown: &str) -> Option<&'static str> {
    let mut best: Option<(&str, usize)> = None;
    for known in KNOWN_TOP_LEVEL_KEYS {
        let dist = levenshtein_distance(unknown, known);
        if dist == 0 {
            return Some(known);
        }
        if dist <= 3 {
            best = Some(match best {
                Some((_, best_dist)) if best_dist <= dist => best.unwrap(),
                _ => (known, dist),
            });
        }
    }
    best.map(|(k, _)| k)
}

/// Simple Levenshtein distance between two strings.
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        for (j, b_char) in b_chars.iter().enumerate().take(b_len) {
            let cost = if a_chars[i - 1] == *b_char { 0 } else { 1 };
            curr[j + 1] = (curr[j] + 1).min(prev[j + 1] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

/// Validates common TOML mistakes and returns a human-friendly error message.
///
/// Checks for:
/// - `provider` declared as a table (`[provider.X]`) instead of a string.
/// - Unknown top-level keys with suggestions.
/// - `provider` present but not a string.
fn validate_toml_value(raw: &toml::Value, path: &Path) -> Result<(), RsGuardError> {
    let table = raw.as_table().ok_or_else(|| {
        RsGuardError::Config(format!(
            "In '{}': config file must be a TOML table (e.g., key = value)",
            path.display()
        ))
    })?;

    // Detect the common [provider.X] (singular) mistake.
    if let Some(provider) = table.get("provider") {
        if provider.is_table() {
            return Err(RsGuardError::Config(format!(
                "In '{}': `[provider.deepseek]` is the singular form and is not valid.\n\n\
                 `provider` must be a string, not a table.\n\n\
                 Did you mean to write:\n\
                   provider = \"deepseek\"\n\n\
                 For per-provider overrides, use the plural table name:\n\
                   [providers.deepseek]\n\
                   api_key_env = \"DEEPSEEK_API_KEY\"\n",
                path.display()
            )));
        }
        if !provider.is_str() {
            return Err(RsGuardError::Config(format!(
                "In '{}':\n\n\
                 `provider` must be a string (e.g., provider = \"deepseek\").\n\
                 Got a non-string value for `provider`.\n",
                path.display()
            )));
        }
    }

    // Detect unknown top-level keys and suggest the closest valid key.
    for key in table.keys() {
        if !KNOWN_TOP_LEVEL_KEYS.contains(&key.as_str()) {
            let suggestion = suggest_key(key);
            let hint = suggestion.map_or_else(
                || String::from("See the documentation for valid configuration keys."),
                |s| format!("Did you mean `{}`?", s),
            );
            return Err(RsGuardError::Config(format!(
                "In '{}':\n\n\
                 Unknown key `{}` at the top level.\n\
                 {}\n\n\
                 Valid top-level keys: {}\n",
                path.display(),
                key,
                hint,
                KNOWN_TOP_LEVEL_KEYS.join(", ")
            )));
        }
    }

    Ok(())
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

    // Parse to a generic value first so we can inspect the structure and emit
    // friendly error messages for common mistakes.
    let raw: toml::Value = toml::from_str(&content).map_err(|e| {
        RsGuardError::Config(format!(
            "Failed to parse config file '{}': {}\n\n\
             See docs/CONFIGURATION.md for the full configuration reference.",
            path.display(),
            e
        ))
    })?;

    validate_toml_value(&raw, path)?;

    let config: TomlConfig = TomlConfig::deserialize(raw).map_err(|e| {
        RsGuardError::Config(format!(
            "Failed to parse config file '{}': {}\n\n\
             See docs/CONFIGURATION.md for the full configuration reference.",
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
    /// Provider-specific model variant (e.g. "flash", "thinking-on").
    pub variant: Option<String>,
    /// Top-level variant from TOML, retained for provider-switch re-resolution.
    top_level_variant: Option<String>,
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
    /// Resolved LLM request timeout in seconds.
    pub llm_timeout_secs: u64,
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
            variant: None,
            top_level_variant: None,
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
            llm_timeout_secs: DEFAULT_LLM_TIMEOUT_SECS,
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
                if parts.len() != 2 {
                    return Err(RsGuardError::Config(format!(
                        "REPO_FULL_NAME must be in 'owner/repo' format, got: '{}'",
                        full
                    )));
                }
                let owner = parts[0];
                let repo = parts[1];

                // Validate non-empty
                if owner.is_empty() || repo.is_empty() {
                    return Err(RsGuardError::Config(format!(
                        "REPO_FULL_NAME owner and repo cannot be empty, got: '{}'",
                        full
                    )));
                }

                // Validate no additional slashes in either part
                if owner.contains('/') || repo.contains('/') {
                    return Err(RsGuardError::Config(format!(
                        "REPO_FULL_NAME must be in 'owner/repo' format (no additional slashes), got: '{}'",
                        full
                    )));
                }

                (Some(owner.to_string()), Some(repo.to_string()))
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
        let env_max_tokens = std::env::var("RS_GUARD_MAX_TOKENS")
            .ok()
            .and_then(|s| s.parse().ok());
        let toml_max_tokens = toml.as_ref().and_then(|t| t.max_tokens);
        let max_tokens_is_explicit = env_max_tokens.is_some() || toml_max_tokens.is_some();

        let mut max_tokens: Option<u32> = env_max_tokens
            .or(toml_max_tokens)
            .or(Some(DEFAULT_MAX_TOKENS));

        // Thinking models (DeepSeek v4, Kimi) share max_tokens between
        // reasoning_content and content. Raise the floor when the user has not
        // set an explicit value — prevents 0-char content on deepseek-v4-pro.
        if !max_tokens_is_explicit && matches!(provider.as_str(), "deepseek" | "kimi") {
            max_tokens = max_tokens.map(|t| t.max(THINKING_MIN_MAX_TOKENS));
        }

        // LLM timeout (seconds): env > toml > DEFAULT_LLM_TIMEOUT_SECS (120)
        // Thinking models (deepseek-v4-pro, Kimi thinking) can take much longer
        // to emit final content after spending the budget on reasoning_content.
        let env_timeout = std::env::var("RS_GUARD_LLM_TIMEOUT")
            .ok()
            .and_then(|s| s.parse().ok());
        let toml_timeout = toml.as_ref().and_then(|t| t.llm_timeout_secs);
        let timeout_is_explicit = env_timeout.is_some() || toml_timeout.is_some();

        let mut llm_timeout_secs = env_timeout
            .or(toml_timeout)
            .unwrap_or(DEFAULT_LLM_TIMEOUT_SECS);

        if !timeout_is_explicit && matches!(provider.as_str(), "deepseek" | "kimi") {
            llm_timeout_secs = llm_timeout_secs.max(THINKING_MIN_LLM_TIMEOUT_SECS);
        }

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

        // Variant: env > toml per-provider > toml top-level
        let env_variant = std::env::var("RS_GUARD_VARIANT").ok();
        let toml_provider_variant = toml_provider.and_then(|p| p.variant.clone());
        let top_level_variant = toml.as_ref().and_then(|t| t.variant.clone());
        let variant = env_variant
            .clone()
            .or(toml_provider_variant.clone())
            .or(top_level_variant.clone());

        let provider_config = ProviderConfig {
            base_url,
            http_referer: toml_provider.and_then(|p| p.http_referer.clone()),
            max_tokens,
            model: model.clone(),
            variant: variant.clone(),
            result_format: toml_provider.and_then(|p| p.result_format.clone()),
            timeout_secs: Some(llm_timeout_secs),
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
            variant,
            top_level_variant,
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
            llm_timeout_secs,
        })
    }

    /// Applies CLI argument overrides to the configuration.
    ///
    /// CLI flags take precedence over environment variables and TOML for `model`,
    /// `variant`, `temperature`, `provider`, `max_tokens`, and `llm_timeout`. If the provider
    /// changes, the API key is re-resolved (respecting TOML `api_key_env`
    /// overrides), the model is reset to the new provider's default unless
    /// explicitly set via the CLI `--model` flag, and the variant is re-resolved
    /// from env/TOML unless explicitly set via the CLI `--variant` flag.
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
                self.provider_config.result_format =
                    toml_provider.and_then(|p| p.result_format.clone());

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

                // Re-resolve variant for the new provider unless CLI --variant was used.
                if args.variant.is_none() {
                    let env_variant = std::env::var("RS_GUARD_VARIANT").ok();
                    let provider_variant = self
                        .toml_providers
                        .get(provider)
                        .and_then(|p| p.variant.clone());
                    self.variant = env_variant
                        .or(provider_variant)
                        .or(self.top_level_variant.clone());
                    self.provider_config.variant = self.variant.clone();
                }
            }
        }

        if let Some(ref model) = args.model {
            self.model = model.clone();
            self.provider_config.model = model.clone();
            self.model_set_via_cli = true;
        }
        if let Some(ref variant) = args.variant {
            self.variant = Some(variant.clone());
            self.provider_config.variant = Some(variant.clone());
        }
        if let Some(temp) = args.temperature {
            self.temperature = temp;
        }
        if let Some(max_tokens) = args.max_tokens {
            self.provider_config.max_tokens = Some(max_tokens);
        }
        if let Some(t) = args.llm_timeout {
            self.llm_timeout_secs = t;
            self.provider_config.timeout_secs = Some(t);
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
    use std::sync::Mutex;

    /// Serializes tests that mutate process-global environment variables.
    /// Rust tests run in parallel threads by default; without this guard, tests
    /// that call `set_var` / `remove_var` on the same key race with each other.
    ///
    /// Scope: unit tests in this module only. Integration tests in `tests/`
    /// compile into a separate binary and cannot race with these; they use
    /// `serial_test::serial` for their own isolation.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

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
                variant: None,
                result_format: None,
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
    fn test_provider_config_result_format_override_preserved() {
        // ProviderConfig must carry a per-provider result_format override so
        // the factory can pass it to the generic client (issue #77).
        let mut providers = HashMap::new();
        providers.insert(
            "qwen".to_string(),
            ProviderTomlConfig {
                api_key_env: None,
                base_url: None,
                http_referer: None,
                variant: None,
                result_format: Some("json_object".to_string()),
            },
        );

        let toml = TomlConfig {
            provider: Some("qwen".to_string()),
            providers: Some(providers),
            ..Default::default()
        };

        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("DASHSCOPE_API_KEY", "test-key");
        let config = Config::from_env(Some(toml)).unwrap();
        std::env::remove_var("DASHSCOPE_API_KEY");

        assert_eq!(
            config.provider_config.result_format,
            Some("json_object".to_string())
        );
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

    #[test]
    fn test_repo_full_name_with_multiple_slashes() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("DEEPSEEK_API_KEY", "test-key");
        std::env::set_var("REPO_FULL_NAME", "owner/repo/subpath");
        let result = Config::from_env(None);
        std::env::remove_var("DEEPSEEK_API_KEY");
        std::env::remove_var("REPO_FULL_NAME");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no additional slashes"));
    }

    #[test]
    fn test_repo_full_name_empty_owner() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("DEEPSEEK_API_KEY", "test-key");
        std::env::set_var("REPO_FULL_NAME", "/repo");
        let result = Config::from_env(None);
        std::env::remove_var("DEEPSEEK_API_KEY");
        std::env::remove_var("REPO_FULL_NAME");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[test]
    fn test_repo_full_name_empty_repo() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("DEEPSEEK_API_KEY", "test-key");
        std::env::set_var("REPO_FULL_NAME", "owner/");
        let result = Config::from_env(None);
        std::env::remove_var("DEEPSEEK_API_KEY");
        std::env::remove_var("REPO_FULL_NAME");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[test]
    fn test_repo_full_name_valid_format() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("DEEPSEEK_API_KEY", "test-key");
        std::env::set_var("REPO_FULL_NAME", "owner/repo");
        let result = Config::from_env(None);
        std::env::remove_var("DEEPSEEK_API_KEY");
        std::env::remove_var("REPO_FULL_NAME");
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.repo_owner, Some("owner".to_string()));
        assert_eq!(config.repo_name, Some("repo".to_string()));
    }

    #[test]
    fn test_invalid_temperature_env_var_errors() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("DEEPSEEK_API_KEY", "test-key");
        std::env::set_var("RS_GUARD_TEMPERATURE", "not-a-number");
        let result = Config::from_env(None);
        std::env::remove_var("DEEPSEEK_API_KEY");
        std::env::remove_var("RS_GUARD_TEMPERATURE");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid RS_GUARD_TEMPERATURE"));
    }

    #[test]
    fn test_temperature_out_of_range_errors() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("DEEPSEEK_API_KEY", "test-key");
        std::env::set_var("RS_GUARD_TEMPERATURE", "3.0");
        let result = Config::from_env(None);
        std::env::remove_var("DEEPSEEK_API_KEY");
        std::env::remove_var("RS_GUARD_TEMPERATURE");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("between 0.0 and 2.0"));
    }

    #[test]
    fn test_thinking_model_max_tokens_floor() {
        // When no explicit max_tokens is set, DeepSeek/Kimi get a raised floor.
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("DEEPSEEK_API_KEY", "test-key");
        let result = Config::from_env(None).unwrap();
        std::env::remove_var("DEEPSEEK_API_KEY");
        assert_eq!(
            result.provider_config.max_tokens,
            Some(THINKING_MIN_MAX_TOKENS)
        );
    }

    #[test]
    fn test_explicit_max_tokens_overrides_thinking_floor() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("DEEPSEEK_API_KEY", "test-key");
        std::env::set_var("RS_GUARD_MAX_TOKENS", "1024");
        let result = Config::from_env(None).unwrap();
        std::env::remove_var("DEEPSEEK_API_KEY");
        std::env::remove_var("RS_GUARD_MAX_TOKENS");
        assert_eq!(result.provider_config.max_tokens, Some(1024));
    }

    #[test]
    fn test_validate_toml_unknown_key_suggests_closest() {
        let raw: toml::Value = toml::from_str("providor = \"deepseek\"").unwrap();
        let result = validate_toml_value(&raw, std::path::Path::new(".reviewer.toml"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Unknown key `providor`"));
        assert!(msg.contains("Did you mean `provider`?"));
    }

    #[test]
    fn test_validate_toml_provider_as_table_errors() {
        let raw: toml::Value = toml::from_str("[provider.deepseek]\napi_key_env = \"X\"").unwrap();
        let result = validate_toml_value(&raw, std::path::Path::new(".reviewer.toml"));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("`[provider.deepseek]` is the singular form"));
    }

    #[test]
    fn test_load_toml_config_invalid_toml_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".reviewer.toml");
        std::fs::write(&path, "not valid toml [[").unwrap();
        let result = load_toml_config(&path);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse config file"));
    }

    #[test]
    fn test_apply_args_switches_provider_and_re_resolves_variant() {
        use clap::Parser;

        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("DEEPSEEK_API_KEY", "ds-key");
        std::env::set_var("KIMI_API_KEY", "kimi-key");

        let toml = TomlConfig {
            provider: Some("deepseek".to_string()),
            providers: Some({
                let mut map = HashMap::new();
                map.insert(
                    "kimi".to_string(),
                    ProviderTomlConfig {
                        api_key_env: None,
                        base_url: None,
                        http_referer: None,
                        variant: Some("thinking-on".to_string()),
                        result_format: None,
                    },
                );
                map
            }),
            ..Default::default()
        };

        let mut config = Config::from_env(Some(toml)).unwrap();
        assert_eq!(config.provider, "deepseek");
        assert!(config.variant.is_none());

        let args = crate::cli::Args::parse_from(["rs-guard", "--provider", "kimi"]);
        config.apply_args(&args).unwrap();

        assert_eq!(config.provider, "kimi");
        assert_eq!(config.api_key, "kimi-key");
        assert_eq!(config.variant, Some("thinking-on".to_string()));

        std::env::remove_var("DEEPSEEK_API_KEY");
        std::env::remove_var("KIMI_API_KEY");
    }

    #[test]
    fn test_pricing_override_from_toml() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("DEEPSEEK_API_KEY", "test-key");

        let toml = TomlConfig {
            provider: Some("deepseek".to_string()),
            pricing: Some({
                let mut map = HashMap::new();
                map.insert(
                    "deepseek".to_string(),
                    PricingTomlConfig {
                        input_per_million: 10,
                        output_per_million: 50,
                    },
                );
                map
            }),
            ..Default::default()
        };

        let config = Config::from_env(Some(toml)).unwrap();
        std::env::remove_var("DEEPSEEK_API_KEY");
        assert!(config.pricing.is_some());
        let pricing = config.pricing.as_ref().unwrap();
        assert_eq!(pricing.get("deepseek").unwrap().input_per_million, 10);
    }
}
