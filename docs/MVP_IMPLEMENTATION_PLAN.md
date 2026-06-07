# diffguard-rs — Implementation Plan

> Master roadmap for building a Rust-based AI code review CLI. Multi-provider LLM support, GitHub Actions integration, and local pre-commit execution.

---

## Project Overview

**diffguard-rs** is a provider-agnostic AI code review CLI that analyzes Pull Request diffs and submits review verdicts (APPROVE, REQUEST_CHANGES, COMMENT) directly to GitHub. It replaces multi-step JavaScript pipelines with a single Rust binary that fetches diffs, calls LLM APIs, parses verdict metadata in-memory, and submits the final review state — all in one execution.

---

## Architecture

### Repository Structure

**Phase 1 (Current): Single Crate**

```
diffguard-rs/
├── Cargo.toml                    # Single crate manifest
├── Cargo.lock
├── deny.toml                     # cargo-deny: license + security audit
├── .rustfmt.toml                 # Formatting config
├── .gitignore
├── README.md                     # Quick start + badges
├── CHANGELOG.md                  # Version history
├── CONTRIBUTING.md               # Dev guidelines
├── CODE_OF_CONDUCT.md
├── SECURITY.md
├── LICENSE                       # MIT (see License Note below)
│
├── src/                          # Single crate source
│   ├── main.rs                   # CLI entry point
│   ├── cli.rs                    # Clap argument parsing
│   ├── config.rs                 # Env vars + .reviewer.toml parsing
│   ├── diff.rs                   # PR diff fetching + local diff
│   ├── llm/                      # LLM provider modules
│   │   ├── mod.rs                # LlmProvider trait + types
│   │   ├── deepseek.rs           # DeepSeek provider (Phase 1)
│   │   ├── kimi.rs               # Kimi provider (Phase 2)
│   │   ├── qwen.rs               # Qwen provider (Phase 2)
│   │   ├── openrouter.rs         # OpenRouter provider (Phase 2)
│   │   ├── openai.rs             # Generic OpenAI provider (Phase 2)
│   │   └── factory.rs            # Provider factory (Phase 2)
│   ├── verdict.rs                # Verdict parsing + review state logic
│   ├── github.rs                 # GitHub API review submission
│   ├── output.rs                 # Terminal output + artifact writing
│   └── error.rs                  # Error types
│
├── examples/
│   ├── github-actions-workflow/  # Sample consumer workflows
│   ├── local-review/             # Pre-commit hook examples
│   └── custom-provider/          # Per-provider config examples (Phase 2)
│
├── benches/                       # Performance benchmarks (Phase 3)
├── tests/                         # Integration tests + test data
│   ├── verdict_tests.rs
│   ├── diff_tests.rs
│   ├── provider_tests.rs
│   └── test_data/
├── docs/                          # Extended documentation
│   ├── PROVIDERS.md               # Phase 2
│   ├── CONFIGURATION.md           # Phase 2
│   ├── LOCAL_MODE.md              # Phase 2
│   ├── ARCHITECTURE.md            # Phase 4
│   ├── USAGE.md                   # Phase 4
│   └── API.md                     # Phase 4
│
└── .github/workflows/             # CI/CD pipelines
```

**Future Reference: Multi-Crate Workspace (Phase 5+)**

> If demand emerges for using `diffguard` components as libraries, the single crate can be split into a workspace:
>
> ```
> crates/
> ├── diffguard-core/           # Diff fetch, verdict parser, GitHub API
> ├── diffguard-llm/            # LlmProvider trait + provider impls
> └── diffguard-cli/            # CLI args, config, main flow
> ```
> See [Future Workspace Decomposition](#future-workspace-decomposition-reference) for migration steps.

### Core Flow

```mermaid
[Fetch PR Diff] --(GitHub API)--> [Call LLM] --(DeepSeek/Kimi/Qwen/etc.)-->
[Parse Response In-Memory] --> [Extract Metadata Block] --> [Determine State]
--> [Submit Review via GitHub API] --> [Dismiss Old Blockers if needed]
```

### Provider Support Roadmap

| Provider | Phase | Base URL | Auth |
|---|---|---|---|
| **DeepSeek** | 1 | `https://api.deepseek.com` | `Bearer {key}` |
| **Kimi** (Moonshot AI) | 2 | `https://api.moonshot.ai/v1` | `Bearer {key}` |
| **Qwen** (Alibaba Cloud) | 2 | `https://dashscope-intl.aliyuncs.com/compatible-mode/v1` | `Bearer {key}` |
| **OpenRouter** | 2 | `https://openrouter.ai/api/v1` | `Bearer {key}` + referer headers |
| **OpenAI** (generic) | 2 | `https://api.openai.com/v1` | `Bearer {key}` |

---

## Quality Targets

| Metric | Target | Tool |
|---|---|---|
| **Test Coverage** | 85%+ | `cargo-tarpaulin` |
| **Documentation Coverage** | 85%+ | `cargo +nightly doc --show-coverage` |
| **Clippy** | 0 warnings | `cargo clippy -- -D warnings` |
| **Rustfmt** | Enforced in CI | `cargo fmt --check` |
| **License Audit** | 0 conflicts | `cargo-deny` |
| **Security Audit** | 0 known vulnerabilities | `cargo-audit` |

---

## Phase 1: Foundation — Single Crate + DeepSeek MVP

### Goal

Create a working Rust CLI in a single crate: fetch PR diffs, call DeepSeek, parse verdicts, submit reviews to GitHub. All other providers and advanced features are deferred.

### Deliverables

#### Repository Setup

- [x] Initialize Git repository with proper `.gitignore` for Rust
- [x] Create `Cargo.toml` with dependencies for single crate:
  - `reqwest`, `serde`, `serde_json`, `tokio`, `clap`, `anyhow`, `thiserror`, `regex`, `env_logger`, `colored`, `wiremock` (dev), `toml` (Phase 2)
- [x] Create `.rustfmt.toml` with project formatting rules
- [x] Create `deny.toml` for `cargo-deny` license + security auditing (includes `Unicode-3.0` license allowance)
- [ ] Add root-level docs: `README.md` (skeleton), `LICENSE`, `CODE_OF_CONDUCT.md`, `SECURITY.md`

#### Single Crate Structure

**`src/retry.rs`** — Basic retry logic:
- `with_retry<T, F, Fut>(operation: F) -> Result<T, DiffguardError>`
- Retry on: HTTP 429, 502, 503, 504, timeout errors
- Strategy: 2 retries with fixed backoff (1s, 2s)
- Never retry: 401/403, 404, parse errors, config errors
- All public items have `///` doc comments

**`src/error.rs`** — Define `DiffguardError` enum with variants:
- `GitHubApi { status: u16, message: String }`
- `LlmApi { provider: String, status: u16, message: String }`
- `VerdictParse(String)`
- `Config(String)`
- `Io(std::io::Error)`
- `DiffTooLarge { size_bytes: usize, line_count: usize }`
- `EmptyDiff`
- `PermissionDenied { state: String, message: String }`
- Helper methods: `is_retryable()`, `is_permission_denied()`
- All public items have `///` doc comments

**`src/diff.rs`** — Implement `fetch_pr_diff()` and `fetch_local_diff()`:
- `fetch_pr_diff(base_url, owner, repo, pr_number, token)` — configurable `base_url` for GitHub Enterprise support
- HTTP GET with `Accept: application/vnd.github.v3.diff`
- `github_headers(token)` helper validates token format via `HeaderValue::from_str` (returns `Config` error instead of panicking)
- Return `DiffResult { content: String, size_bytes: usize, line_count: usize }` or `DiffguardError::GitHubApi`
- Handle empty diff gracefully (`DiffguardError::EmptyDiff`)
- Size guard: if diff exceeds 100KB or 1,500 lines, return `DiffguardError::DiffTooLarge`
- `fetch_local_diff()` — executes `git diff --cached` subprocess for local mode
- All public items have `///` doc comments

**`src/verdict.rs`** — Implement verdict parsing:
- `parse_metadata_block(response: &str) -> Option<Verdict>`
- Regex compiled once via `std::sync::LazyLock` (avoids recompilation per call)
- `determine_review_state(verdict: &Verdict) -> ReviewState`
- Logic: `NEGATIVE || security > 0 || critical > 2` => `REQUEST_CHANGES`
- Logic: `critical == 0 && security == 0` => `APPROVE`
- Else => `COMMENT`
- Fallback: `evaluate_by_tags(response: &str) -> Verdict` — counts `[Critical Bug]` and `[Security]` occurrences
- All public items have `///` doc comments

**`src/github.rs`** — Implement GitHub review submission:
- `submit_review(base_url, owner, repo, pr_number, state, message, token)` — configurable `base_url` for GitHub Enterprise
- `dismiss_previous_reviews(base_url, owner, repo, pr_number, token)` — queries reviews with `CHANGES_REQUESTED` state and bodies containing `<!-- diffguard-bot -->` signature, then dismisses them
- Permission fallback: if `REQUEST_CHANGES` or `APPROVE` fails with permission error, retry with `COMMENT` and prepend `[Bot fallback from {state}]`
- `github_headers(token)` helper validates token format (returns `Config` error instead of panicking)
- Individual dismissal failures are logged as warnings (not silently swallowed)
- All public items have `///` doc comments

**`src/output.rs`** — Artifact + console output:
- `ARTIFACT_FILENAME` constant (`"review-result.txt"`)
- `write_artifact(review, verdict, state, config, path)` — writes structured artifact
- `print_colored_report(review, verdict, state)` — terminal output with verdict metadata included
- `print_colored_summary(review, verdict, state, config)` — full summary with provider metadata
- All public items have `///` doc comments

**`src/llm/mod.rs`** — LLM provider types + enum dispatch:
- Phase 1 uses enum-based dispatch (`Provider` enum with match arms) instead of a trait object
- The `LlmProvider` trait is deferred to Phase 2 when multiple providers require dynamic dispatch
- Shared types: `ChatMessage`, `ChatRequest`, `ChatResponse`, `LlmError`
- All public items have `///` doc comments

**`src/llm/deepseek.rs`** — DeepSeek provider implementation:
- Base URL: `https://api.deepseek.com`
- Endpoint: `POST /chat/completions`
- Model default: `deepseek-v4-flash`
- Temperature default: `0.1`
- `DeepSeekClient::new(api_key)` returns `Result<Self, DiffguardError>` (validates API key format, no panics)
- Builder methods: `with_base_url()`, `with_model()`
- Request body: OpenAI-compatible `messages` array with `system` + `user` roles
- Response parsing: extract `choices[0].message.content`
- All public items have `///` doc comments

**`src/llm/factory.rs`** — Provider factory:
- `create_provider(provider_name, api_key) -> Result<Provider, DiffguardError>`
- Propagates `DeepSeekClient::new()` errors (invalid API key format)
- All public items have `///` doc comments

**`src/cli.rs`** — Clap derive struct:
```rust
#[derive(Parser)]
pub struct Args {
    #[arg(short, long, default_value = ".github/review-prompt.md")]
    pub prompt_file: PathBuf,
    
    #[arg(short, long, default_value = "deepseek-v4-flash")]
    pub model: String,
    
    #[arg(short, long, default_value_t = 0.1)]
    pub temperature: f32,
    
    #[arg(long, env = "DIFFGUARD_PROVIDER", default_value = "deepseek")]
    pub provider: String,
}
```
- Note: `--config` / `-c` flag deferred to Phase 2 (TOML parsing not yet implemented)
- All public items have `///` doc comments

**`src/config.rs`** — Environment variable resolution + default prompt:
- `DEEPSEEK_API_KEY` (required for DeepSeek)
- `GITHUB_TOKEN` (required for GitHub mode)
- `PR_NUMBER` (required for GitHub mode)
- `REPO_FULL_NAME` (required for GitHub mode)
- `GITHUB_ACTIONS` (auto-detected for CI vs local mode)
- Embedded default prompt: used when `--prompt-file` is not found or not specified
- `Config::from_env()` — resolves all env vars, returns `Result`
- `Config::apply_args(&mut self, args: &Args)` — applies CLI flag overrides for `model`, `temperature`, `provider`
- `Config::load_prompt_file()` — loads prompt from file or keeps default
- `Config::validate_for_ci()` — validates required CI fields are present
- `Config::github_base_url` — configurable GitHub API base URL (default: `https://api.github.com`)
- All public items have `///` doc comments

**Default Prompt Template (embedded in binary):**
```markdown
You are a senior software engineer performing a code review on a Pull Request diff.

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
```

**`src/main.rs`** — Entry point:
- Parse CLI args with Clap
- `Config::from_env()` → `config.apply_args(&args)` → `config.load_prompt_file()` → `config.validate_for_ci()`
- `run_pipeline(config)` — extracted pipeline function for testability
- CI mode: fetch PR diff → call LLM → parse verdict → submit review → dismiss old blockers → write artifact
- Error handling: `anyhow::Context` for human-readable error messages
- Exit codes: `0` for success, `1` for any error, `2` for local mode `REQUEST_CHANGES`
- Uses `ARTIFACT_FILENAME` constant from `output` module

#### Tests (Phase 1)

- [x] `tests/verdict_tests.rs` — 15 integration tests for all verdict parsing scenarios
- [x] `tests/diff_tests.rs` — 4 mock HTTP tests for diff fetching (use `wiremock`)
- [x] `tests/provider_tests.rs` — 3 tests: DeepSeek mock API, factory creation, unknown provider
- [x] `src/retry.rs` inline tests — 3 tests: first-attempt success, eventual success, no retry on non-retryable
- [x] `src/verdict.rs` inline tests — 7 tests for metadata parsing and tag fallback
- [x] `src/diff.rs` inline tests — 2 tests for PR diff fetching
- [x] `src/llm/deepseek.rs` inline tests — 2 tests for chat completion
- [ ] Integration test: full pipeline with mock GitHub + mock LLM servers

#### CI Setup (Phase 1)

- [x] `.github/workflows/ci.yml`:
  - Format check: `cargo fmt --all -- --check`
  - Lint: `cargo clippy --all-targets --all-features -- -D warnings`
  - Unit + integration tests: `cargo test`
  - Doc tests: `cargo test --doc`
  - Release build smoke: `cargo build --release`
  - `cargo-deny`: license + security audit via `EmbarkStudios/cargo-deny-action@v1`
  - `cargo-audit`: vulnerability scanning
  - Coverage: `cargo tarpaulin --workspace --out xml` + upload to Codecov (deferred)
  - Doc coverage: `cargo +nightly doc --show-coverage` (deferred)
- [x] `.github/workflows/release.yml`:
  - Trigger: push tags `v*`
  - Build: `cargo build --release --target x86_64-unknown-linux-gnu`
  - Strip binary for size reduction
  - Create GitHub Release with binary asset via `softprops/action-gh-release@v2`

### Test Matrix for Phase 1

| Test | Input | Expected |
|---|---|---|
| Parse valid POSITIVE | `Verdict: POSITIVE, CriticalBugs: 0, SecurityIssues: 0` | `ReviewState::Approve` |
| Parse NEGATIVE | `Verdict: NEGATIVE` | `ReviewState::RequestChanges` |
| Parse critical > 2 | `CriticalBugs: 5` | `ReviewState::RequestChanges` |
| Parse security > 0 | `SecurityIssues: 1` | `ReviewState::RequestChanges` |
| Missing metadata | (no block in response) | Fallback to tag counting |
| Tag fallback | `[Critical Bug] x3` | `ReviewState::RequestChanges` |
| Clean tag fallback | No tags found | `ReviewState::Comment` |
| Empty diff | GitHub returns 200 + empty | Graceful warning, exit 0 |
| Diff too large | Diff exceeds 100KB or 1,500 lines | Submit comment explaining limit, exit 0 |
| GitHub 404 | PR doesn't exist | Error with PR number in message |
| GitHub 429 | Rate limited | Retry with backoff or clear error |
| DeepSeek timeout | No response in 60s | Retry once, then error |
| GitHub 429 | Rate limited | Retry twice with 1s/2s backoff |
| GitHub 503 | Transient outage | Retry twice with 1s/2s backoff |

### Changelog Entry — Phase 1

```markdown
## [0.1.0] — 2026-06-XX

### Added
- Initial release with DeepSeek provider support (`deepseek-v4-flash`)
- GitHub Actions integration: fetches PR diffs and submits review states
- In-memory verdict parsing (`[DIFFGUARD_VERDICT_METADATA]` block)
- Three review states: `APPROVE`, `REQUEST_CHANGES`, `COMMENT`
- Permission fallback: downgrades to `COMMENT` when approval/rejection is not permitted
- Dismissal of previous diffguard `CHANGES_REQUESTED` reviews (identified by `<!-- diffguard-bot -->` HTML comment signature) when new state is non-blocking
- `review-result.txt` artifact for downstream jobs
- Embedded default prompt (works out-of-the-box; override via `--prompt-file`)
- `--model` and `--temperature` CLI flags
- Single crate architecture (lean MVP — workspace deferred until library demand emerges)
- Basic retry logic for transient API failures (429, 502, 503, 504, timeouts)
- Comprehensive test suite (unit + integration) with mock HTTP servers
- CI pipeline: format, clippy, test, coverage, doc coverage, release build
- `cargo-deny` license and security auditing
```

---

## Phase 2: Multi-Provider Support + Local Mode

### Goal

Extend `src/llm/` to support multiple LLM providers. Add `.reviewer.toml` configuration and implement local pre-commit execution mode.

### Deliverables

#### Provider Implementations

> **Note:** Phase 1 uses enum-based dispatch (`Provider` enum). In Phase 2, introduce the `LlmProvider` async trait and refactor `Provider` to use `Box<dyn LlmProvider>` for dynamic dispatch across multiple providers.

- [ ] `src/llm/kimi.rs` — Kimi/Moonshot AI provider:
  - Base URL: `https://api.moonshot.ai/v1`
  - Auth header: `Bearer {KIMI_API_KEY}`
  - OpenAI-compatible schema with `reasoning_content` field support
  - Default model: `kimi-k2.5`
  - Client struct: `KimiClient { api_key: String, base_url: String }`

- [ ] `src/llm/qwen.rs` — Qwen/Alibaba Cloud provider:
  - Base URL: `https://dashscope-intl.aliyuncs.com/compatible-mode/v1`
  - Auth header: `Bearer {DASHSCOPE_API_KEY}`
  - Requires `result_format: "message"` for some models
  - Default model: `qwen-plus`
  - Client struct: `QwenClient { api_key: String, base_url: String }`

- [ ] `src/llm/openrouter.rs` — OpenRouter gateway:
  - Base URL: `https://openrouter.ai/api/v1`
  - Auth header: `Bearer {OPENROUTER_API_KEY}`
  - Additional headers: `HTTP-Referer`, `X-Title` for attribution
  - Supports routing to any model via OpenRouter's unified API
  - Client struct: `OpenRouterClient { api_key: String, base_url: String, http_referer: String }`

- [ ] `src/llm/openai.rs` — Generic OpenAI-compatible provider:
  - Base URL: `https://api.openai.com/v1` (configurable)
  - Auth header: `Bearer {OPENAI_API_KEY}`
  - Default model: `gpt-4o-mini`
  - Catch-all for any OpenAI-compatible endpoint
  - Client struct: `OpenAiClient { api_key: String, base_url: String }`

#### Provider Factory

- [ ] `src/llm/factory.rs` — `create_provider(provider_name: &str, api_key: &str) -> Provider`:
  - Matches provider name to enum variant
  - Returns `Provider::DeepSeek(...)`, `Provider::Kimi(...)`, etc.
  - Validates that required API key environment variable is set

#### Configuration File Support

- [ ] `src/config.rs` — TOML configuration:
  - Add `--config` / `-c` CLI flag to `src/cli.rs` (deferred from Phase 1)
  - Parse `.reviewer.toml` from repository root
  - `Config::apply_args()` already handles CLI overrides; extend to merge TOML values
  - Schema:
    ```toml
    provider = "deepseek"
    model = "deepseek-v4-flash"
    temperature = 0.1
    max_tokens = 8192
    
    [providers.deepseek]
    api_key_env = "DEEPSEEK_API_KEY"
    base_url = "https://api.deepseek.com"
    
    [providers.kimi]
    api_key_env = "KIMI_API_KEY"
    base_url = "https://api.moonshot.ai/v1"
    
    [providers.qwen]
    api_key_env = "DASHSCOPE_API_KEY"
    base_url = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
    
    [providers.openrouter]
    api_key_env = "OPENROUTER_API_KEY"
    base_url = "https://openrouter.ai/api/v1"
    http_referer = "https://github.com/YOUR_ORG/diffguard-rs"
    
    [providers.openai]
    api_key_env = "OPENAI_API_KEY"
    base_url = "https://api.openai.com/v1"
    ```
  - CLI flags override TOML values
  - Environment variables override both

#### Local Mode (Pre-commit)

- [ ] Detect local execution: `GITHUB_ACTIONS` env var is absent
- [ ] `src/diff.rs` — Local diff source: execute `git diff --cached` subprocess
- [ ] Skip GitHub API calls in local mode
- [ ] Terminal output with `colored` crate:
  - Print full LLM review with syntax highlighting
  - Print verdict summary with color-coded state
  - Print metadata block extract
- [ ] Exit behavior:
  - `exit(0)` if `ReviewState::Approve` or `ReviewState::Comment`
  - `exit(2)` if `ReviewState::RequestChanges` — aborts the commit
- [ ] `examples/local-review/pre-commit-hook.sh` — Example git hook script

#### Documentation (Phase 2)

- [ ] `docs/PROVIDERS.md` — Per-provider setup guide with API key acquisition instructions
- [ ] `docs/CONFIGURATION.md` — Complete `.reviewer.toml` reference
- [ ] `docs/LOCAL_MODE.md` — Pre-commit hook setup and local usage

### Changelog Entry — Phase 2

```markdown
## [0.2.0] — 2026-07-XX

### Added
- Kimi (Moonshot AI) provider support with `kimi-k2.5` default model
- Qwen (Alibaba Cloud) provider support with `qwen-plus` default model
- OpenRouter provider support with unified gateway routing
- Generic OpenAI-compatible provider for custom endpoints
- Provider factory for dynamic provider selection via `--provider` flag
- `.reviewer.toml` configuration file support
- Local pre-commit mode: analyzes `git diff --cached` and prints colored terminal output
- Commit blocking: aborts commit when review returns `REQUEST_CHANGES`
- `--provider` CLI flag for provider selection
- Provider-specific environment variable support (`KIMI_API_KEY`, `DASHSCOPE_API_KEY`, `OPENROUTER_API_KEY`, `OPENAI_API_KEY`)
- `docs/PROVIDERS.md`, `docs/CONFIGURATION.md`, `docs/LOCAL_MODE.md`

### Changed
- `src/llm/` restructured with provider-per-module pattern
- CLI argument parsing extended with provider selection
- Configuration resolution: CLI flags > env vars > TOML file > defaults
```

---

## Phase 3: Advanced Features

### Goal

Add production-hardening features: diff chunking for large PRs, response caching, cost/latency metrics, and enhanced CI pipeline features.

### Deliverables

#### Diff Chunking

- [ ] Detect diff size against model context window
- [ ] Truncation strategy: preserve first N and last N lines, summarize middle section with placeholder
- [ ] Configurable `max_tokens` in `.reviewer.toml`
- [ ] Warning when diff is truncated (included in review comment)

#### Response Caching

- [ ] Cache LLM responses by diff content hash (SHA-256)
- [ ] Cache location: `~/.cache/diffguard/responses/` or project-local `.diffguard/cache/`
- [ ] TTL: 24 hours by default, configurable
- [ ] Skip cache with `--no-cache` flag
- [ ] Cache hit logged in CI output for transparency

#### Metrics Export

- [ ] Track per-run metrics: token usage (input/output), API latency, cost estimate
- [ ] Export as JSON artifact: `diffguard-metrics.json`
- [ ] Console summary in CI logs:
  ```
  diffguard-rs Review Complete
  =============================
  Provider:    deepseek
  Model:       deepseek-v4-flash
  Tokens In:   4,230
  Tokens Out:  892
  Latency:     8.4s
  Est. Cost:   $0.003
  Verdict:     POSITIVE
  State:       APPROVE
  ```

#### Enhanced CI Pipeline

- [ ] `.github/workflows/ci.yml` additions:
  - `cargo-deny check` for license + security audit
  - `cargo-audit` for vulnerability scanning
  - Benchmark comparison against baseline (`cargo bench`)
- [ ] `.github/workflows/docs-deploy.yml` — Deploy `cargo doc` to GitHub Pages

#### Error Recovery

- [ ] Retry logic for transient failures (5xx, 429):
  - Exponential backoff: 1s, 2s, 4s, 8s
  - Max 3 retries per request
  - Jitter to avoid thundering herd
- [ ] Circuit breaker pattern: skip LLM call if provider has failed N times recently

### Changelog Entry — Phase 3

```markdown
## [0.3.0] — 2026-08-XX

### Added
- Diff chunking for large PRs exceeding model context window
- Response caching by diff hash (SHA-256) with configurable TTL
- `--no-cache` flag to bypass cache
- Metrics export: `diffguard-metrics.json` with token usage, latency, cost estimate
- Console metrics summary in CI output
- Exponential backoff retry for transient API failures (5xx, 429)
- Circuit breaker pattern for failing providers
- `cargo-deny` and `cargo-audit` integrated in CI
- Documentation auto-deployment to GitHub Pages

### Changed
- `fetch_pr_diff()` now returns diff + size metadata for chunking decisions
- `LlmProvider::chat_completion()` accepts optional `max_tokens` parameter
```

---

## Phase 4: README + Documentation Polish

### Goal

Create a world-class README and complete all documentation files. This is the public-facing quality gate before crates.ai registration.

### Deliverables

#### README.md (Complete Rewrite)

- [ ] **Hero section**: One-sentence description + animated GIF or screenshot of terminal output
- [ ] **Badges**: CI status, test coverage, docs.rs, crates.io version, license
- [ ] **Quick Start** (3-step copy-paste):
  ```bash
  # 1. Download binary
  curl -L -o diffguard \
    https://github.com/YOUR_ORG/diffguard-rs/releases/latest/download/diffguard
  chmod +x diffguard
  
  # 2. Create prompt file
  echo "Act as a Principal Architect reviewing code..." > .github/review-prompt.md
  
  # 3. Add to your workflow (see examples/github-actions-workflow/ai-review.yml)
  ```
- [ ] **Feature highlights** with icons:
  - Multi-provider (DeepSeek, Kimi, Qwen, OpenRouter, OpenAI)
  - In-memory verdict parsing (no intermediate comments)
  - GitHub Actions + local pre-commit support
  - Configurable prompts per repository
  - Fast: single binary, ~3s execution
- [ ] **Installation**: Binary download, compile from source, cargo install (when published)
- [ ] **Usage examples**: CI mode, local mode, with different providers
- [ ] **Configuration**: Link to `docs/CONFIGURATION.md`
- [ ] **Provider setup**: Quick links to `docs/PROVIDERS.md`
- [ ] **Architecture**: Brief overview + link to `docs/ARCHITECTURE.md`
- [ ] **Contributing**: Link to `CONTRIBUTING.md`
- [ ] **License**: MIT badge + full text link

#### docs/ARCHITECTURE.md

- [ ] System design overview with diagrams (ASCII or mermaid)
- [ ] In-memory pipeline explanation (why no intermediate comments)
- [ ] Provider trait design and extension guide
- [ ] CI vs local mode detection logic
- [ ] Security model: secret handling, token isolation, permissions
- [ ] Performance characteristics: latency breakdown, memory usage, binary size

#### docs/USAGE.md

- [ ] Complete CLI reference with all flags and environment variables
- [ ] Exit codes reference table
- [ ] GitHub Actions integration guide with full workflow YAML
- [ ] Local pre-commit setup with git hook examples
- [ ] `.reviewer.toml` schema documentation
- [ ] Troubleshooting section: common errors and solutions

#### docs/API.md

- [ ] Library crate API documentation (if workspace split occurred, otherwise module-level docs)
- [ ] Examples of using modules as libraries in other Rust projects
- [ ] Provider trait implementation guide for custom providers

#### CHANGELOG.md Update

- [ ] Ensure all versions follow [Keep a Changelog](https://keepachangelog.com/) format
- [ ] Add `[Unreleased]` section for work in progress

### Changelog Entry — Phase 4

```markdown
## [0.4.0] — 2026-08-XX

### Added
- Complete README.md with quick-start, badges, feature highlights, and usage examples
- `docs/ARCHITECTURE.md` — System design and extension guide
- `docs/USAGE.md` — Full CLI reference and troubleshooting
- `docs/API.md` — Library module API documentation
- GitHub Pages documentation site auto-deployment

### Changed
- README rewritten for clarity and completeness
- All documentation reviewed and cross-linked
```

---

## Phase 5: Implementation Guide

### Goal

Create a comprehensive developer-facing guide that documents how the project is built, how to extend it, and the architectural decisions behind key design choices. This is the "how and why" companion to the user-facing documentation.

### Deliverables

#### docs/implementation-guide.md

**1. Getting Started for Contributors**
- [ ] Development environment setup: Rust toolchain (1.82+), required components (`clippy`, `rustfmt`)
- [ ] `cargo` commands for daily development:
  ```bash
  cargo build
  cargo test
  cargo clippy --all-targets --all-features -- -D warnings
  cargo fmt --all
  cargo doc --no-deps --open
  ```
- [ ] Running integration tests: `cargo test -- --ignored` for tests requiring network
- [ ] Using `cargo-tarpaulin` for coverage reports locally

**2. Crate Organization**
- [ ] Rationale for single-crate structure vs workspace (see [Architecture Decision](#why-single-crate-for-mvp))
- [ ] Module dependency flow: `main.rs` → `cli.rs` + `config.rs`; `diff.rs` + `github.rs` + `verdict.rs` + `llm/` as core pipeline
- [ ] When and how to split into a workspace (see [Future Workspace Decomposition](#future-workspace-decomposition-reference))

**3. Adding a New LLM Provider**
- [ ] Step-by-step guide:
  1. Create `src/llm/{provider}.rs`
  2. Implement `{Provider}Client` struct with `chat_completion` method
  3. Add `Provider::{Provider}` variant in `src/llm/mod.rs`
  4. Add match arm in `Provider::chat_completion()`
  5. Add API key env var constant
  6. Register in `src/llm/factory.rs`
  7. Add TOML config schema in `src/config.rs`
  8. Add unit tests with mock responses
  9. Update `docs/PROVIDERS.md`
- [ ] Provider implementation checklist with code review criteria

**4. The In-Memory Pipeline**
- [ ] Why we parse metadata in-memory instead of posting intermediate comments
- [ ] Comparison with two-step JS pipeline: network calls, race conditions, latency
- [ ] Error handling strategy: what happens when each step fails

**5. Testing Strategy**
- [ ] Unit test patterns: pure functions, mock traits, test data fixtures
- [ ] Integration test patterns: `wiremock` for HTTP, `tempfile` for filesystem
- [ ] How to write a good test for `verdict.rs`, `diff.rs`, `github.rs`
- [ ] Test data organization in `tests/test_data/`

**6. CI/CD Pipeline**
- [ ] Full explanation of each CI job and its purpose
- [ ] How the release pipeline works: binary compilation, stripping, GitHub Release creation
- [ ] Version tagging strategy and release cadence recommendations

**7. Performance Considerations**
- [ ] Why `reqwest` over `ureq` or `minreq`
- [ ] Why compile to a static binary instead of running `cargo run` in CI
- [ ] Binary size optimization: `strip`, LTO, `panic = "abort"`
- [ ] Benchmarking with `criterion.rs`

**8. Security Model**
- [ ] Secret handling: env vars only, no hardcoded keys
- [ ] Log sanitization: `[REDACTED]` for auth headers
- [ ] GitHub token scope: minimum required permissions
- [ ] Supply chain security: `cargo-deny`, `cargo-audit`, `Cargo.lock`

**9. Common Tasks**
- [ ] Bumping the version: update `Cargo.toml`, update `CHANGELOG.md`, tag and push
- [ ] Adding a new CLI flag: update `cli.rs`, `config.rs`, `main.rs`, `docs/USAGE.md`
- [ ] Debugging a failing review: enable `RUST_LOG=debug`, check `review-result.txt`

### Changelog Entry — Phase 5

```markdown
## [0.5.0] — 2026-09-XX

### Added
- `docs/implementation-guide.md` — Comprehensive developer guide
- Contributor onboarding documentation
- Step-by-step guide for adding new LLM providers
- Testing strategy documentation with patterns and examples
- Performance and security deep-dives
- Common task recipes for maintainers
```

---

## Phase 6: crates.ai Registration

### Goal

Register diffguard-rs on [crates.ai](https://crates.ai) for discovery and distribution as a Rust crate, and optionally publish to [crates.io](https://crates.io) for `cargo install` support.

### Prerequisites Checklist

Before registration, all of the following must be complete:
- [ ] `Cargo.toml` has proper metadata:
  - `name`, `version`, `authors`, `edition`, `license`, `description`, `repository`, `keywords`, `categories`
- [ ] `README.md` is complete and professional
- [ ] `CHANGELOG.md` has at least one released version
- [ ] `LICENSE` file present at root (MIT)
- [ ] All public API items have doc comments (`#![deny(missing_docs)]`)
- [ ] `cargo test` passes 100%
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] `cargo deny check` passes (license + security)
- [ ] `cargo audit` passes (no known vulnerabilities)
- [ ] Test coverage >= 85% (`cargo tarpaulin`)
- [ ] Documentation coverage >= 85% (`cargo +nightly doc --show-coverage`)
- [ ] At least one published GitHub Release with binary asset
- [ ] CI pipeline is green on `main` branch

### crates.ai Registration Steps

1. **Create a crates.ai account** at https://crates.ai (uses GitHub OAuth)
2. **Register the repository**:
   - Submit the GitHub repository URL
   - crates.ai scans the `Cargo.toml`
   - Verifies buildability and documentation
3. **Add project metadata**:
   - Description: "AI-powered code review CLI for GitHub PRs. Multi-provider LLM support with in-memory verdict parsing."
   - Tags: `ai`, `code-review`, `github`, `cli`, `llm`, `devops`, `ci-cd`
   - Screenshot or demo GIF of terminal output
4. **Link documentation**:
   - README (primary)
   - docs.rs (auto-generated from `cargo doc`)
   - GitHub Pages docs site

### crates.io Publishing (Optional but Recommended)

If publishing to crates.io for `cargo install diffguard`:

1. **Verify `Cargo.toml`**:
   ```toml
   [package]
   name = "diffguard"
   version = "0.5.0"
   edition = "2021"
   authors = ["Your Name <email@example.com>"]
   license = "MIT"
   description = "AI-powered code review CLI for GitHub PRs"
   repository = "https://github.com/YOUR_ORG/diffguard-rs"
   homepage = "https://github.com/YOUR_ORG/diffguard-rs"
   documentation = "https://docs.rs/diffguard"
   readme = "README.md"
   keywords = ["ai", "code-review", "github", "llm", "cli"]
   categories = ["development-tools", "command-line-utilities"]
   ```
2. **Login and publish**:
   ```bash
   cargo login  # paste API key from crates.io
   cargo publish
   ```

### Changelog Entry — Phase 6

```markdown
## [0.6.0] — 2026-09-XX

### Added
- Registered on crates.ai for project discovery
- Published to crates.io: `cargo install diffguard`
- docs.rs documentation auto-generated and linked

### Changed
- `Cargo.toml` metadata finalized
- `README.md` includes `cargo install` instructions
```

---

## Reference: Future Workspace Decomposition

> **When to split:** Only if concrete demand emerges for using `diffguard` components as standalone libraries (e.g., another project wants to import just the LLM provider trait, or just the verdict parser).
>
> **Migration steps:**
> 1. Create workspace `Cargo.toml` with `[workspace.members]`
> 2. Extract `src/llm/` → `crates/diffguard-llm/src/`
> 3. Extract `src/diff.rs`, `src/verdict.rs`, `src/github.rs`, `src/output.rs`, `src/error.rs` → `crates/diffguard-core/src/`
> 4. Keep `src/main.rs`, `src/cli.rs`, `src/config.rs` → `crates/diffguard-cli/src/`
> 5. Add `diffguard-core` and `diffguard-llm` as path dependencies in `diffguard-cli/Cargo.toml`
> 6. Use `[workspace.dependencies]` to share common crate versions
> 7. Update all `use` statements and test imports
> 8. Update CI to use `--workspace` flag
>
> **Why we didn't start here:** Workspace boundaries add friction to early iteration. Internal APIs change frequently during MVP development. Crate-splitting is easy to do later and hard to undo if done too early.

---

## Reference: Why Single Crate for MVP

**Decision:** Start as a single crate. Defer workspace split until library demand is proven.

**Rationale:**
- **Faster iteration:** No cross-crate compilation boundaries; refactoring is a single `cargo check`
- **Simpler testing:** Unit tests can access private modules via `#[cfg(test)]`; no need to expose internals prematurely
- **Less boilerplate:** One `Cargo.toml`, one version to bump, no workspace dependency management
- **YAGNI:** No identified consumer needs `diffguard-llm` or `diffguard-core` as independent libraries yet
- **Easy to split later:** Moving modules between crates is a well-understood Rust refactoring; the reverse is painful

**When to revisit:** If any of the following happen:
- Another project wants to depend on `diffguard` LLM providers as a library
- The CLI binary and library logic need independent versioning
- Compile times become a bottleneck due to crate size (unlikely for this scope)

---

## Appendix A: Environment Variables Reference

| Variable | Required By | Description |
|---|---|---|
| `DEEPSEEK_API_KEY` | DeepSeek provider | API key from DeepSeek platform |
| `KIMI_API_KEY` | Kimi provider | API key from Moonshot AI platform |
| `DASHSCOPE_API_KEY` | Qwen provider | API key from Alibaba Cloud DashScope |
| `OPENROUTER_API_KEY` | OpenRouter provider | API key from OpenRouter |
| `OPENAI_API_KEY` | OpenAI provider | API key from OpenAI |
| `GITHUB_TOKEN` | GitHub mode | Auto-provided by GitHub Actions |
| `PR_NUMBER` | GitHub mode | Pull request number |
| `REPO_FULL_NAME` | GitHub mode | Repository in `owner/repo` format |
| `GITHUB_ACTIONS` | Auto-detected | Presence indicates CI mode |

## Appendix B: CLI Flags Reference

| Flag | Short | Default | Description |
|---|---|---|---|
| `--prompt-file` | `-p` | `.github/review-prompt.md` | Path to system prompt markdown file |
| `--model` | `-m` | (provider-specific) | LLM model identifier |
| `--temperature` | `-t` | `0.1` | Sampling temperature (0.0 - 2.0) |
| `--provider` | | `deepseek` | LLM provider to use |
| `--config` | `-c` | `.reviewer.toml` | Path to configuration TOML file (Phase 2) |
| `--no-cache` | | | Bypass response cache (Phase 3) |
| `--help` | `-h` | | Display help |
| `--version` | `-V` | | Display version |

## Appendix C: Exit Codes

| Code | Meaning |
|---|---|
| `0` | Review completed successfully |
| `1` | Error occurred (API failure, parse error, config error, etc.) |
| `2` | Local mode only: review returned `REQUEST_CHANGES` (blocks commit) |

## Appendix D: Review State Logic

```
if verdict == "NEGATIVE" || security_issues > 0 || critical_bugs > 2:
    state = REQUEST_CHANGES
else if critical_bugs == 0 && security_issues == 0:
    state = APPROVE
else:
    state = COMMENT
```

### Design Rationale: Asymmetric Safety Model

The logic intentionally treats the LLM's pessimism as authoritative but its optimism as conditional:

- **Pessimistic signals are always trusted:** A `NEGATIVE` verdict, any security issue, or >2 critical bugs always results in `REQUEST_CHANGES`. These are signals we never want to ignore.
- **Optimistic signals require clean counts:** A `POSITIVE` verdict only yields `APPROVE` when both `CriticalBugs == 0` and `SecurityIssues == 0`. If the LLM is positive but reports minor bugs (1–2 critical), the state is `COMMENT` — a human can still approve, but we don't auto-approve questionable code.

**Why asymmetric?** It's safer. A false `APPROVE` lets bugs slip through. A false `COMMENT` just means a human takes a second look. This behavior should be documented in `README.md` and `docs/ARCHITECTURE.md`.

If `REQUEST_CHANGES` or `APPROVE` fails due to GitHub permissions, fallback to `COMMENT`.

## Appendix E: License Note

The root `LICENSE` file is **MIT** (Copyright 2026 Nebula Ideas). Earlier versions of this plan referenced Apache-2.0; the root `LICENSE` file takes precedence. If you wish to change the license, update both the `LICENSE` file and `Cargo.toml` `license` field before publishing.
