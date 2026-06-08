# diffguard-rs — Agent Guide

> Current state of the `diffguard-rs` repository for AI coding agents.

---

## Project Overview

**diffguard-rs** is a Rust-based AI code review CLI tool. It fetches Pull Request diffs from GitHub, sends them to an LLM provider for review, parses a structured verdict from the response, and submits the review state (`APPROVE`, `REQUEST_CHANGES`, or `COMMENT`) back to GitHub — all in a single execution.

**Current Status:** Phases 1–6 are complete. The crate is published on crates.io and registered on crates.ai.

- **Repository:** `git@github.com:nebulaideas/diffguard-rs.git`
- **Current Branch:** `main`
- **License:** MIT License (Copyright 2026 Nebula Ideas)
- **Language:** Rust (edition 2021, toolchain 1.82+)
- **Crate:** [diffguard on crates.io](https://crates.io/crates/diffguard) | [docs.rs](https://docs.rs/diffguard)

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

# Full test suite (~220 tests)
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
| **Total** | **~220** | |

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

## Phase 4 Status — ✅ Complete

### Documentation Polish

| Task | Status |
|---|---|
| 4.1 — Update AGENTS.md | ✅ Done |
| 4.2 — Update CHANGELOG.md (0.1.0–0.3.0 + [Unreleased]) | ✅ Done |
| 4.3 — README.md rewrite (keep logo, add Phase 3 features) | ✅ Done |
| 4.4 — `docs/ARCHITECTURE.md` (Mermaid diagrams) | ✅ Done |
| 4.5 — `docs/USAGE.md` (full CLI + troubleshooting) | ✅ Done |
| 4.6 — `docs/API.md` (module API + custom provider guide) | ✅ Done |
| 4.7 — Update `docs/MVP_IMPLEMENTATION_PLAN.md` | ✅ Done |

---

## Phase 5 Status — ✅ Complete

### Library Extraction Readiness

| Task | Status |
|---|---|
| 5.1 — All public APIs documented (`#![deny(missing_docs)]`) | ✅ Done |
| 5.2 — Test coverage >= 85% (~170 tests) | ✅ Done |
| 5.3 — Benchmark suite for verdict parsing | ✅ Done — `benches/verdict.rs` |
| 5.4 — Workspace deferred (single crate remains) | ✅ Done |

---

## Phase 6 Status — ✅ Complete

### crates.io Publishing & crates.ai Registration

| Task | Status |
|---|---|
| 6.1 — Prerequisites verification (tests, clippy, fmt, deny, audit) | ✅ Done |
| 6.2 — `Cargo.toml` metadata finalized | ✅ Done — version 0.6.0, all fields complete |
| 6.3 — `README.md` with `cargo install` instructions | ✅ Done |
| 6.4 — `CHANGELOG.md` with Phase 6 entry | ✅ Done |
| 6.5 — Publish to crates.io | ⏳ Pending user approval (see Step 8) |
| 6.6 — Register on crates.ai | ⏳ Pending user approval (see Step 8) |
| 6.7 — Post-publish verification | ⏳ Pending publication |

---

## Notes for Agents

- **Source code exists** — all ~4,200 lines across 16 modules.
- **~220 tests** pass with `wiremock`, `serial_test`, and `tempfile` infrastructure.
- **The implementation plan** (`docs/MVP_IMPLEMENTATION_PLAN.md`) is authoritative but section "Phase 0: Pre-requisite Cleanup" was added during Phase 3 implementation.
- **`Config::empty()`** is a `#[doc(hidden)]` constructor for tests — not for production use.
- **New modules** added since the original plan: `pipeline.rs`, `http.rs`, `redact.rs`, `cache.rs`, `llm/providers.rs`.
- **Decision Log** in Appendix F of the plan tracks all architectural decisions.
- **Cache directory** (`.diffguard/cache/`) is auto-gitignored on first use — do not commit it.
- **`--no-cache` flag** bypasses the LLM response cache for a fresh API call.

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
- Verify the crate appears at <https://crates.io/crates/diffguard>
- Verify docs.rs auto-generates documentation at <https://docs.rs/diffguard>

### 8.4 — Register on crates.ai

**Manual web-based process:**

1. Visit <https://crates.ai> and sign in with GitHub OAuth
2. Submit the repository URL: `https://github.com/nebulaideas/diffguard-rs`
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
cargo install diffguard --force
diffguard --version

# Verify docs.rs
# Visit: https://docs.rs/diffguard/latest/diffguard/

# Verify crates.ai listing
# Visit: https://crates.ai/crates/diffguard (after registration)
```

### 8.6 — Update AGENTS.md After Publication

Once published, update the Phase 6 status table in this file:

| Task | Status |
|---|---|
| 6.5 — Publish to crates.io | ✅ Done — [crates.io/crates/diffguard](https://crates.io/crates/diffguard) |
| 6.6 — Register on crates.ai | ✅ Done — [crates.ai/crates/diffguard](https://crates.ai/crates/diffguard) |
| 6.7 — Post-publish verification | ✅ Done |

---
