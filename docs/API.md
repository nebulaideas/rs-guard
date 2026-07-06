# rs-guard — API Reference

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
├── lib.rs           # Library root (14 public modules)
├── main.rs          # CLI entry point
├── cli.rs           # Clap argument parsing
├── config.rs        # Resolved configuration
├── diff.rs          # Diff fetching + chunking
├── error.rs         # RsGuardError enum
├── github.rs        # GitHub review submission
├── http.rs          # HTTP utilities + URL validation
├── llm/
│   ├── mod.rs           # LlmProvider trait + shared types
│   ├── generic_client.rs # GenericOpenAiCompatibleClient — serves all providers
│   ├── factory.rs       # Provider factory
│   └── providers.rs     # Centralized ProviderMeta metadata (one entry per provider)
│                        # All providers are served by a single generic_client
│                        # instance; adding a provider is a metadata entry here.
├── output.rs        # Console output + artifacts + metrics
├── pipeline.rs      # Pipeline orchestration
├── redact.rs        # Secret redaction
├── retry.rs         # Retry logic + circuit breaker
├── rules.rs         # Project rules detection and loading
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
    pub dry_run: bool,
    pub cache_dir: Option<String>,
    pub circuit_breaker: Option<CircuitBreaker>,
    pub pricing: Option<HashMap<String, PricingTomlConfig>>,
    pub auto_gitignore: bool,
    pub chunk_head_lines: usize,
    pub chunk_tail_lines: usize,
}
```

### `verdict::Verdict` and `verdict::ReviewState`

```rust
pub struct Verdict {
    pub verdict: String,      // "POSITIVE" or "NEGATIVE"
    pub critical_bugs: u32,
    pub security_issues: u32,
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
    ) -> Result<String, RsGuardError>;
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

| Item                               | Description                                                                       |
| ---------------------------------- | --------------------------------------------------------------------------------- |
| `parse_verdict(response: &str, important_threshold: u32)` | Extracts `[RS_GUARD_VERDICT_METADATA]` block and returns `(Verdict, ReviewState)` |
| `Verdict`                          | Review verdict with bug/security counts                                           |
| `ReviewState`                      | `Approve` / `RequestChanges` / `Comment`                                          |
| `evaluate_by_tags(response: &str)` | Tag-based fallback for when metadata block is missing                             |
| `determine_review_state(verdict: &Verdict, important_threshold: u32)` | Maps a parsed verdict to a GitHub review state            |

### `config`

| Item                                               | Description                                   |
| -------------------------------------------------- | --------------------------------------------- |
| `Config`                                           | Resolved application configuration            |
| `Config::from_env(toml: Option<TomlConfig>)`       | Resolves env vars with optional TOML defaults |
| `Config::apply_args(&mut self, args: &ReviewArgs)` | Applies CLI overrides                         |
| `Config::load_prompt_file(&mut self, path: &Path)` | Loads prompt from file                        |
| `Config::validate_for_ci(&self)`                   | Validates required CI fields                  |
| `load_toml_config(path: &Path)`                    | Parses `.reviewer.toml`                       |
| `TomlConfig`                                       | TOML configuration structure                  |
| `DEFAULT_PROMPT`                                   | Embedded default system prompt                |

### `diff`

| Item                                              | Description                                |
| ------------------------------------------------- | ------------------------------------------ |
| `fetch_pr_diff(base_url, owner, repo, pr, token)` | Fetches PR diff via GitHub API             |
| `fetch_local_diff()`                              | Runs `git diff --cached`                   |
| `fetch_file_diff(path)`                           | Reads diff from a file                     |
| `chunk_diff(content: &str)`                       | Truncates large diffs to 400 head + 400 tail |
| `DiffResult`                                      | Struct holding diff content and metadata   |

### `github`

| Item                                                              | Description                                    |
| ----------------------------------------------------------------- | ---------------------------------------------- |
| `submit_review(base_url, owner, repo, pr, state, message, token)` | Submits a review via GitHub API                |
| `dismiss_previous_reviews(base_url, owner, repo, pr, token)`      | Dismisses previous `CHANGES_REQUESTED` reviews |

### `http`

| Item                              | Description                                 |
| --------------------------------- | ------------------------------------------- |
| `build_github_http_client()`      | Shared `reqwest::Client` builder for GitHub |
| `github_diff_headers(token)`      | Standard headers for GitHub diff API        |
| `validate_github_base_url(url)`   | SSRF allowlist check for GitHub URLs        |
| `validate_provider_base_url(url)` | SSRF allowlist check for provider URLs      |

### `cache`

| Item                              | Description                               |
| --------------------------------- | ----------------------------------------- |
| `DiffCache`                       | Cache using SHA-256 keyed filenames       |
| `CacheConfig`                     | TTL, max size, and enable/disable options |
| `CacheConfig::default()`          | 24h TTL, 100 MB limit, enabled by default |
| `DiffCache::new(config)`          | Creates cache instance                    |
| `DiffCache::get()`                | Check cache by key hash                   |
| `DiffCache::set()`                | Store response atomically                 |
| `DiffCache::enforce_size_limit()` | LRU cleanup if exceeded max size          |
| `DiffCache::ensure_gitignored()`  | Adds `.rs-guard/cache/` to `.gitignore` (returns `Result`, controlled by `auto_gitignore`) |

### `retry`

| Item                                                      | Description                                          |
| --------------------------------------------------------- | ---------------------------------------------------- |
| `with_retry(operation, circuit: Option<&CircuitBreaker>)` | Retries on transient errors with exponential backoff |
| `CircuitBreaker`                                          | Simple Closed/Open circuit breaker                   |
| `CircuitBreakerConfig`                                    | Threshold, cooldown, and enable/disable              |

### `output`

| Item                                                         | Description                                                          |
| ------------------------------------------------------------ | -------------------------------------------------------------------- |
| `print_colored_report(msg, verdict, state, writer)`          | Print colored review summary                                         |
| `print_colored_summary(msg, verdict, state, config, writer)` | Full colored summary with metrics                                    |
| `write_artifact(msg, verdict, state, config, path)`          | Write `review-result.txt`                                            |
| `write_metrics(metrics, path)`                               | Write `rs-guard-metrics.json`                                        |
| `Artifact`                                                   | Struct for artifact file contents                                    |
| `ReviewMetrics`                                              | JSON metrics: provider, model, tokens, latency, cost, verdict, state |
| `ARTIFACT_FILENAME`                                          | `"review-result.txt"`                                                |
| `METRICS_FILENAME`                                           | `"rs-guard-metrics.json"`                                            |

### `error`

| Item                                   | Description                                                                                                                        |
| -------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `RsGuardError`                         | Enum: `GitHubApi`, `LlmApi`, `VerdictParse`, `Config`, `Io`, `DiffTooLarge`, `EmptyDiff`, `InvalidDiffContent`, `PermissionDenied` |
| `RsGuardError::is_retryable()`         | Returns true for transient errors                                                                                                  |
| `RsGuardError::is_permission_denied()` | Returns true for 403 permission errors                                                                                             |

### `llm`

| Item                                | Description                                                        |
| ----------------------------------- | ------------------------------------------------------------------ |
| `LlmProvider` trait                 | `name()` + `chat_completion()`                                     |
| `Provider`                          | Type alias: `Box<dyn LlmProvider>`                                 |
| `ProviderConfig`                    | Per-provider config overrides                                      |
| `ChatMessage`                       | Single message with `role` and `content`                           |
| `ChatRequest`                       | Request body with `model`, `messages`, `temperature`, `max_tokens`, `extra_body` (for VariantEffect) |
| `ChatResponse` / `ChatChoice` / `ChatMessageResponse` | Document the expected OpenAI-compatible shape. **Runtime parsing uses a loose `serde_json::Value` traversal** (in `parse_completion_response_body`) to tolerate `"content": null`, multimodal arrays, and extra fields from thinking models. |
| `factory::create_provider()`        | Factory: `provider_name + api_key -> Provider`                     |
| `providers::all_providers()`        | Metadata for all known providers                                   |
| `providers::find_provider()`               | Lookup provider metadata by name                                   |
| `providers::get_provider_context_window()` | Returns context window size for a provider                         |
| `providers::known_provider_names()`        | List of all supported provider names                               |

### `redact`

| Item                            | Description                          |
| ------------------------------- | ------------------------------------ |
| `redact_secrets(content)`       | Removes secret patterns from content |
| `log_redacted(prefix, content)` | Logs content with secrets redacted   |

### `rules`

Project rules detection and loading. Detects AI-agent instruction files in the repository root, loads the selected file, and applies a 32 KB soft cap with a truncation warning banner.

| Item | Description |
| ---- | ----------- |
| `detect_project_rules(repo_root)` | Detects and loads the highest-priority rules file. Returns `None` if no file is found. |
| `detect_all_rules_files(repo_root)` | Returns all matching rules file paths in priority order. |
| `load_rules_file(path)` | Loads a specific rules file with the default soft cap. |
| `select_rules_file(files, is_tty, select_fn)` | Selects a rules file from a list. Uses `select_fn` in TTY mode, falls back to the first file otherwise. |
| `should_show_picker(is_ci, file_count, rules_file, no_project_rules, is_tty)` | Pure predicate for whether to show the interactive picker. |
| `RulesDetector` | Builder-driven detector with configurable repo root and soft cap. |
| `RulesDetectorBuilder` | Builder for `RulesDetector`. |
| `DetectedRules` | Loaded rules content plus metadata (path, size, truncation flag). |
| `DEFAULT_RULES_CAP_BYTES` | 32 KB soft cap. |

---

## Using as a Library

While rs-guard is designed as a CLI tool, internal modules are public and can be used from dependent Rust projects.

### Example: Verdict Parsing

```rust
use rs_guard::verdict;

let llm_response = r#"Review of the PR:
... lots of analysis ...

[RS_GUARD_VERDICT_METADATA]
Verdict: POSITIVE
CriticalBugs: 0
SecurityIssues: 0
"#;

let (verdict, state) = verdict::parse_verdict(llm_response, 3).unwrap();
assert_eq!(verdict.verdict, "POSITIVE");
assert_eq!(state, verdict::ReviewState::Approve);
```

### Example: Verdict Tag Fallback

When the LLM doesn't include the structured `[RS_GUARD_VERDICT_METADATA]` block, the parser falls back to tag counting:

```rust
let response = "Good changes!
[Critical Bug]
[Critical Bug]
[Critical Bug]";
let (verdict, state) = verdict::parse_verdict(response, 3).unwrap();
assert_eq!(state, verdict::ReviewState::RequestChanges); // 3 critical bugs
```

### Example: Diff Chunking

```rust
use rs_guard::diff::chunk_diff;

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
use rs_guard::error::DiffguardError;

match result {
    Err(RsGuardError::DiffTooLarge { size_bytes, line_count }) => {
        // Handle large diff specifically
    }
    Err(RsGuardError::LlmApi { provider, status, message }) => {
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

> **Note on crate-internal functions:** The helper functions `build_llm_client()`, `chat_messages()`, and `send_chat_request()` used in the example below are all `pub(crate)` — they are only accessible from within the `rs-guard` crate itself. External provider implementations (e.g., libraries that depend on `rs-guard`) must use the public [`LlmProvider`] trait directly and implement their own HTTP client logic, message construction, and request handling. The guide below shows the pattern as it exists inside the crate for maintainers adding first-party providers.

With the generic-client refactor, all OpenAI-compatible providers are served by a single internal `GenericOpenAiCompatibleClient`. Adding a new provider is now a **metadata entry** in `src/llm/providers.rs` rather than a new module.

### 1. Add a `ProviderMeta` entry in `src/llm/providers.rs`

Append to `all_providers()`:

```rust
ProviderMeta {
    name: "newprovider",
    default_base_url: "https://api.newprovider.com/v1",
    default_model: "default-model",
    api_key_env: "NEWPROVIDER_API_KEY",
    ci_allowed_hosts: &[("https", "api.newprovider.com")],
    context_window: 128_000,
    variants: &[],
    result_format: None,  // Or Some(Cow::Borrowed("message")) for Qwen/DashScope
    default_extra_headers: &[],  // Add default headers if needed (e.g. OpenRouter attribution)
}
```

The `factory.rs` module resolves the provider name to a `ProviderMeta` and constructs a `GenericOpenAiCompatibleClient` parameterized by that metadata — no new module or match arm is required.

**Field explanations:**
- `variants`: Provider-specific model variants (e.g. DeepSeek's `flash`/`pro`, Kimi's `thinking-on`/`thinking-off`). Leave empty if your provider has no variants.
- `result_format`: Uses `Option<Cow<'static, str>>` so known providers keep a zero-cost borrowed value. Set to `Some(Cow::Borrowed("message"))` when the provider requires it (Qwen/DashScope); otherwise `None`. Per-provider TOML overrides (`[providers.<name>].result_format`) take precedence over this static default at runtime.
- `default_extra_headers`: Default HTTP headers sent with every request. Use for provider-specific attribution (e.g. OpenRouter's `HTTP-Referer` and `X-Title`). Most providers don't need this.

### 2. Update `.reviewer.toml` Schema

Add the provider section in `docs/CONFIGURATION.md` and ensure the documentation example includes it.

### Verification Checklist

After adding a new provider:

- [ ] Register provider metadata in `all_providers()` in `src/llm/providers.rs`
- [ ] Add integration test using wiremock against the new `ProviderMeta`
- [ ] Update `docs/PROVIDERS.md` with setup instructions
- [ ] Update `docs/CONFIGURATION.md` `.reviewer.toml` example
- [ ] Verify CI pass (clippy, tests, format)

---

## Error Handling

### `RsGuardError` Enum

```rust
pub enum RsGuardError {
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
impl RsGuardError {
    /// Returns true for transient errors (429, 5xx, connection failures)
    pub fn is_retryable(&self) -> bool { ... }
    /// Returns true for 403 permission errors
    pub fn is_permission_denied(&self) -> bool { ... }
}
```

### Best Practices

- Use `anyhow::Context` for contextual error messages in `main.rs` and `pipeline.rs`.
- Use `thiserror` derive macros for display/error conversion (already in `RsGuardError`).
- Check `is_retryable()` before deciding whether retry.
- Check `is_permission_denied()` for automatic fallback to `COMMENT` status.

---

## See Also

- [docs/ARCHITECTURE.md](ARCHITECTURE.md) — System design
- [docs/USAGE.md](USAGE.md) — Complete usage reference
- [src/lib.rs](../src/lib.rs) — Library root
- [src/llm/mod.rs](../src/llm/mod.rs) — LLM provider trait
- [src/llm/factory.rs](../src/llm/factory.rs) — Provider factory
