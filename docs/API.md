# diffguard-rs — API Reference

Library module API documentation, key types reference, and custom provider implementation guide.

---

## Table of Contents

- [Crate Layout](#crate-layout)
- [Key Types](#key-types)
- [Module Overview](#module-overview)
- [Using as a Library](#using-as-a-library)
- [Custom Provider Implementation Guide](#custom-provider-implementation-guide)
- [Error Handling](#error-handling)

---

## Crate Layout

```text
src/
├── lib.rs           # Library root (13 public modules)
├── main.rs          # CLI entry point
├── cli.rs           # Clap argument parsing
├── config.rs        # Resolved configuration
├── diff.rs          # Diff fetching + chunking
├── error.rs         # DiffguardError enum
├── github.rs        # GitHub review submission
├── http.rs          # HTTP utilities + URL validation
├── llm/
│   ├── mod.rs       # LlmProvider trait + shared types
│   ├── deepseek.rs  # DeepSeek provider
│   ├── kimi.rs      # Kimi provider
│   ├── qwen.rs      # Qwen provider
│   ├── openrouter.rs # OpenRouter provider
│   ├── openai.rs    # OpenAI provider
│   ├── factory.rs   # Provider factory
│   └── providers.rs # Centralized provider metadata
├── output.rs        # Console output + artifacts + metrics
├── pipeline.rs      # Pipeline orchestration
├── redact.rs        # Secret redaction
├── retry.rs         # Retry logic + circuit breaker
└── verdict.rs       # Verdict parsing + review state
```

---

## Key Types

### `diff::DiffResult`

Returned by all diff-fetching functions.

```rust
pub struct DiffResult {
    pub content: String,
    pub size_bytes: usize,
    pub line_count: usize,
}
```

### `config::Config`

Resolved application configuration. Available via `config::Config::empty()` in tests only.

```rust
pub struct Config {
    pub provider: String,
    pub model: String,
    pub temperature: f32,
    pub api_key: String,
    pub github_token: Option<String>,
    pub pr_number: Option<u64>,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub prompt: String,
    pub is_ci: bool,
    pub github_base_url: String,
    pub provider_config: ProviderConfig,
    pub no_cache: bool,
}
```

### `verdict::Verdict` and `verdict::ReviewState`

```rust
pub struct Verdict {
    pub verdict: String,      // "POSITIVE" or "NEGATIVE"
    pub critical_bugs: usize,
    pub security_issues: usize,
}

pub enum ReviewState {
    Approve,
    RequestChanges,
    Comment,
}
```

### `pipeline::PipelineResult`

Exit signal from the pipeline.

```rust
pub enum PipelineResult {
    Success,       // exit 0
    ReviewBlocked, // exit 2
}
```

### `pipeline::run_pipeline()`

```rust
pub async fn run_pipeline(
    config: Config,
    diff_file: Option<&str>,
) -> anyhow::Result<PipelineResult>
```

The single entry point for all review logic. Returns a `PipelineResult` instead of calling `process::exit()`, enabling integration testing.

### `llm::LlmProvider` Trait

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &'static str;
    async fn chat_completion(
        &self,
        system_prompt: &str,
        user_message: &str,
        temperature: f32,
    ) -> Result<String, DiffguardError>;
}
```

### `llm::ProviderConfig`

Provider-specific configuration overrides.

```rust
pub struct ProviderConfig {
    pub base_url: Option<String>,
    pub http_referer: Option<String>,
    pub max_tokens: Option<u32>,
    pub model: String,
}
```

---

## Module Overview

### `verdict`

| Item | Description |
|---|---|
| `parse_verdict(response: &str)` | Extracts `[DIFFGUARD_VERDICT_METADATA]` block and returns `(Verdict, ReviewState)` |
| `Verdict` | Review verdict with bug/security counts |
| `ReviewState` | `Approve` / `RequestChanges` / `Comment` |
| `evaluate_by_tags(response: &str)` | Tag-based fallback for when metadata block is missing |

### `config`

| Item | Description |
|---|---|
| `Config` | Resolved application configuration |
| `Config::from_env(toml: Option<TomlConfig>)` | Resolves env vars with optional TOML defaults |
| `Config::apply_args(&mut self, args: &Args)` | Applies CLI overrides |
| `Config::load_prompt_file(&mut self, path: &Path)` | Loads prompt from file |
| `Config::validate_for_ci(&self)` | Validates required CI fields |
| `load_toml_config(path: &Path)` | Parses `.reviewer.toml` |
| `TomlConfig` | TOML configuration structure |
| `DEFAULT_PROMPT` | Embedded default system prompt |

### `diff`

| Item | Description |
|---|---|
| `fetch_pr_diff(base_url, owner, repo, pr, token)` | Fetches PR diff via GitHub API |
| `fetch_local_diff()` | Runs `git diff --cached` |
| `fetch_file_diff(path)` | Reads diff from a file |
| `chunk_diff(content: &str)` | Truncates large diffs to 50 head + 50 tail |
| `DiffResult` | Struct holding diff content and metadata |

### `github`

| Item | Description |
|---|---|
| `submit_review(base_url, owner, repo, pr, state, message, token)` | Submits a review via GitHub API |
| `dismiss_previous_reviews(base_url, owner, repo, pr, token)` | Dismisses previous `CHANGES_REQUESTED` reviews |

### `http`

| Item | Description |
|---|---|
| `build_github_http_client()` | Shared `reqwest::Client` builder for GitHub |
| `github_diff_headers(token)` | Standard headers for GitHub diff API |
| `validate_github_base_url(url)` | SSRF allowlist check for GitHub URLs |
| `validate_provider_base_url(url)` | SSRF allowlist check for provider URLs |

### `cache`

| Item | Description |
|---|---|
| `DiffCache` | Cache using SHA-256 keyed filenames |
| `CacheConfig` | TTL, max size, and enable/disable options |
| `CacheConfig::default()` | 24h TTL, 100 MB limit, enabled by default |
| `DiffCache::new(config)` | Creates cache instance |
| `DiffCache::get()` | Check cache by key hash |
| `DiffCache::set()` | Store response atomically |
| `DiffCache::enforce_size_limit()` | LRU cleanup if exceeded max size |
| `DiffCache::ensure_gitignored()` | Adds `.diffguard/cache/` to `.gitignore` |

### `retry`

| Item | Description |
|---|---|
| `with_retry(operation)` | Retries on transient errors with exponential backoff |
| `CircuitBreaker` | Simple Closed/Open circuit breaker |
| `CircuitBreakerConfig` | Threshold, cooldown, and enable/disable |

### `output`

| Item | Description |
|---|---|
| `print_colored_report(msg, verdict, state, writer)` | Print colored review summary |
| `print_colored_summary(msg, verdict, state, config, writer)` | Full colored summary with metrics |
| `write_artifact(msg, verdict, state, config, path)` | Write `review-result.txt` |
| `write_metrics(metrics, path)` | Write `diffguard-metrics.json` |
| `Artifact` | Struct for artifact file contents |
| `ReviewMetrics` | JSON metrics: provider, model, tokens, latency, cost, verdict, state |
| `ARTIFACT_FILENAME` | `"review-result.txt"` |
| `METRICS_FILENAME` | `"diffguard-metrics.json"` |

### `error`

| Item | Description |
|---|---|
| `DiffguardError` | Enum: `GitHubApi`, `LlmApi`, `VerdictParse`, `Config`, `Io`, `DiffTooLarge`, `EmptyDiff`, `InvalidDiffContent`, `PermissionDenied` |
| `DiffguardError::is_retryable()` | Returns true for transient errors |
| `DiffguardError::is_permission_denied()` | Returns true for 403 permission errors |

### `llm`

| Item | Description |
|---|---|
| `LlmProvider` trait | `name()` + `chat_completion()` |
| `Provider` | Type alias: `Box<dyn LlmProvider>` |
| `ProviderConfig` | Per-provider config overrides |
| `ChatMessage` | Single message with `role` and `content` |
| `ChatRequest` | Request body with `model`, `messages`, `temperature`, `max_tokens` |
| `ChatResponse` | Parsed response with `choices` vector |
| `factory::create_provider()` | Factory: `provider_name + api_key -> Provider` |
| `providers::all_providers()` | Metadata for all known providers |
| `providers::find_provider()` | Lookup provider metadata by name |
| `providers::known_provider_names()` | List of all supported provider names |

### `redact`

| Item | Description |
|---|---|
| `redact_secrets(content)` | Removes secret patterns from content |
| `log_redacted(prefix, content)` | Logs content with secrets redacted |

---

## Using as a Library

While diffguard-rs is designed as a CLI tool, internal modules are public and can be used from dependent Rust projects.

### Example: Verdict Parsing

```rust
use diffguard::verdict;

let llm_response = r#"Review of the PR:
... lots of analysis ...

[DIFFGUARD_VERDICT_METADATA]
Verdict: POSITIVE
CriticalBugs: 0
SecurityIssues: 0
"#;

let (verdict, state) = verdict::parse_verdict(llm_response).unwrap();
assert_eq!(verdict.verdict, "POSITIVE");
assert_eq!(state, verdict::ReviewState::Approve);
```

### Example: Verdict Tag Fallback

When the LLM doesn't include the structured `[DIFFGUARD_VERDICT_METADATA]` block, the parser falls back to tag counting:

```rust
let response = "Good changes!
[Critical Bug]
[Critical Bug]
[Critical Bug]";
let (verdict, state) = verdict::parse_verdict(response).unwrap();
assert_eq!(state, verdict::ReviewState::RequestChanges); // 3 critical bugs
```

### Example: Diff Chunking

```rust
use diffguard::diff::chunk_diff;

let large_diff = "line 1
line 2
...";
let (truncated, was_truncated, omitted_lines) = chunk_diff(large_diff);

if was_truncated {
    // truncated is Cow::Owned – contains truncated content
    println!("Omitted {} middle lines", omitted_lines);
} else {
    // truncated is Cow::Borrowed – zero allocation
    println!("Diff fits within limit: {} lines", truncated.len());
}
```

### Example: Error Handling

```rust
use diffguard::error::DiffguardError;

match result {
    Err(DiffguardError::DiffTooLarge { size_bytes, line_count }) => {
        // Handle large diff specifically
    }
    Err(DiffguardError::LlmApi { provider, status, message }) => {
        // Handle API errors with provider context
    }
    Err(err) if err.is_retryable() => {
        // Retry on transient errors
    }
    Err(err) => {
        // Fallback error handling
    }
}
```

---

## Custom Provider Implementation Guide

Adding a new LLM provider requires changes in four locations.

### 1. Create Provider Module (`src/llm/newprovider.rs`)

```rust
use crate::error::DiffguardError;
use crate::llm::{chat_messages, build_llm_client, send_chat_request};
use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;

#[derive(Debug)]
struct NewProviderClient {
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: Option<u32>,
    client: Client,
}

impl NewProviderClient {
    pub fn new(api_key: &str) -> Result<Self, DiffguardError> {
        // Validate API key format
        // Build reqwest client
        let client = build_llm_client("newprovider", api_key, &[])?;
        Ok(Self {
            api_key: api_key.to_string(),
            base_url: "https://api.newprovider.com/v1".to_string(),
            model: "default-model".to_string(),
            max_tokens: None,
            client,
        })
    }

    pub fn with_base_url(&mut self, url: String) -> &mut Self {
        self.base_url = url;
        self
    }

    pub fn with_model(&mut self, model: String) -> &mut Self {
        self.model = model;
        self
    }

    pub fn with_max_tokens(&mut self, max_tokens: Option<u32>) -> &mut Self {
        self.max_tokens = max_tokens;
        self
    }
}

#[async_trait]
impl crate::llm::LlmProvider for NewProviderClient {
    fn name(&self) -> &'static str {
        "newprovider"
    }

    async fn chat_completion(
        &self,
        system_prompt: &str,
        user_message: &str,
        temperature: f32,
    ) -> Result<String, DiffguardError> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": chat_messages(system_prompt, user_message),
            "temperature": temperature,
            "max_tokens": self.max_tokens,
        });
        send_chat_request(&self.client, &format!("{}/chat/completions", self.base_url), &body, "newprovider").await
    }
}
```

### 2. Register in `providers.rs`

Add to `all_providers()`:

```rust
ProviderMeta {
    name: "newprovider",
    default_base_url: "https://api.newprovider.com/v1",
    default_model: "default-model",
    api_key_env: "NEWPROVIDER_API_KEY",
    ci_allowed_hosts: &[("https", "api.newprovider.com")],
}
```

Also add the module to `src/llm/mod.rs`:

```rust
pub mod newprovider;
```

### 3. Add Factory Match Arm (`src/llm/factory.rs`)

```rust
"newprovider" => {
    let mut client = newprovider::NewProviderClient::new(api_key)?;
    if let Some(ref url) = config.base_url {
        client = client.with_base_url(url.clone());
    }
    client = client
        .with_model(config.model.clone())
        .with_max_tokens(config.max_tokens);
    Ok(Box::new(client))
}
```

### 4. Update `.reviewer.toml` Schema

Add the provider section in `docs/CONFIGURATION.md` and ensure the documentation example includes it.

### Verification Checklist

After implementing a new provider:

- [ ] Implement `LlmProvider` trait (name + chat_completion)
- [ ] Add module to `src/llm/mod.rs`
- [ ] Register provider metadata in `all_providers()` in `src/llm/providers.rs`
- [ ] Add match arm in `src/llm/factory.rs`.
- [ ] Add inline unit tests with mock response parsing
- [ ] Add integration test using wiremock
- [ ] Update `docs/PROVIDERS.md` with setup instructions
- [ ] Verify CI pass (clippy, tests, format)

---

## Error Handling

### `DiffguardError` Enum

```rust
pub enum DiffguardError {
    /// GitHub REST API error
    GitHubApi { status: u16, message: String },
    /// LLM provider error
    LlmApi { provider: String, status: u16, message: String },
    /// Failed to parse verdict metadata
    VerdictParse(String),
    /// Configuration error
    Config(String),
    /// I/O error
    Io(std::io::Error),
    /// Diff exceeds allowed size
    DiffTooLarge { size_bytes: usize, line_count: usize },
    /// Empty diff content
    EmptyDiff,
    /// Diff content is invalid (e.g. JSON instead of diff)
    InvalidDiffContent,
    /// Insufficient token permissions
    PermissionDenied { state: String, message: String },
}
```

### Helper Methods

```rust
impl DiffguardError {
    /// Returns true for transient errors (429, 5xx, connection failures)
    pub fn is_retryable(&self) -> bool { ... }
    /// Returns true for 403 permission errors
    pub fn is_permission_denied(&self) -> bool { ... }
}
```

### Best Practices

- Use `anyhow::Context` for contextual error messages in `main.rs` and `pipeline.rs`.
- Use `thiserror` derive macros for display/error conversion (already in `DiffguardError`).
- Check `is_retryable()` before deciding whether retry.
- Check `is_permission_denied()` for automatic fallback to `COMMENT` status.

---

## See Also

- [docs/ARCHITECTURE.md](ARCHITECTURE.md) — System design
- [docs/USAGE.md](USAGE.md) — Complete usage reference
- [src/lib.rs](../src/lib.rs) — Library root
- [src/llm/mod.rs](../src/llm/mod.rs) — LLM provider trait
- [src/llm/factory.rs](../src/llm/factory.rs) — Provider factory
