# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] — 2026-07-XX

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
- CI pipeline: format, clippy, test, coverage, doc coverage, release build
- `cargo-deny` license and security auditing
