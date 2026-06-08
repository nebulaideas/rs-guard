# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Ongoing work and future enhancements

## [0.6.0] — 2026-06-XX

### Added

- Registered on crates.ai for project discovery
- Published to crates.io: `cargo install diffguard`
- docs.rs documentation auto-generated and linked

### Changed

- `Cargo.toml` metadata finalized with `documentation`, `readme`, and `devops` keyword
- `README.md` includes `cargo install` instructions and updated badges
- Phase tracking changelog entries for all phases (0.1.0 through 0.6.0)

## [0.5.0] — 2026-06-XX

### Added

- Phase 5: Library extraction readiness (single crate remains, workspace deferred)
- All public APIs documented and tested
- Benchmark suite for verdict parsing performance

## [0.4.0] — 2026-06-XX

### Added

- Phase 4: Documentation polish
- `docs/ARCHITECTURE.md` — System design with Mermaid diagrams, pipeline explanation, provider trait guide, security model
- `docs/USAGE.md` — Full CLI reference, exit codes, GitHub Actions guide, pre-commit setup, `.reviewer.toml` schema, troubleshooting
- `docs/API.md` — Module API documentation, key types reference, custom provider implementation guide
- README.md comprehensive rewrite with Phase 3 features, updated architecture diagram, and Mermaid pipeline overview
- CHANGELOG.md versioned entries for all phases

## [0.3.0] — 2026-06-XX

### Added

- **Response caching** (`src/cache.rs`): SHA-256 keyed LLM response cache in `.diffguard/cache/`
  - Cache key combines diff content, prompt, provider, model, and temperature — all parameters matter
  - Timestamps stored in file content (line 1), not mtime — reliable across clock changes and file copies
  - Atomic writes via temp-file-then-rename — prevents partial reads by concurrent processes
  - Configurable TTL (default: 24 hours) and max size (default: 100 MB) with LRU cleanup
  - Auto-adds `.diffguard/cache/` to `.gitignore` on first use
  - `--no-cache` flag to bypass cache and force a fresh LLM API call
  - 13 inline unit tests
- **Metrics export** (`diffguard-metrics.json`): per-run JSON artifact with token counts, latency, cost estimate, verdict, and state
  - CI summary printed to stdout: provider, model, tokens in/out, latency, estimated cost, diff lines, verdict, state
  - Cost estimation in integer cents (avoids floating point precision issues)
- **Error recovery** (`src/retry.rs`): exponential backoff retry + optional circuit breaker
  - Exponential backoff: 1s, 2s, 4s base delays with ±25% jitter, up to 3 retries
  - Circuit breaker: simple Closed/Open two-state (no half-open), opt-in, default disabled
  - Thread-safe: `Arc<Mutex<>>` internal state, safe for concurrent async tasks
  - 20 inline tests covering retry, circuit breaker, thread safety, and auto-reset
- **Diff chunking** (`diff.rs`): preserves first 50 and last 50 lines when diff exceeds threshold
  - Uses `Cow<str>` return type — zero allocation when no truncation is needed
  - Truncation warning shown in both CI (review body prefix) and local (stderr) modes
  - Placeholder line shows exact count of omitted lines
- **Enhanced CI pipeline**:
  - `cargo deny check` for license and dependency auditing
  - `cargo audit` for vulnerability scanning
  - Benchmark job (`cargo bench --bench verdict -- --quick`) runs on main branch pushes
  - `Swatinem/rust-cache@v2` dependency caching across all CI jobs
  - All GitHub Actions pinned to commit SHAs for supply-chain security
- `docs-deploy.yml` workflow: deploys `cargo doc` output to GitHub Pages on main branch pushes
- `benches/verdict.rs`: 5 Criterion benchmarks for verdict parsing (simple, complex, multiline, no-block, large)
- `tests/test_data/`: sample diffs and LLM responses for integration tests
- `tests/github_tests.rs`: 13 wiremock-backed tests for GitHub API review submission
- Full pipeline integration tests (`tests/integration_tests.rs`): 5 end-to-end scenarios with mock servers

### Changed

- `pipeline.rs`: cache check inserted before LLM call; response cached after successful LLM call
- `pipeline.rs`: metrics collected and written to `diffguard-metrics.json` on every run
- `pipeline.rs`: `PipelineResult` enum replaces `process::exit()` — enables integration testing without process termination
- `output.rs` print functions refactored to `impl Write` parameter — enables buffer-based testing
- `output.rs`: added `write_metrics()` for JSON metrics artifact
- `diff.rs`: `chunk_diff()` integrated into pipeline before LLM call
- `http.rs`: shared `build_github_http_client()` and `github_diff_headers()` helpers eliminate boilerplate
- `#![deny(missing_docs)]` enforced at crate level — all public items documented

### Fixed

- `github.rs`: review submission now falls back to `COMMENT` when `APPROVE`/`REQUEST_CHANGES` is rejected by GitHub permissions (403)
- Cache gitignore: uses exact line matching to avoid duplicating entries with similar paths

## [0.2.0] — 2026-06-XX

### Added

- Kimi (Moonshot AI) provider support with `kimi-k2.5` default model
- Qwen (Alibaba Cloud) provider support with `qwen-plus` default model
- OpenRouter provider support with unified gateway routing
- Generic OpenAI-compatible provider for custom endpoints
- `LlmProvider` async trait with `Box<dyn LlmProvider>` dynamic dispatch
- Provider factory with `ProviderConfig` for TOML-driven base URL, HTTP referer, and max tokens overrides
- `.reviewer.toml` configuration file support with per-provider sections
- `--config` / `-c` CLI flag for custom config file path
- `--max-tokens` CLI flag for limiting LLM completion length
- Configuration resolution: CLI flags > env vars > TOML file > defaults
- `reasoning_content` field support in chat completion responses (logged at debug level)
- Shared `send_chat_request` helper eliminating HTTP boilerplate across providers
- Local pre-commit mode: analyzes `git diff --cached` and prints colored terminal output
- Commit blocking: aborts commit when review returns `REQUEST_CHANGES`
- Provider-specific environment variable support (`KIMI_API_KEY`, `DASHSCOPE_API_KEY`, `OPENROUTER_API_KEY`, `OPENAI_API_KEY`)
- Per-provider default model selection in configuration
- Custom `api_key_env` override per provider in `.reviewer.toml`
- `docs/PROVIDERS.md` — Per-provider setup guide with API key acquisition instructions
- `docs/CONFIGURATION.md` — Complete `.reviewer.toml` reference
- `docs/LOCAL_MODE.md` — Pre-commit hook setup and local usage guide
- `examples/local-review/pre-commit-hook.sh` — Example git hook script

### Changed

- `src/llm/` restructured with provider-per-module pattern
- `Provider` enum refactored to `Box<dyn LlmProvider>` trait object
- All provider `chat_completion` implementations delegated to shared `send_chat_request` helper
- Qwen provider uses typed `QwenChatRequest` struct instead of `serde_json::json!` macro
- `OpenRouterClient::with_http_referer` now returns `Result` instead of silently swallowing errors
- CLI `--model`, `--temperature`, `--provider` changed to `Option<T>` for reliable override detection
- `Config::from_env()` now accepts optional `TomlConfig` for layered resolution
- `Config::apply_args()` uses `Option` fields to distinguish explicit CLI overrides from defaults
- Unknown provider names now return `Config` error instead of silently falling back to DeepSeek
- `src/config.rs` extended with `standard_api_key_env_var()` (returns `Result`) and `default_model()` mappings for all providers

### Fixed

- Pre-commit hook `set -e` bug that made exit-code-2 handling dead code
- TOML per-provider `base_url`, `http_referer`, and `api_key_env` settings now correctly wired to provider clients
- CLI argument override detection no longer compares against hardcoded clap defaults

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
- Single crate architecture (lean MVP)
- Basic retry logic for transient API failures (429, 502, 503, 504, timeouts)
- Comprehensive test suite (unit + integration) with mock HTTP servers
- CI pipeline: format, clippy, test, doc coverage, release build
- `cargo-deny` license and security auditing
