# diffguard-rs — Implementation Plan

> Master roadmap for building a Rust-based AI code review CLI. Multi-provider LLM support, GitHub Actions integration, and local pre-commit execution.

---

## Project Overview

**diffguard-rs** is a provider-agnostic AI code review CLI that analyzes Pull Request diffs and submits review verdicts (APPROVE, REQUEST_CHANGES, COMMENT) directly to GitHub. It replaces multi-step JavaScript pipelines with a single Rust binary that fetches diffs, calls LLM APIs, parses verdict metadata in-memory, and submits the final review state — all in one execution.

---

## Architecture

### Workspace Structure

```
diffguard-rs/
|-- Cargo.toml                    # Workspace manifest
|-- Cargo.lock
|-- deny.toml                     # cargo-deny: license + security audit
|-- .rustfmt.toml                 # Formatting config
|-- .gitignore
|-- README.md                     # Quick start + badges
|-- CHANGELOG.md                  # Version history
|-- CONTRIBUTING.md               # Dev guidelines
|-- CODE_OF_CONDUCT.md
|-- SECURITY.md
|-- LICENSE                       # Apache-2.0
|
|-- crates/
|   |-- diffguard-core/           # Diff fetch, verdict parser, GitHub API
|   |-- diffguard-llm/            # LlmProvider trait + provider impls
|   |-- diffguard-cli/            # CLI args, config, main flow
|
|-- examples/
|   |-- github-actions-workflow/  # Sample consumer workflows
|   |-- local-review/             # Pre-commit hook examples
|   |-- custom-provider/          # Per-provider config examples
|
|-- benches/                       # Performance benchmarks
|-- tests/                         # Integration tests + test data
|-- docs/                          # Extended documentation
|-- .github/workflows/             # CI/CD pipelines
```

### Core Flow

```
[Fetch PR Diff] --(GitHub API)--> [Call LLM] --(DeepSeek/Kimi/Qwen/etc.)-->
[Parse Response In-Memory] --> [Extract Metadata Block] --> [Determine State]
--> [Submit Review via gh pr review] --> [Dismiss Old Blockers if needed]
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

## Phase 1: Foundation — Workspace + DeepSeek MVP

### Goal
Create the repository structure with a Cargo workspace, implement the core modules (diff fetching, verdict parsing, error handling), and wire the first LLM provider (DeepSeek) into a working CLI binary that runs in GitHub Actions.

### Deliverables

#### Repository Setup
- [ ] Initialize Git repository with proper `.gitignore` for Rust
- [ ] Create workspace `Cargo.toml` with `[workspace.dependencies]` for shared crates
- [ ] Create `.rustfmt.toml` with project formatting rules
- [ ] Create `deny.toml` for `cargo-deny` license + security auditing
- [ ] Add root-level docs: `README.md` (skeleton), `LICENSE`, `CODE_OF_CONDUCT.md`, `SECURITY.md`

#### Crate: `diffguard-core`
- [ ] Create crate manifest with dependencies: `reqwest`, `serde`, `thiserror`, `regex`
- [ ] `src/error.rs` — Define `DiffguardError` enum with variants:
  - `GitHubApi { status: u16, message: String }`
  - `LlmApi { provider: String, status: u16, message: String }`
  - `VerdictParse(String)`
  - `Config(String)`
  - `Io(std::io::Error)`
- [ ] `src/diff.rs` — Implement `fetch_pr_diff()`:
  - HTTP GET to `https://api.github.com/repos/{owner}/{repo}/pulls/{number}`
  - Header: `Accept: application/vnd.github.v3.diff`
  - Header: `Authorization: Bearer {GITHUB_TOKEN}`
  - Header: `X-GitHub-Api-Version: 2022-11-28`
  - Return `String` or `DiffguardError::GitHubApi`
  - Handle empty diff gracefully (warning log + early exit)
- [ ] `src/verdict.rs` — Implement verdict parsing:
  - `parse_metadata_block(response: &str) -> Option<Verdict>`
  - Regex: `\[OPENCODE_VERDICT_METADATA\][\s\S]*?Verdict:\s*(\w+)[\s\S]*?CriticalBugs:\s*(\d+)[\s\S]*?SecurityIssues:\s*(\d+)`
  - `determine_review_state(verdict: &Verdict) -> ReviewState`
  - Logic: `NEGATIVE || security > 0 || critical > 2` => `REQUEST_CHANGES`
  - Logic: `critical == 0 && security == 0` => `APPROVE`
  - Else => `COMMENT`
  - Fallback: `evaluate_by_tags(response: &str) -> Verdict` — counts `[Critical Bug]` and `[Security]` occurrences
- [ ] `src/github.rs` — Implement GitHub review submission:
  - `submit_review(pr_number: u64, state: ReviewState, message: &str)` — executes `gh pr review {n} --{state} -b "{msg}"`
  - `dismiss_previous_reviews(pr_number: u64)` — queries bot reviews with `CHANGES_REQUESTED` state and dismisses them
  - Permission fallback: if `--request-changes` or `--approve` fails with permission error, retry with `--comment` and prepend `[Bot fallback from {state}]`
- [ ] `src/output.rs` — Artifact + console output:
  - `write_artifact(content: &str, path: &str)` — writes `review-result.txt`
  - `print_colored_report(review: &str, state: &ReviewState)` — terminal output for local mode (Phase 2)

#### Crate: `diffguard-llm`
- [ ] Create crate manifest with dependencies: `reqwest`, `serde`, `serde_json`, `tokio`, `async-trait`, `thiserror`
- [ ] `src/lib.rs` — Define `LlmProvider` trait:
  ```rust
  #[async_trait]
  pub trait LlmProvider: Send + Sync {
      async fn chat_completion(
          &self,
          system_prompt: &str,
          user_message: &str,
          temperature: f32,
      ) -> Result<String, LlmError>;
  }
  ```
- [ ] `src/types.rs` — Shared types: `ChatMessage`, `ChatRequest`, `ChatResponse`, `LlmError`
- [ ] `src/deepseek.rs` — DeepSeek provider implementation:
  - Base URL: `https://api.deepseek.com`
  - Endpoint: `POST /chat/completions`
  - Model default: `deepseek-v4-flash`
  - Temperature default: `0.1`
  - Request body: OpenAI-compatible `messages` array with `system` + `user` roles
  - Response parsing: extract `choices[0].message.content`

#### Crate: `diffguard-cli`
- [ ] Create crate manifest with dependencies: `clap`, `tokio`, `anyhow`, `colored`
- [ ] `src/cli.rs` — Clap derive struct:
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
- [ ] `src/config.rs` — Environment variable resolution:
  - `DEEPSEEK_API_KEY` (required for DeepSeek)
  - `GITHUB_TOKEN` (required for GitHub mode)
  - `PR_NUMBER` (required for GitHub mode)
  - `REPO_FULL_NAME` (required for GitHub mode)
  - `GITHUB_ACTIONS` (auto-detected for CI vs local mode)
- [ ] `src/main.rs` — Entry point:
  - Parse CLI args with Clap
  - Detect CI mode: `std::env::var("GITHUB_ACTIONS").is_ok()`
  - CI mode: fetch PR diff → call LLM → parse verdict → submit review → dismiss old blockers → write artifact
  - Error handling: `anyhow::Context` for human-readable error messages
  - Exit codes: `0` for success, `1` for any error

#### Tests
- [ ] `crates/diffguard-core/tests/verdict_tests.rs` — Unit tests for all verdict parsing scenarios (see test matrix in analysis doc)
- [ ] `crates/diffguard-core/tests/diff_tests.rs` — Mock HTTP tests for diff fetching (use `wiremock`)
- [ ] `crates/diffguard-llm/tests/provider_tests.rs` — Mock DeepSeek API responses
- [ ] Integration test: full pipeline with mock GitHub + mock LLM servers

#### CI Setup
- [ ] `.github/workflows/ci.yml`:
  - Format check: `cargo fmt --all -- --check`
  - Lint: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - Unit + integration tests: `cargo test --workspace`
  - Doc tests: `cargo test --doc --workspace`
  - Coverage: `cargo tarpaulin --workspace --out xml` + upload to Codecov
  - Doc coverage: `cargo +nightly doc --show-coverage` (parse and enforce 85% threshold)
  - Release build smoke: `cargo build --release -p diffguard-cli`
- [ ] `.github/workflows/release.yml`:
  - Trigger: push tags `v*`
  - Build: `cargo build --release -p diffguard-cli --target x86_64-unknown-linux-gnu`
  - Strip binary for size reduction
  - Create GitHub Release with binary asset

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
| GitHub 404 | PR doesn't exist | Error with PR number in message |
| GitHub 429 | Rate limited | Retry with backoff or clear error |
| DeepSeek timeout | No response in 60s | Error, no review submitted |

### Changelog Entry

```markdown
## [0.1.0] — 2026-06-XX

### Added
- Initial release with DeepSeek provider support (`deepseek-v4-flash`)
- GitHub Actions integration: fetches PR diffs and submits review states
- In-memory verdict parsing (`[OPENCODE_VERDICT_METADATA]` block)
- Three review states: `APPROVE`, `REQUEST_CHANGES`, `COMMENT`
- Permission fallback: downgrades to `COMMENT` when approval/rejection is not permitted
- Dismissal of previous bot `CHANGES_REQUESTED` reviews when new state is non-blocking
- `review-result.txt` artifact for downstream jobs
- Configurable system prompts via `--prompt-file` flag
- `--model` and `--temperature` CLI flags
- Workspace structure with 3 crates: `diffguard-core`, `diffguard-llm`, `diffguard-cli`
- Comprehensive test suite (unit + integration) with mock HTTP servers
- CI pipeline: format, clippy, test, coverage, doc coverage, release build
- `cargo-deny` license and security auditing
```

---

## Phase 2: Multi-Provider Support

### Goal
Extend `diffguard-llm` to support multiple LLM providers through the existing `LlmProvider` trait. Add Kimi, Qwen, OpenRouter, and generic OpenAI support. Introduce `.reviewer.toml` configuration and implement local pre-commit execution mode.

### Deliverables

#### Provider Implementations
- [ ] `crates/diffguard-llm/src/kimi.rs` — Kimi/Moonshot AI provider:
  - Base URL: `https://api.moonshot.ai/v1`
  - Auth header: `Bearer {KIMI_API_KEY}`
  - OpenAI-compatible schema with `reasoning_content` field support
  - Default model: `kimi-k2.5`
- [ ] `crates/diffguard-llm/src/qwen.rs` — Qwen/Alibaba Cloud provider:
  - Base URL: `https://dashscope-intl.aliyuncs.com/compatible-mode/v1`
  - Auth header: `Bearer {DASHSCOPE_API_KEY}`
  - Requires `result_format: "message"` for some models
  - Default model: `qwen-plus`
- [ ] `crates/diffguard-llm/src/openrouter.rs` — OpenRouter gateway:
  - Base URL: `https://openrouter.ai/api/v1`
  - Auth header: `Bearer {OPENROUTER_API_KEY}`
  - Additional headers: `HTTP-Referer`, `X-Title` for attribution
  - Supports routing to any model via OpenRouter's unified API
- [ ] `crates/diffguard-llm/src/openai.rs` — Generic OpenAI-compatible provider:
  - Base URL: `https://api.openai.com/v1` (configurable)
  - Auth header: `Bearer {OPENAI_API_KEY}`
  - Default model: `gpt-4o-mini`
  - Catch-all for any OpenAI-compatible endpoint

#### Provider Factory
- [ ] `crates/diffguard-llm/src/factory.rs` — `create_provider(provider_name: &str, api_key: &str) -> Box<dyn LlmProvider>`:
  - Matches provider name to implementation
  - Returns boxed trait object for dynamic dispatch
  - Validates that required API key environment variable is set

#### Configuration File Support
- [ ] `crates/diffguard-cli/src/config.rs` — TOML configuration:
  - Parse `.reviewer.toml` from repository root
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
  - `exit(1)` if `ReviewState::RequestChanges` — aborts the commit
- [ ] `examples/local-review/pre-commit-hook.sh` — Example git hook script

#### Documentation
- [ ] `docs/PROVIDERS.md` — Per-provider setup guide with API key acquisition instructions
- [ ] `docs/CONFIGURATION.md` — Complete `.reviewer.toml` reference
- [ ] `docs/LOCAL_MODE.md` — Pre-commit hook setup and local usage

### Changelog Entry

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
- `diffguard-llm` crate restructured with provider-per-module pattern
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

### Changelog Entry

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
- [ ] **License**: Apache-2.0 badge + full text link

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
- [ ] Library crate API documentation for `diffguard-core` and `diffguard-llm`
- [ ] Examples of using crates as libraries in other Rust projects
- [ ] Provider trait implementation guide for custom providers

#### CHANGELOG.md Update
- [ ] Ensure all versions follow [Keep a Changelog](https://keepachangelog.com/) format
- [ ] Add `[Unreleased]` section for work in progress

### Changelog Entry

```markdown
## [0.4.0] — 2026-08-XX

### Added
- Complete README.md with quick-start, badges, feature highlights, and usage examples
- `docs/ARCHITECTURE.md` — System design and extension guide
- `docs/USAGE.md` — Full CLI reference and troubleshooting
- `docs/API.md` — Library crate API documentation
- GitHub Pages documentation site auto-deployment

### Changed
- README rewritten for clarity and completeness
- All documentation reviewed and cross-linked
```

---

## Phase 5: implementation-guide.md

### Goal
Create a comprehensive developer-facing guide that documents how the project is built, how to extend it, and the architectural decisions behind key design choices. This is the "how and why" companion to the user-facing documentation.

### Deliverables

#### docs/implementation-guide.md

**1. Getting Started for Contributors**
- [ ] Development environment setup: Rust toolchain (1.82+), required components (`clippy`, `rustfmt`)
- [ ] `cargo` commands for daily development:
  ```bash
  cargo build --workspace
  cargo test --workspace
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  cargo fmt --all
  cargo doc --workspace --no-deps --open
  ```
- [ ] Running integration tests: `cargo test --workspace -- --ignored` for tests requiring network
- [ ] Using `cargo-tarpaulin` for coverage reports locally

**2. Workspace Organization**
- [ ] Rationale for 3-crate structure vs single crate or more granular decomposition
- [ ] Dependency flow: `diffguard-cli` depends on both `diffguard-core` and `diffguard-llm`; `diffguard-core` is independent; `diffguard-llm` is independent
- [ ] Adding a new crate to the workspace (checklist)

**3. Adding a New LLM Provider**
- [ ] Step-by-step guide:
  1. Create `crates/diffguard-llm/src/{provider}.rs`
  2. Implement `LlmProvider` trait
  3. Add API key env var constant
  4. Register in `factory.rs`
  5. Add TOML config schema in `config.rs`
  6. Add unit tests with mock responses
  7. Update `docs/PROVIDERS.md`
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
- [ ] Bumping the version: update workspace `Cargo.toml`, update `CHANGELOG.md`, tag and push
- [ ] Adding a new CLI flag: update `cli.rs`, `config.rs`, `main.rs`, `docs/USAGE.md`
- [ ] Debugging a failing review: enable `RUST_LOG=debug`, check `review-result.txt`

### Changelog Entry

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
- [ ] All crates in workspace have proper `Cargo.toml` metadata:
  - `name`, `version`, `authors`, `edition`, `license`, `description`, `repository`, `keywords`, `categories`
- [ ] `README.md` is complete and professional
- [ ] `CHANGELOG.md` has at least one released version
- [ ] `LICENSE` file present at root (Apache-2.0)
- [ ] All public API items have doc comments (`#![deny(missing_docs)]`)
- [ ] `cargo test --workspace` passes 100%
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] `cargo deny check` passes (license + security)
- [ ] `cargo audit` passes (no known vulnerabilities)
- [ ] Test coverage >= 85% (`cargo tarpaulin`)
- [ ] Documentation coverage >= 85% (`cargo +nightly doc --show-coverage`)
- [ ] At least one published GitHub Release with binary asset
- [ ] CI pipeline is green on `main` branch

### crates.ai Registration Steps

1. **Create a crates.ai account** at https://crates.ai (uses GitHub OAuth)
2. **Register the workspace**:
   - Submit the GitHub repository URL
   - crates.ai scans the `Cargo.toml` workspace manifest
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

If publishing to crates.io for `cargo install diffguard-cli`:

1. **Verify each crate's `Cargo.toml`**:
   ```toml
   [package]
   name = "diffguard-cli"
   version = "0.5.0"
   edition = "2021"
   authors = ["Your Name <email@example.com>"]
   license = "Apache-2.0"
   description = "AI-powered code review CLI for GitHub PRs"
   repository = "https://github.com/YOUR_ORG/diffguard-rs"
   homepage = "https://github.com/YOUR_ORG/diffguard-rs"
   documentation = "https://docs.rs/diffguard-cli"
   readme = "../../README.md"
   keywords = ["ai", "code-review", "github", "llm", "cli"]
   categories = ["development-tools", "command-line-utilities"]
   ```
2. **Login and publish**:
   ```bash
   cargo login  # paste API key from crates.io
   cd crates/diffguard-core && cargo publish
   cd ../diffguard-llm && cargo publish
   cd ../diffguard-cli && cargo publish
   ```
3. **Note**: Publish in dependency order (core first, then llm, then cli)

### Changelog Entry

```markdown
## [0.6.0] — 2026-09-XX

### Added
- Registered on crates.ai for project discovery
- Published to crates.io: `cargo install diffguard-cli`
- All workspace crates published with proper metadata
- docs.rs documentation auto-generated and linked

### Changed
- `Cargo.toml` metadata finalized for all crates
- `README.md` includes `cargo install` instructions
```

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
| `--config` | `-c` | `.reviewer.toml` | Path to configuration TOML file |
| `--no-cache` | | | Bypass response cache |
| `--help` | `-h` | | Display help |
| `--version` | `-V` | | Display version |

## Appendix C: Exit Codes

| Code | Meaning |
|---|---|
| `0` | Review completed successfully |
| `1` | Error occurred (API failure, parse error, etc.) |
| `1` | Local mode: review returned `REQUEST_CHANGES` (blocks commit) |

## Appendix D: Review State Logic

```
if verdict == "NEGATIVE" || security_issues > 0 || critical_bugs > 2:
    state = REQUEST_CHANGES
else if critical_bugs == 0 && security_issues == 0:
    state = APPROVE
else:
    state = COMMENT
```

If `REQUEST_CHANGES` or `APPROVE` fails due to GitHub permissions, fallback to `COMMENT`.
