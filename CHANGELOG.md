# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.3.0]

### Added

- **Per-provider `result_format` override** — `.reviewer.toml` now supports
  `result_format` under `[providers.<name>]`, allowing custom OpenAI-compatible
  endpoints to request formats such as `"json_object"`. Closes #77.
- **Dynamic `result_format` internals** — `ChatRequest.result_format` and
  `ProviderMeta.result_format` are now `Option<Cow<'static, str>>`, keeping the
  zero-cost static path for Qwen while supporting owned dynamic overrides.
- **Coverage reporting** — CI now runs `cargo llvm-cov` and uploads an LCOV
  report to Codecov; a coverage badge has been added to the README.

### Changed

- **DRY diff-fetch error handling** — `DiffTooLarge`, `EmptyDiff`, and generic
  fetch errors are now handled by a shared helper in `pipeline.rs`, reducing
  triplication across file, CI, and local diff sources.

### Improved

- Expanded test coverage for config edge cases (invalid temperature, thinking
  model token floors, TOML typo suggestions, provider switching, pricing
  overrides), redaction patterns (GitHub token variants, RSA keys, passwords),
  and verdict parsing (invalid verdict values, threshold issues, tag fallback
  variants).
- Blank `result_format` values in TOML are ignored so static provider defaults
  are preserved.
- Pipeline diff-fetch error handling now has unit and integration test coverage.
- Codecov upload is a required CI gate with patch coverage thresholds
  (`codecov.yml`).

## [1.2.4]

### Added

- ** Improved quality for thinking models like Kimi and DeepSeek


## [1.2.1]

### Fixed

- **Empty LLM responses on thinking models (DeepSeek v4, Kimi)** — HTTP 200 with
  0-char `content` no longer proceeds to verdict parsing. Empty assistant content
  is now a retryable `LlmApi` error (up to 3 retries with backoff). Default
  `max_tokens` for `deepseek` and `kimi` is raised to 16,384 when not explicitly
  configured, because thinking models share the output budget between
  `reasoning_content` and `content`. Cache writes are deferred until after a
  successful verdict parse, preventing poisoned cache entries in local mode.

### Added

- **Grok (xAI) provider** — first-class support via the generic client.
  Selectable with `--provider grok`, default model `grok-3`, env var
  `XAI_API_KEY`, base URL `https://api.x.ai/v1`. Approximate pricing arm in
  `default_pricing()`. Closes #74.
- **GLM (Zhipu AI) provider** — first-class support via the generic client.
  Selectable with `--provider glm`, default model `glm-4`, env var
  `ZHIPUAI_API_KEY`, base URL `https://open.bigmodel.cn/api/paas/v4`.
  Approximate pricing arm in `default_pricing()`. Closes #73.
- **`GenericOpenAiCompatibleClient`** (pub(crate)) — single data-driven
  implementation of the OpenAI `/chat/completions` flow shared by all
  providers, parameterized by `ProviderMeta`.
- **Provider metadata hooks** — `result_format` and `default_extra_headers`
  fields on `ProviderMeta` model the remaining per-provider differences
  (Qwen `result_format: "message"`, OpenRouter attribution headers) as data.
- **docs/GITHUB_BOT_SETUP.md** — dedicated machine-user / GitHub App identity
  guide for automated reviews (recommended over personal PATs), with
  fine-grained token scopes and storage guidance.
- **docs/PERFORMANCE.md** — binary-size baselines, runtime benchmarks,
  GitHub Actions cold-start tuning, and the caching lever.
- **Grok + GLM sections in docs/PROVIDERS.md** — full setup guides matching
  the style of the other providers, plus env var reference rows.
- **ExtraBody collision guard** — `apply_variant` now rejects ExtraBody keys
  that collide with standard `ChatRequest` fields (model, messages,
  temperature, max_tokens, result_format), preventing silent overwrites.
- **Cache key documentation** — `docs/PERFORMANCE.md` now documents all cache
  key components and explains how configuration changes affect cache behavior.
- **Machine specification in PERFORMANCE.md** — binary size baseline now
  includes machine details (Apple M1 Max, 32GB RAM, macOS 26.5.1).
- **Header override tests** — comprehensive tests for OpenRouter header
  override logic (replace defaults, append new headers, override both).

### Changed

- **Provider docs made provider-agnostic** — README, USAGE, CONFIGURATION,
  INSTALLATION, API, ARCHITECTURE, and local-review hook examples now show
  provider variety symmetrically instead of DeepSeek-only examples. DeepSeek
  remains the legitimate programmatic default.
- **docs/ARCHITECTURE.md** module graph updated to reflect the single
  `GenericOpenAiCompatibleClient` (no 5-client fan-out from the factory).
- **docs/API.md** custom-provider guide rewritten — adding a provider is now
  a `ProviderMeta` entry in `src/llm/providers.rs`, not a new client module.
- **`create_provider` factory** simplified from a ~80-line per-provider match
  to a single metadata-driven path (~20 lines).
- **http_referer warning** — changed from `log::warn!` to `eprintln!` to
  ensure users always see the warning when `http_referer` is set for a
  non-OpenRouter provider.

### Removed

- **Per-provider client modules** — `src/llm/{deepseek,kimi,qwen,openrouter,openai}.rs`
  and their `*Client` structs (`DeepSeekClient`, `KimiClient`, `QwenClient`,
  `OpenRouterClient`, `OpenAiClient`) are deleted with no shims or re-exports.
  Use `llm::factory::create_provider` for all providers. This is a breaking
  change for library consumers who constructed clients directly.
- **`with_http_referer` builder method** — OpenRouter referer override is now
  handled at factory construction time via `ProviderConfig.http_referer`
  (merged into the client's default headers). Same effective HTTP behavior.
- **Hardcoded `"openai"` provider name** — `name()` now returns
  `ProviderMeta::name` dynamically.
- **Tech-debt comment** in `factory.rs` calling for this consolidation.

### Deprecated

- n/a

### Fixed

- n/a

---

## [1.2.3] - 2026-06-21

### Upgrade Notes

- **New timeout configuration**: You can now control LLM request timeout via `--llm-timeout`, `RS_GUARD_LLM_TIMEOUT`, or `llm_timeout_secs` in `.reviewer.toml`.
- **Improved defaults for thinking models**: When using `deepseek` (including `deepseek-v4-pro`) or `kimi` without an explicit timeout, rs-guard now defaults to **180 seconds** (previously 120s globally, 60s before v1.2.3). This greatly reduces flakiness on reasoning-heavy models.
- **deepseek-v4-pro users**: We strongly recommend either:
  - Using `--variant pro` (preferred), or
  - Explicitly setting `max_tokens` (≥16384) and `llm_timeout_secs` (≥180) for complex PRs.
- Existing configurations continue to work. The new auto-raise only applies when you have not set a value.

### Added

- **Configurable LLM request timeout** — `RS_GUARD_LLM_TIMEOUT` (env), `llm_timeout_secs` (TOML), and `--llm-timeout` (CLI). Default raised to 120 seconds (from 60) to support thinking models whose reasoning phase can take longer. The timeout is a total request timeout and is honored for all providers via the generic client.
- `DEFAULT_LLM_TIMEOUT_SECS` constant (120) and corresponding field on `Config` / `ProviderConfig`.
- **Auto-raised LLM timeout for thinking providers** — `deepseek` and `kimi` now automatically use a minimum of 180s when no explicit `llm_timeout_secs` / `RS_GUARD_LLM_TIMEOUT` is provided (mirrors the existing `max_tokens` floor logic). Introduces `THINKING_MIN_LLM_TIMEOUT_SECS`.

### Changed

- `build_llm_client` and `GenericOpenAiCompatibleClient::new` now accept an explicit timeout (falls back to default when not provided in `ProviderConfig`).
- All internal HTTP clients for LLM calls respect the configured timeout.
- Default timeout for `deepseek` (including `deepseek-v4-pro`) and `kimi` is now 180s when unset.

### Fixed

- Empty/null `content` with `reasoning_content` on thinking models is still treated as a retryable error (status 0). Cache writes remain deferred until after successful verdict parse.
- Improved test coverage for reasoning content stripping (final content returned never includes internal `reasoning_content`).

### Documentation

- Added comprehensive **deepseek-v4-pro** guide in `docs/PROVIDERS.md`:
  - Full examples for CLI (`--variant pro --max-tokens 16384 --llm-timeout 180`), environment variables, and TOML (top-level + `[providers.deepseek]`).
  - Recommended settings and best practices for reasoning models.
  - Complete GitHub Actions example (matching real CI usage patterns with `timeout-minutes` and `continue-on-error` advice).
- Updated CONFIGURATION.md (field tables + example) and USAGE.md (CLI table + examples) with deepseek-v4-pro usage and the new auto-raised timeout behavior.
- Enhanced troubleshooting for "Empty assistant content" and slow thinking-model responses.
- Added precedence rules and CI reliability guidance for `deepseek-v4-pro`.

### Known Issues

- Extremely large or complex diffs using `deepseek-v4-pro` may still occasionally require manually raising `--llm-timeout` / `llm_timeout_secs` above 180s (e.g. 240–300). The auto-raise is a safety net, not a guarantee for every workload.
- No other known issues at time of release.

---

## [1.2.2] - 2026-06-21

### Fixed

- **DeepSeek/Kimi thinking-model responses** — loose JSON parsing for
  `/chat/completions` bodies: tolerates `"content": null`, multimodal content
  arrays, and extra choice fields (previously caused `Failed to parse response:
  error decoding response body`). Empty or null assistant `content` is a
  retryable `LlmApi` error. Default `max_tokens` for `deepseek` and `kimi`
  rises to 16,384 when not explicitly configured. Cache writes occur only after
  a successful verdict parse.

---

## [1.1.0] - 2026-06-17

### Added

- **Generic model variant mechanism** — `VariantEffect` (ModelAlias + ExtraBody),
  `ProviderVariant`, `ProviderMeta`, `all_providers()`, `find_provider_variant()`,
  and `provider_variant_names()` in `llm::providers`. A centralized `apply_variant()`
  helper (pub(crate)) resolves effective model + extra request body fields.
- **`ChatRequest.extra_body`** — `HashMap` merged via `#[serde(flatten)]` (with `default`)
  so `ExtraBody` variants can inject arbitrary top-level fields (e.g. Kimi's `thinking` object).
- **Kimi thinking mode variants** (`thinking-on` / `thinking-off`) using `ExtraBody`
  to send `{"thinking": {"type": "enabled"}}` / `disabled`. Preserves existing
  `reasoning_content` handling.
- **`ProviderConfig.variant`** and `with_variant()` builder on all `*Client` types.
- **CLI / config / env support** for `--variant` / `variant` / `RS_GUARD_VARIANT`.
- **Documentation** — Expanded `PROVIDERS.md` (per-provider variant tables), `API.md`,
  `CONFIGURATION.md`, `USAGE.md`, `implementation-guide.md` (custom provider example now
  shows `extra_body`).

### Changed

- Provider clients now route through `apply_variant()` for consistent model + extra
  handling.

## [1.0.2] - 2026-06-15

### Added

- **Helpful TOML configuration error messages** — `load_toml_config()` now parses the raw TOML
  structure first and detects common mistakes, emitting actionable guidance instead of raw
  serde/toml errors.
- **SHA-256 checksums for release binaries** — The release workflow now generates
  `rs-guard-x86_64-unknown-linux-gnu.sha256` and uploads it alongside the binary.

### Fixed

- **Issue #63** — Using `[provider.deepseek]` (singular table) now produces a clear error that
  explains `provider` must be a string and shows the correct plural form `[providers.deepseek]`.
- **Issue #64** — Unknown top-level keys (e.g., `providor`) now produce a helpful message that
  suggests the closest valid key and lists all accepted top-level keys.
- **Non-string `provider` values** — `provider = 123` now reports that `provider` must be a
  string with an example of the correct syntax.
- **AI Code Review workflow 404 failure** — `.github/workflows/ai-review.yml` now downloads the
  correctly named release asset (`rs-guard-x86_64-unknown-linux-gnu`) and verifies its SHA-256
  checksum instead of the old `rs-guard` filename that did not exist.
- **Documentation/implementation mismatch** — `docs/INSTALLATION.md`, `docs/USAGE.md`,
  `docs/implementation-guide.md`, and `examples/github-actions-workflow/README.md` now reflect
  the actual release asset name and the Linux-x86_64-only pre-built binary policy.
- **Node.js 20 deprecation warnings** — All workflows and documented examples now pin
  `actions/checkout@v5` and `actions/upload-artifact@v5` (or their SHA-pinned equivalents) to
  avoid the upcoming Node.js 20 removal.

## [1.0.1] - 2026-06-15

### Fixed

- **`.gitignore` cache entry** — `ensure_gitignored()` now writes the cache directory as a
  git-root-relative path (e.g. `.rs-guard/cache`) instead of the absolute filesystem path.
- **Duplicate `.gitignore` entries** — `ensure_gitignored()` now normalizes existing lines
  (trim whitespace, ignore trailing `/`) before checking for an existing entry, preventing
  repeated appends on every run.

## [1.0.0] - 2026-06-11

### Added

- **Five-axis review system** — `DEFAULT_PROMPT` now directs the LLM across five structured
  review axes: Correctness, Security, Performance, Maintainability, and Test Coverage
- **Four-level severity taxonomy** — `[Critical]`, `[Security]`, `[Important]`, `[Suggestion]`
  replace the old binary critical/non-critical model; each level has defined merge implications
- **`important_issues` field** on `Verdict` struct — counts `[Important]`-tagged findings
- **`suggestions` field** on `Verdict` struct — counts `[Suggestion]`-tagged findings (advisory only)
- **`IMPORTANT_ISSUES_THRESHOLD` constant** (`3`) — configures when accumulated important issues
  escalate from COMMENT to REQUEST_CHANGES
- **Language-agnostic example prompt library** (`examples/prompts/`):
  - `general-code-review.md` — language/framework-agnostic template (mirrors `DEFAULT_PROMPT`)
  - `backend-api.md` — REST/GraphQL API focused template
  - `frontend-spa.md` — SPA/component framework focused template
  - `cli-tooling.md` — CLI tool and systems programming focused template
- **`--dry-run` CLI flag** — run the full pipeline without submitting reviews or blocking commits
- **`cache_dir` config field** — custom cache directory path
- **`circuit_breaker` config field** — optional circuit breaker configuration
- **`pricing` config field** — per-provider pricing overrides
- **`chunk_head_lines` and `chunk_tail_lines` config fields** — diff chunking control
- **`auto_gitignore` config option** — control `.gitignore` auto-modification behavior
- **Hybrid ASCII/non-ASCII token estimation** (`estimate_tokens`) for more accurate cost estimates
- **User-facing progress indicators** in local mode (🤖 before LLM call, ✅ after)
- **Per-provider `context_window` metadata** in `ProviderMeta` (64K–128K tokens)
- **Token limit warning** when estimated tokens exceed 80% of provider context window

### Changed

- **`determine_review_state` logic** extended with a three-tier decision tree:
  - `NEGATIVE` verdict, any `[Critical]` or `[Security]` issue → `REQUEST_CHANGES`
  - `important_issues >= 3` → `REQUEST_CHANGES`
  - `important_issues` 1–2 → `COMMENT` (advisory, not blocked)
  - Otherwise → `APPROVE`
- **Metadata block format** updated to include four severity-count fields (plus the existing
  `Verdict` line):

  ```text
  [RS_GUARD_VERDICT_METADATA]
  Verdict: POSITIVE
  CriticalIssues: <count>
  SecurityIssues: <count>
  ImportantIssues: <count>
  Suggestions: <count>
  ```

- `ensure_gitignored()` now returns `Result` instead of silently logging warnings
- `ensure_gitignore()` now writes `.gitignore` at the git repository root (via `find_git_root()`)
  instead of the current working directory
- `CacheConfig` includes `auto_gitignore` field (default: `true`)
- Example GitHub Actions workflows pinned to `v1.0.0` release (previously used `latest`)
- All documentation updated to reflect five-axis review, four-field metadata block, and the
  new prompt template library; framework-specific inline examples removed from `docs/USAGE.md`
- **BREAKING:** `ReviewMetrics::estimated_cost_cents` changed from `u64` to `f64` to avoid
  integer truncation for small diffs. Consumers parsing `rs-guard-metrics.json` must update
  their type expectations.

### Removed / Deprecated

- **`CriticalBugs`** metadata field — replaced by `CriticalIssues`. The parser accepts both for
  one release cycle for backward compatibility; `CriticalBugs` will be removed in `v1.1.0`.
- Framework-specific inline prompt templates (React/TypeScript, Rails) removed from
  `docs/USAGE.md`; use the templates in `examples/prompts/` instead.

## [0.7.1] - 2026-06-08

### Added

- Fixed markdown formatting in all documentation files

## [0.7.0] - 2026-06-08

### Added

- 33 new tests: `fetch_local_diff`, `load_prompt_file`, `validate_for_ci` edge cases, CRLF
  chunk diffing, `is_retryable`/`is_permission_denied` unit tests, corrupted cache file tests
- Auto-creation of parent directories before artifact and metrics file writes
- Temperature validation for env/TOML sources (was previously only validated for CLI)

### Changed

- CI pipeline now uses `cargo run --release --quiet` for binary execution (resilient to
  package name changes on base branch)
- Factory error message derived from `providers.rs` metadata (single source of truth)
- `ReviewMetrics.tokens_in/out` renamed to `estimated_tokens_in/out` for clarity
- `main.rs` error handling DRY'd with `exit_on_error()` helper
- Release workflow: added `publish-crates` job for automatic crates.io publishing via
  `CRATES_TOKEN` secret

### Improved

- **Benchmarks:** `parse_metadata_block` is 2x faster (635ns → 326ns) and
  `parse_large_response` (10KB) is 10x faster (3.86µs → 361ns) by replacing the
  regex-based metadata parser with manual substring scanning
- Cache temp files use a monotonic counter for uniqueness (prevents concurrent write
  collisions on macOS)
- Error response bodies preserved when diagnostic info is available (replaced
  `unwrap_or_default()` with readable fallback text)
- CI-mode `unwrap()` calls replaced with
  `expect("validated in validate_for_ci()")` for clear failure messages

### Fixed

- 5 `cargo doc` intra-doc link warnings (private const references in public docs)
- Dead test fixture removed (`tests/test_data/sample_diff.diff`)
- Wrong metadata marker in test data (`DIFFGUARD` → `RS_GUARD`)
- Trailing-slash inconsistency in GitHub API URL construction
- Silent DeepSeek model fallback replaced with explicit `expect()` (guaranteed to succeed
  due to earlier validation)
- Missing `# Errors` docs on provider `new()` methods (kimi, qwen, openrouter, openai)

## [0.6.0] — 2026-06-30

### Added

- Registered on crates.ai for project discovery
- Published to crates.io: `cargo install rs-guard`
- docs.rs documentation auto-generated and linked

### Changed

- `Cargo.toml` metadata finalized with `documentation`, `readme`, and `devops` keyword
- `README.md` includes `cargo install` instructions and updated badges
- Phase tracking changelog entries for all phases (0.1.0 through 0.6.0)

## [0.5.0] — 2026-06-30

### Added

- Phase 5: Library extraction readiness (single crate remains, workspace deferred)
- All public APIs documented and tested
- Benchmark suite for verdict parsing performance

## [0.4.0] — 2026-06-28

### Added

- Phase 4: Documentation polish
- `docs/ARCHITECTURE.md` — System design with Mermaid diagrams, pipeline explanation, provider trait guide, security model
- `docs/USAGE.md` — Full CLI reference, exit codes, GitHub Actions guide, pre-commit setup, `.reviewer.toml` schema, troubleshooting
- `docs/API.md` — Module API documentation, key types reference, custom provider implementation guide
- README.md comprehensive rewrite with Phase 3 features, updated architecture diagram, and Mermaid pipeline overview
- CHANGELOG.md versioned entries for all phases

## [0.3.0] — 2026-06-28

### Added

- **Response caching** (`src/cache.rs`): SHA-256 keyed LLM response cache in `.rs-guard/cache/`
  - Cache key combines diff content, prompt, provider, model, and temperature — all parameters matter
  - Timestamps stored in file content (line 1), not mtime — reliable across clock changes and file copies
  - Atomic writes via temp-file-then-rename — prevents partial reads by concurrent processes
  - Configurable TTL (default: 24 hours) and max size (default: 100 MB) with LRU cleanup
  - Auto-adds `.rs-guard/cache/` to `.gitignore` on first use
  - `--no-cache` flag to bypass cache and force a fresh LLM API call
  - 13 inline unit tests
- **Metrics export** (`rs-guard-metrics.json`): per-run JSON artifact with token counts, latency, cost estimate, verdict, and state
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
- `pipeline.rs`: metrics collected and written to `rs-guard-metrics.json` on every run
- `pipeline.rs`: `PipelineResult` enum replaces `process::exit()` — enables integration testing without process termination
- `output.rs` print functions refactored to `impl Write` parameter — enables buffer-based testing
- `output.rs`: added `write_metrics()` for JSON metrics artifact
- `diff.rs`: `chunk_diff()` integrated into pipeline before LLM call
- `http.rs`: shared `build_github_http_client()` and `github_diff_headers()` helpers eliminate boilerplate
- `#![deny(missing_docs)]` enforced at crate level — all public items documented

### Fixed

- `github.rs`: review submission now falls back to `COMMENT` when `APPROVE`/`REQUEST_CHANGES` is rejected by GitHub permissions (403)
- Cache gitignore: uses exact line matching to avoid duplicating entries with similar paths

## [0.2.0] — 2026-06-28

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
- Provider-specific environment variable support (`KIMI_API_KEY`,
  `DASHSCOPE_API_KEY`, `OPENROUTER_API_KEY`, `OPENAI_API_KEY`)
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

## [0.1.0] — 2026-06-27

### Added

- Initial release with DeepSeek provider support (`deepseek-v4-flash`)
- GitHub Actions integration: fetches PR diffs and submits review states
- In-memory verdict parsing (`[RS_GUARD_VERDICT_METADATA]` block)
- Three review states: `APPROVE`, `REQUEST_CHANGES`, `COMMENT`
- Permission fallback: downgrades to `COMMENT` when approval/rejection is not permitted
- Dismissal of previous rs-guard `CHANGES_REQUESTED` reviews (identified by `<!-- rs-guard-bot -->` HTML comment signature) when new state is non-blocking
- `review-result.txt` artifact for downstream jobs
- Embedded default prompt (works out-of-the-box; override via `--prompt-file`)
- `--model` and `--temperature` CLI flags
- Single crate architecture (lean MVP)
- Basic retry logic for transient API failures (429, 502, 503, 504, timeouts)
- Comprehensive test suite (unit + integration) with mock HTTP servers
- CI pipeline: format, clippy, test, doc coverage, release build
- `cargo-deny` license and security auditing
