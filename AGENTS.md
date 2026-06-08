# diffguard-rs — Agent Guide

> Current state of the `diffguard-rs` repository for AI coding agents.

---

## Project Overview

**diffguard-rs** is a Rust-based AI code review CLI tool. It fetches Pull Request diffs from GitHub, sends them to an LLM provider for review, parses a structured verdict from the response, and submits the review state (`APPROVE`, `REQUEST_CHANGES`, or `COMMENT`) back to GitHub — all in a single execution.

**Current Status:** Phases 1–3 are complete. Phase 4 (README + Documentation Polish) is in progress.

- **Repository:** `git@github.com:nebulaideas/diffguard-rs.git`
- **Current Branch:** `phase-4-docs-polish`
- **License:** MIT License (Copyright 2026 Nebula Ideas)
- **Language:** Rust (edition 2021, toolchain 1.82+)

---

## Technology Stack

| Layer | Technology |
|---|---|
| Language | Rust (edition 2021, toolchain 1.82+) |
| Build Tool | Cargo (single crate) |
| Async Runtime | Tokio |
| HTTP Client | reqwest (rustls-tls) |
| CLI Framework | clap (derive macros) |
| Serialization | serde, serde_json, toml |
| Error Handling | thiserror, anyhow |
| Terminal Output | colored |
| Testing | Built-in test framework + wiremock (HTTP mocking) |
| URL Validation | url crate |
| Secrets | env vars + redact module |
| Hashing | sha2 + hex (cache keys) |

### Implemented LLM Providers

| Provider | Status | Default Model |
|---|---|---|
| DeepSeek | ✅ Phase 1 | `deepseek-v4-flash` |
| Kimi (Moonshot AI) | ✅ Phase 2 | `kimi-k2.5` |
| Qwen (Alibaba Cloud) | ✅ Phase 2 | `qwen-plus` |
| OpenRouter | ✅ Phase 2 | `openai/gpt-4o-mini` |
| OpenAI | ✅ Phase 2 | `gpt-4o-mini` |

---

## Repository Structure

```
diffguard-rs/
├── src/                           # Single crate source (16 modules)
│   ├── main.rs                    # CLI entry point (thin)
│   ├── lib.rs                     # Library root
│   ├── pipeline.rs                # Orchestration + PipelineResult
│   ├── cache.rs                   # LLM response caching (SHA-256 keyed)
│   ├── cli.rs                     # Clap argument parsing
│   ├── config.rs                  # Env vars + .reviewer.toml parsing
│   ├── diff.rs                    # PR diff fetching + local diff + chunking
│   ├── error.rs                   # DiffguardError enum
│   ├── github.rs                  # GitHub API review submission
│   ├── http.rs                    # HTTP utilities + URL validation
│   ├── llm/                       # LLM provider modules
│   │   ├── mod.rs                 # LlmProvider trait + types
│   │   ├── deepseek.rs            # DeepSeek provider
│   │   ├── kimi.rs                # Kimi provider
│   │   ├── qwen.rs                # Qwen provider
│   │   ├── openrouter.rs          # OpenRouter provider
│   │   ├── openai.rs              # OpenAI provider
│   │   ├── factory.rs             # Provider factory
│   │   └── providers.rs           # Centralized provider metadata
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
│   ├── provider_tests.rs          # 14 provider tests (wiremock)
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
│   └── LOCAL_MODE.md              # Pre-commit hook setup
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

| Decision | Choice |
|---|---|
| Crate structure | Single crate (workspace deferred until library demand emerges) |
| Provider dispatch | `Box<dyn LlmProvider>` trait objects (refactored from enum dispatch in Phase 1) |
| Exit signal | `PipelineResult` enum (Success / ReviewBlocked) — not `process::exit()` in library code |
| SSRF protection | URL allowlist per provider in CI mode; loopback allowed in local mode |
| Print functions | Accept `impl Write` for testability |
| `Config::empty()` | Test-only constructor for integration tests |
| `#![deny(missing_docs)]` | Enforced at crate level |
| Cache keying | SHA-256 over (diff \| prompt \| provider \| model \| temperature) — all parameters matter |
| Cache timestamps | Stored in file content (line 1), not mtime — reliable across clock changes and file copies |
| Cache size limit | 100 MB default with LRU cleanup — prevents unbounded disk usage |
| Circuit breaker | Simple Closed/Open only (no half-open), opt-in, default disabled |
| Cost calculation | Integer cents, not floating point — avoids precision issues |
| Diff chunking | `Cow<str>` return — zero allocation when no truncation needed |

---

## Build and Test Commands

```bash
# Build
cargo build

# Full test suite (~170 tests)
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
cargo audit
```

---

## Test Coverage

| Module | Test Count | Type |
|---|---|---|
| `verdict.rs` | 22 (7 inline + 15 integration) | Unit + Integration |
| `config.rs` | 21 | Integration |
| `github.rs` | 13 | Inline (wiremock) |
| `output.rs` | 6 | Inline |
| `cache.rs` | 13 | Inline |
| `retry.rs` | 20 (5 retry + 15 circuit breaker) | Inline |
| `provider*` | 19 (5 inline + 14 integration) | Unit + Integration |
| `diff.rs` | 12 (4 inline + 8 integration) | Unit + Integration |
| `redact.rs` | 8 | Inline |
| `pipeline.rs` | 5 | Integration |
| `http.rs` | 16 | Inline |
| `cli.rs` | 3 | Inline |
| **Total** | **~170** | |

---

## Phase 3 Status — ✅ Complete

### Pre-requisite Cleanup (Phase 0)

| Task | Status |
|---|---|
| P0.1 — Remove `process::exit` from `run_pipeline()` | ✅ Done |
| P0.2 — `github.rs` test suite (13 tests) | ✅ Done |
| P0.3 — `output.rs` `impl Write` refactor + tests | ✅ Done |
| P0.4 — `#![deny(missing_docs)]` | ✅ Done |
| P0.5 — Update AGENTS.md | ✅ Done (this file) |
| P0.6 — DRY diff-fetch error handling | ❌ Deferred (behaviors differ per source) |
| P0.7 — Shared HTTP client builder | ✅ Done |
| P0.8 — `tests/test_data/` directory | ✅ Done |
| P0.9 — Full pipeline integration test (5 scenarios) | ✅ Done |

### Advanced Features

| Task | Status |
|---|---|
| 3.1 — Response caching (`src/cache.rs`, `.diffguard/cache/`) | ✅ Done — 13 inline tests, SHA-256 keyed, TTL+size limit, atomic writes |
| 3.2 — Metrics export (`diffguard-metrics.json`) | ✅ Done — `ReviewMetrics` struct, `write_metrics()`, per-run JSON artifact |
| 3.3 — Error recovery (exp backoff + circuit breaker) | ✅ Done — `with_retry`, `CircuitBreaker`, 20 inline tests, thread-safe |
| 3.4 — Diff chunking (50/50 head/tail, `Cow<str>`) | ✅ Done — integrated in pipeline, warning shown in both CI and local modes |
| 3.5 — Enhanced CI pipeline (deny, audit, bench, docs-deploy) | ✅ Done — `ci.yml` + `docs-deploy.yml`, `benches/verdict.rs` |

---

## Phase 4 Status — ⏳ In Progress

### Documentation Polish

| Task | Status |
|---|---|
| 4.1 — Update AGENTS.md | ✅ Done (this file) |
| 4.2 — Update CHANGELOG.md (0.1.0–0.3.0 + [Unreleased]) | ⏳ In progress |
| 4.3 — README.md rewrite (keep logo, add Phase 3 features) | ⏳ In progress |
| 4.4 — `docs/ARCHITECTURE.md` (Mermaid diagrams) | ⏳ In progress |
| 4.5 — `docs/USAGE.md` (full CLI + troubleshooting) | ⏳ In progress |
| 4.6 — `docs/API.md` (module API + custom provider guide) | ⏳ In progress |
| 4.7 — Update `docs/MVP_IMPLEMENTATION_PLAN.md` | ⏳ In progress |

---

## Notes for Agents

- **Source code exists** — all ~4,200 lines across 16 modules.
- **~170 tests** pass with `wiremock`, `serial_test`, and `tempfile` infrastructure.
- **The implementation plan** (`docs/MVP_IMPLEMENTATION_PLAN.md`) is authoritative but section "Phase 0: Pre-requisite Cleanup" was added during Phase 3 implementation.
- **`Config::empty()`** is a `#[doc(hidden)]` constructor for tests — not for production use.
- **New modules** added since the original plan: `pipeline.rs`, `http.rs`, `redact.rs`, `cache.rs`, `llm/providers.rs`.
- **Decision Log** in Appendix F of the plan tracks all architectural decisions.
- **Cache directory** (`.diffguard/cache/`) is auto-gitignored on first use — do not commit it.
- **`--no-cache` flag** bypasses the LLM response cache for a fresh API call.
