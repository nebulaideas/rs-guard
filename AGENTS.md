# rs-guard — Agent Guide

> Current state of the `rs-guard` repository for AI coding agents.

---

## Project Overview

**rs-guard** is a Rust-based AI code review CLI tool. It fetches Pull Request diffs from GitHub, sends them to an LLM provider for review, parses a structured verdict from the response, and submits the review state (`APPROVE`, `REQUEST_CHANGES`, or `COMMENT`) back to GitHub — all in a single execution.

**Current Status:** Phases 1–7 are complete; v1.3.0 is in progress on branch `feature/77-dynamic-result-format`. The crate is published on crates.io and registered on crates.ai.

**Variant Feature Track (issues #65–#68, PR #70, merged 2026-06-17):** Generic `VariantEffect` (ModelAlias + ExtraBody) support added, with DeepSeek flash/pro and first ExtraBody use for Kimi thinking-on/off. Full CLI/config/env support, integration test coverage, and docs. Released as v1.1.0. See `docs/PROVIDERS.md` and the feature branch history.

**Client Extraction (v1.2, issue #72):** The 5 duplicated per-provider clients (deepseek/kimi/qwen/openrouter/openai) were replaced by a single data-driven `GenericOpenAiCompatibleClient` (pub(crate)) parameterized by `ProviderMeta`. Grok (xAI) and GLM (Zhipu AI) became first-class. Provider-agnostic documentation pass + new bot-setup and performance guides. Released as v1.2.0.

**Dynamic `result_format` (v1.3, issue #77):** `ChatRequest.result_format` and `ProviderMeta.result_format` moved to `Option<Cow<'static, str>>` to keep the zero-cost static path while supporting per-provider TOML overrides. Added a Codecov coverage job and badge, DRY diff-fetch error handling, and expanded config/redact/verdict test coverage.

- **Repository:** `git@github.com:nebulaideas/rs-guard.git`
- **Current Branch:** `feature/77-dynamic-result-format` (v1.3.0); `main` for releases
- **License:** MIT License (Copyright 2026 Nebula Ideas)
- **Language:** Rust (edition 2021, toolchain 1.82+)
- **Crate:** [rs-guard on crates.io](https://crates.io/crates/rs-guard) | [docs.rs](https://docs.rs/rs-guard)

---

## Technology Stack

| Layer           | Technology                                        |
| --------------- | ------------------------------------------------- |
| Language        | Rust (edition 2021, toolchain 1.82+)              |
| Build Tool      | Cargo (single crate)                              |
| Async Runtime   | Tokio                                             |
| HTTP Client     | reqwest (rustls-tls)                              |
| CLI Framework   | clap (derive macros)                              |
| Serialization   | serde, serde_json, toml                           |
| Error Handling  | thiserror, anyhow                                 |
| Terminal Output | colored                                           |
| Testing         | Built-in test framework + wiremock (HTTP mocking) |
| URL Validation  | url crate                                         |
| Secrets         | env vars + redact module                          |
| Hashing         | sha2 + hex (cache keys)                           |

### Implemented LLM Providers

| Provider             | Status     | Default Model        | API Key Env          |
| -------------------- | ---------- | -------------------- | -------------------- |
| DeepSeek             | ✅ Phase 1 | `deepseek-v4-flash`  | `DEEPSEEK_API_KEY`   |
| Kimi (Moonshot AI)   | ✅ Phase 2 | `kimi-k2.5`          | `KIMI_API_KEY`       |
| Qwen (Alibaba Cloud) | ✅ Phase 2 | `qwen-plus`          | `DASHSCOPE_API_KEY`  |
| OpenRouter           | ✅ Phase 2 | `openai/gpt-4o-mini` | `OPENROUTER_API_KEY` |
| OpenAI               | ✅ Phase 2 | `gpt-4o-mini`        | `OPENAI_API_KEY`     |
| Grok (xAI)           | ✅ Phase 7 | `grok-3`             | `XAI_API_KEY`        |
| GLM (Zhipu AI)       | ✅ Phase 7 | `glm-4`              | `ZHIPUAI_API_KEY`    |

All 7 providers are served by a single `GenericOpenAiCompatibleClient` (pub(crate)) parameterized by `ProviderMeta`. Per-provider differences (Qwen `result_format`, OpenRouter attribution headers) are expressed as metadata fields, not per-client code.

---

## Repository Structure

```text
rs-guard/
├── src/                           # Single crate source (16 modules)
│   ├── main.rs                    # CLI entry point (thin)
│   ├── lib.rs                     # Library root
│   ├── pipeline.rs                # Orchestration + PipelineResult
│   ├── cache.rs                   # LLM response caching (SHA-256 keyed)
│   ├── cli.rs                     # Clap argument parsing
│   ├── config.rs                  # Env vars + .reviewer.toml parsing
│   ├── diff.rs                    # PR diff fetching + local diff + chunking
│   ├── error.rs                   # RsGuardError enum
│   ├── github.rs                  # GitHub API review submission
│   ├── http.rs                    # HTTP utilities + URL validation
│   ├── llm/                       # LLM provider modules
│   │   ├── mod.rs                 # LlmProvider trait + shared types
│   │   ├── generic_client.rs      # GenericOpenAiCompatibleClient (all providers)
│   │   ├── factory.rs             # Provider factory (metadata-driven)
│   │   └── providers.rs           # Centralized provider metadata + variants
│   ├── output.rs                  # Terminal output + artifact + metrics writing
│   ├── redact.rs                  # Secret redaction
│   ├── retry.rs                   # Retry logic + circuit breaker
│   └── verdict.rs                 # Verdict parsing + review state
├── benches/
│   └── verdict.rs                 # Criterion benchmarks (5 scenarios)
├── tests/
│   ├── test_data/                 # Sample diffs + LLM responses
│   ├── config_tests.rs            # 21 config tests
│   ├── diff_tests.rs              # 12 diff tests (wiremock + inline)
│   ├── github_tests.rs            # 13 github tests (wiremock)
│   ├── integration_tests.rs       # 5 full pipeline tests (wiremock)
│   ├── provider_tests.rs          # 22 provider tests (wiremock)
│   └── verdict_tests.rs           # 15 verdict tests
├── examples/
│   ├── github-actions-workflow/   # Sample CI workflows
│   └── local-review/              # Pre-commit hook examples
├── docs/
│   ├── MVP_IMPLEMENTATION_PLAN.md # Implementation roadmap
│   ├── ARCHITECTURE.md            # System design + Mermaid diagrams
│   ├── USAGE.md                   # Full CLI reference + troubleshooting
│   ├── API.md                     # Module API docs + custom provider guide
│   ├── PROVIDERS.md               # Per-provider setup guide
│   ├── CONFIGURATION.md           # .reviewer.toml reference
│   ├── LOCAL_MODE.md              # Pre-commit hook setup
│   ├── GITHUB_BOT_SETUP.md        # Dedicated GitHub bot/machine-user setup
│   └── PERFORMANCE.md             # Binary size + runtime perf baselines
├── .github/workflows/
│   ├── ci.yml                     # CI pipeline (format, lint, test, deny, audit, bench)
│   ├── docs-deploy.yml            # GitHub Pages docs deployment
│   ├── release.yml                # Release pipeline
│   └── ai-review.yml              # Sample AI review workflow
├── Cargo.toml / Cargo.lock
├── deny.toml                      # cargo-deny config
├── .rustfmt.toml                  # Formatting config
├── .gitignore
├── README.md
├── CHANGELOG.md
└── LICENSE                        # MIT
```

---

## Key Architecture Decisions

| Decision                 | Choice                                                                                     |
| ------------------------ | ------------------------------------------------------------------------------------------ |
| Crate structure          | Single crate (workspace deferred until library demand emerges)                             |
| Provider dispatch        | `Box<dyn LlmProvider>` trait objects (refactored from enum dispatch in Phase 1)            |
| Provider client        | Single `GenericOpenAiCompatibleClient` (pub(crate)) parameterized by `ProviderMeta`; per-provider differences are metadata (v1.2) |
| Exit signal              | `PipelineResult` enum (Success / ReviewBlocked) — not `process::exit()` in library code    |
| SSRF protection          | URL allowlist per provider in CI mode; loopback allowed in local mode                      |
| Print functions          | Accept `impl Write` for testability                                                        |
| `Config::empty()`        | Test-only constructor for integration tests                                                |
| `#![deny(missing_docs)]` | Enforced at crate level                                                                    |
| Cache keying             | SHA-256 over (diff \| prompt \| provider \| model \| temperature) — all parameters matter  |
| Cache timestamps         | Stored in file content (line 1), not mtime — reliable across clock changes and file copies |
| Cache size limit         | 100 MB default with LRU cleanup — prevents unbounded disk usage                            |
| Circuit breaker          | Simple Closed/Open only (no half-open), opt-in, default disabled                           |
| Cost calculation         | Integer cents, not floating point — avoids precision issues                                |
| Diff chunking            | `Cow<str>` return — zero allocation when no truncation needed                              |

---

## Build and Test Commands

```bash
# Build
cargo build

# Full test suite (~267 tests)
cargo test

# Lint (zero warnings required)
cargo clippy --all-targets --all-features -- -D warnings

# Format check
cargo fmt --all -- --check

# Documentation
cargo doc --no-deps --open

# Benchmarks
cargo bench --bench verdict -- --quick

# Security audits
cargo deny check
cargo install cargo-audit --locked  # one-time setup
cargo audit
```

---

## Test Coverage

| Module        | Test Count                        | Type               |
| ------------- | --------------------------------- | ------------------ |
| `verdict.rs`  | 56 (22 inline + 34 integration)   | Unit + Integration |
| `config.rs`   | 49                                | Integration        |
| `github.rs`   | 19                                | Inline (wiremock)  |
| `output.rs`   | 11                                | Inline             |
| `cache.rs`    | 31                                | Inline             |
| `retry.rs`    | 17 (6 retry + 11 circuit breaker) | Inline             |
| `provider*`   | 74 (45 inline + 29 integration)   | Unit + Integration |
| `diff.rs`     | 40 (35 inline + 5 integration)    | Unit + Integration |
| `redact.rs`   | 15                                | Inline             |
| `pipeline.rs` | 37 (21 inline + 16 integration)   | Unit + Integration |
| `http.rs`     | 18                                | Inline             |
| `cli.rs`      | 3                                 | Inline             |
| **Total**     | **~450**                          |                    |

---

## Phase 3 Status — ✅ Complete

### Pre-requisite Cleanup (Phase 0)

| Task                                                | Status                                    |
| --------------------------------------------------- | ----------------------------------------- |
| P0.1 — Remove `process::exit` from `run_pipeline()` | ✅ Done                                   |
| P0.2 — `github.rs` test suite (13 tests)            | ✅ Done                                   |
| P0.3 — `output.rs` `impl Write` refactor + tests    | ✅ Done                                   |
| P0.4 — `#![deny(missing_docs)]`                     | ✅ Done                                   |
| P0.5 — Update AGENTS.md                             | ✅ Done (this file)                       |
| P0.6 — DRY diff-fetch error handling                | ✅ Done — shared `handle_diff_fetch_error` helper in `pipeline.rs` |
| P0.7 — Shared HTTP client builder                   | ✅ Done                                   |
| P0.8 — `tests/test_data/` directory                 | ✅ Done                                   |
| P0.9 — Full pipeline integration test (5 scenarios) | ✅ Done                                   |

### Advanced Features

| Task                                                         | Status                                                                     |
| ------------------------------------------------------------ | -------------------------------------------------------------------------- |
| 3.1 — Response caching (`src/cache.rs`, `.rs-guard/cache/`)  | ✅ Done — 13 inline tests, SHA-256 keyed, TTL+size limit, atomic writes    |
| 3.2 — Metrics export (`rs-guard-metrics.json`)               | ✅ Done — `ReviewMetrics` struct, `write_metrics()`, per-run JSON artifact |
| 3.3 — Error recovery (exp backoff + circuit breaker)         | ✅ Done — `with_retry`, `CircuitBreaker`, 20 inline tests, thread-safe     |
| 3.4 — Diff chunking (50/50 head/tail, `Cow<str>`)            | ✅ Done — integrated in pipeline, warning shown in both CI and local modes |
| 3.5 — Enhanced CI pipeline (deny, audit, bench, docs-deploy) | ✅ Done — `ci.yml` + `docs-deploy.yml`, `benches/verdict.rs`               |

---

## Phase 4 Status — ✅ Complete

### Documentation Polish

| Task                                                      | Status  |
| --------------------------------------------------------- | ------- |
| 4.1 — Update AGENTS.md                                    | ✅ Done |
| 4.2 — Update CHANGELOG.md (0.1.0–0.3.0 + [Unreleased])    | ✅ Done |
| 4.3 — README.md rewrite (keep logo, add Phase 3 features) | ✅ Done |
| 4.4 — `docs/ARCHITECTURE.md` (Mermaid diagrams)           | ✅ Done |
| 4.5 — `docs/USAGE.md` (full CLI + troubleshooting)        | ✅ Done |
| 4.6 — `docs/API.md` (module API + custom provider guide)  | ✅ Done |
| 4.7 — Update `docs/MVP_IMPLEMENTATION_PLAN.md`            | ✅ Done |

---

## Phase 5 Status — ✅ Complete

### Library Extraction Readiness

| Task                                                        | Status                         |
| ----------------------------------------------------------- | ------------------------------ |
| 5.1 — All public APIs documented (`#![deny(missing_docs)]`) | ✅ Done                        |
| 5.2 — Test coverage >= 85% (~170 tests)                     | ✅ Done                        |
| 5.3 — Benchmark suite for verdict parsing                   | ✅ Done — `benches/verdict.rs` |
| 5.4 — Workspace deferred (single crate remains)             | ✅ Done                        |

---

## Phase 6 Status — ✅ Complete

### crates.io Publishing & crates.ai Registration

| Task                                                               | Status                                                                   |
| ------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| 6.1 — Prerequisites verification (tests, clippy, fmt, deny, audit) | ✅ Done                                                                  |
| 6.2 — `Cargo.toml` metadata finalized                              | ✅ Done — version 0.6.0, all fields complete                             |
| 6.3 — `README.md` with `cargo install` instructions                | ✅ Done                                                                  |
| 6.4 — `CHANGELOG.md` with Phase 6 entry                            | ✅ Done                                                                  |
| 6.5 — Publish to crates.io                                         | ✅ Done — [crates.io/crates/rs-guard](https://crates.io/crates/rs-guard) |
| 6.6 — Register on crates.ai                                        | ✅ Done — [crates.ai/crates/rs-guard](https://crates.ai/crates/rs-guard) |
| 6.7 — Post-publish verification                                    | ✅ Done                                                                  |

---

## Phase 7 Status — ✅ Complete

### v1.2 Client Extraction (issue #72)

| Task | Status |
| 7.1 — `GenericOpenAiCompatibleClient` (pub(crate)) + hooks | ✅ Done — data-driven; `result_format` + `default_extra_headers` on `ProviderMeta` |
| 7.2 — Delete 5 duplicated clients (deepseek/kimi/qwen/openrouter/openai) | ✅ Done — no shims or re-exports |
| 7.3 — Factory simplified to metadata-driven path | ✅ Done — ~80-line match → ~20 lines |
| 7.4 — Grok (xAI) first-class (`XAI_API_KEY`, `grok-3`) | ✅ Done — closes #74 |
| 7.5 — GLM (Zhipu AI) first-class (`ZHIPUAI_API_KEY`, `glm-4`) | ✅ Done — closes #73 |
| 7.6 — `known_provider_names().len() == 7` | ✅ Done |
| 7.7 — Grok/GLM default pricing | ✅ Done — Grok verified from docs.x.ai (125/250); GLM `None` (unverifiable, F9) |
| 7.8 — Provider-agnostic documentation pass | ✅ Done — README/USAGE/CONFIG/INSTALL/API/ARCHITECTURE/hooks de-biased |
| 7.9 — docs/PROVIDERS.md Grok + GLM sections | ✅ Done |
| 7.10 — docs/GITHUB_BOT_SETUP.md (bot/machine-user guide) | ✅ Done |
| 7.11 — docs/PERFORMANCE.md (binary size + perf baselines) | ✅ Done |
| 7.12 — Hardcoded "openai" provider name removed | ✅ Done — `name()` returns `meta.name` |
| 7.13 — Full linter gates (fmt, clippy -D warnings, test, deny, audit) | ✅ Done |

---

## Notes for Agents

- **Source code exists** — all ~3,800 lines across 13 modules.
- **~450 tests** pass with `wiremock`, `serial_test`, and `tempfile` infrastructure.
- **The implementation plan** (`docs/MVP_IMPLEMENTATION_PLAN.md`) is authoritative but section "Phase 0: Pre-requisite Cleanup" was added during Phase 3 implementation.
- **`Config::empty()`** is a `#[doc(hidden)]` constructor for tests — not for production use.
- **New modules** added since the original plan: `pipeline.rs`, `http.rs`, `redact.rs`, `cache.rs`, `llm/providers.rs`, `llm/generic_client.rs` (v1.2).
- **Decision Log** in Appendix F of the plan tracks all architectural decisions.
- **Cache directory** (`.rs-guard/cache/`) is auto-gitignored on first use — do not commit it.
- **`--no-cache` flag** bypasses the LLM response cache for a fresh API call.
- **v1.2 client extraction** — the 5 per-provider clients were removed; all providers now use `GenericOpenAiCompatibleClient`. Adding a provider = a `ProviderMeta` entry in `llm/providers.rs` + docs + tests.

---

## Step 8: crates.io Publishing & crates.ai Registration (Pending User Approval)

The following steps require your explicit approval and API credentials. Do not proceed until you are ready.

### 8.1 — Prerequisites Verification (Run Locally)

Before publishing, verify all checks pass:

```bash
# All tests must pass
cargo test

# Zero clippy warnings
cargo clippy --all-targets --all-features -- -D warnings

# Formatting clean
cargo fmt --all -- --check

# License + security clean
cargo deny check --config deny.toml

# No known vulnerabilities
cargo install cargo-audit --locked  # one-time setup
cargo audit
```

### 8.2 — Dry Run Publishing

Verify the crate is ready for publishing:

```bash
cargo publish --dry-run
```

If any issues are found, fix them before proceeding.

### 8.3 — Publish to crates.io

**Requires your crates.io API key.** Obtain it from <https://crates.io/settings/tokens>.

```bash
# Login once (API key stored locally)
cargo login

# Publish the crate
cargo publish
```

After publishing:

- Verify the crate appears at <https://crates.io/crates/rs-guard>
- Verify docs.rs auto-generates documentation at <https://docs.rs/rs-guard>

### 8.4 — Register on crates.ai

**Manual web-based process:**

1. Visit <https://crates.ai> and sign in with GitHub OAuth
2. Submit the repository URL: `https://github.com/nebulaideas/rs-guard`
3. Add project metadata:
   - **Description:** "AI-powered code review CLI for GitHub PRs. Multi-provider LLM support with in-memory verdict parsing."
   - **Tags:** `ai`, `code-review`, `github`, `llm`, `cli`, `devops`, `ci-cd`
   - **Screenshot/GIF:** Terminal output showing colored review summary
4. Link documentation:
   - README (primary)
   - docs.rs (auto-generated)
   - GitHub Pages docs site (when deployed)

### 8.5 — Post-Publish Verification

After publication, verify from a clean environment:

```bash
# Test cargo install
cargo install rs-guard --force
rs-guard --version

# Verify docs.rs
# Visit: https://docs.rs/rs-guard/latest/rs-guard/

# Verify crates.ai listing
# Visit: https://crates.ai/crates/rs-guard (after registration)
```

### 8.6 — Update AGENTS.md After Publication

Once published, update the Phase 6 status table in this file:

| Task                            | Status                                                                   |
| ------------------------------- | ------------------------------------------------------------------------ |
| 6.5 — Publish to crates.io      | ✅ Done — [crates.io/crates/rs-guard](https://crates.io/crates/rs-guard) |
| 6.6 — Register on crates.ai     | ✅ Done — [crates.ai/crates/rs-guard](https://crates.ai/crates/rs-guard) |
| 6.7 — Post-publish verification | ✅ Done                                                                  |

---

<!-- lean-ctx-compression -->
OUTPUT STYLE: dense
- Each statement = one atomic fact line
- Use abbreviations: fn, cfg, impl, deps, req, res, ctx, err, ret
- Diff lines only (+/-/~), never repeat unchanged code
- Symbols: → (causes), + (adds), − (removes), ~ (modifies), ∴ (therefore)
- No narration, no filler, no hedging
- BUDGET: ≤200 tokens per response unless code block required
<!-- /lean-ctx-compression -->
