# diffguard-rs — Agent Guide

> Current state of the `diffguard-rs` repository for AI coding agents.

---

## Project Overview

**diffguard-rs** is a Rust-based AI code review CLI tool. It fetches Pull Request diffs from GitHub, sends them to an LLM provider for review, parses a structured verdict from the response, and submits the review state (`APPROVE`, `REQUEST_CHANGES`, or `COMMENT`) back to GitHub — all in a single execution.

**Current Status:** Phases 1 and 2 are complete. Phase 3 (Pre-requisite Cleanup + Advanced Features) is in progress.

- **Repository:** `git@github.com:nebulaideas/diffguard-rs.git`
- **Current Branch:** `phase-3-advanced-features`
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
├── src/                           # Single crate source (15 modules)
│   ├── main.rs                    # CLI entry point (thin)
│   ├── lib.rs                     # Library root
│   ├── pipeline.rs                # Orchestration + PipelineResult
│   ├── cli.rs                     # Clap argument parsing
│   ├── config.rs                  # Env vars + .reviewer.toml parsing
│   ├── diff.rs                    # PR diff fetching + local diff
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
│   ├── output.rs                  # Terminal output + artifact writing
│   ├── redact.rs                  # Secret redaction
│   ├── retry.rs                   # Retry logic for API failures
│   └── verdict.rs                 # Verdict parsing + review state
├── tests/
│   ├── test_data/                 # Sample diffs + LLM responses
│   ├── config_tests.rs            # 21 config tests
│   ├── diff_tests.rs              # 5 diff tests (wiremock)
│   ├── github_tests.rs            # (planned)
│   ├── integration_tests.rs       # 5 full pipeline tests (wiremock)
│   ├── provider_tests.rs          # 14 provider tests (wiremock)
│   └── verdict_tests.rs           # 15 verdict tests
├── examples/
│   ├── github-actions-workflow/   # Sample CI workflows
│   └── local-review/              # Pre-commit hook examples
├── docs/
│   ├── MVP_IMPLEMENTATION_PLAN.md # 950-line implementation roadmap
│   ├── PROVIDERS.md               # Per-provider setup guide
│   ├── CONFIGURATION.md           # .reviewer.toml reference
│   └── LOCAL_MODE.md              # Pre-commit hook setup
├── .github/workflows/
│   ├── ci.yml                     # CI pipeline (format, lint, test, deny, audit)
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
| `provider*` | 19 (5 inline + 14 integration) | Unit + Integration |
| `diff.rs` | 7 (2 inline + 5 integration) | Unit + Integration |
| `retry.rs` | 5 | Inline |
| `redact.rs` | 8 | Inline |
| `pipeline.rs` | 5 | Integration |
| `http.rs` | 16 | Inline |
| `cli.rs` | 3 | Inline |
| **Total** | **~170** | |

**Coverage gap:** `github.rs`, `output.rs`, `pipeline.rs` — now covered. Remaining low-coverage areas: `config.rs` (integration tests exist but some paths need inline tests).

---

## Phase 3 Status

### Pre-requisite Cleanup (Phase 0)

| Task | Status |
|---|---|
| P0.1 — Remove `process::exit` from `run_pipeline()` | ✅ Done |
| P0.2 — `github.rs` test suite (13 tests) | ✅ Done |
| P0.3 — `output.rs` `impl Write` refactor + tests | ✅ Done |
| P0.4 — `#![deny(missing_docs)]` | ✅ Done |
| P0.5 — Update AGENTS.md | ✅ Done (this file) |
| P0.6 — DRY diff-fetch error handling | ❌ Deferred (behaviors differ) |
| P0.7 — Shared HTTP client builder | ✅ Done |
| P0.8 — `tests/test_data/` directory | ✅ Done |
| P0.9 — Full pipeline integration test (5 scenarios) | ✅ Done |

### Advanced Features

| Task | Status |
|---|---|
| 3.1 — Response caching (`.diffguard/cache/`) | ⏳ In progress |
| 3.2 — Metrics export | ⏳ Planned |
| 3.3 — Error recovery (exp backoff + circuit breaker) | ⏳ Planned |
| 3.4 — Diff chunking (50/50 head/tail) | ⏳ Planned |
| 3.5 — Enhanced CI pipeline (benchmarks, docs-deploy) | ⏳ Planned |

---

## Notes for Agents

- **Source code exists** — all ~3,700 lines across 15 modules.
- **~170 tests** pass with `wiremock`, `serial_test`, and `tempfile` infrastructure.
- **The implementation plan** (`docs/MVP_IMPLEMENTATION_PLAN.md`) is authoritative but section "Phase 0: Pre-requisite Cleanup" was added during Phase 3 implementation.
- **`Config::empty()`** is a `#[doc(hidden)]` constructor for tests — not for production use.
- **New modules** added since the original plan: `pipeline.rs`, `http.rs`, `redact.rs`, `llm/providers.rs`.
- **Decision Log** in Appendix F of the plan tracks all architectural decisions.
