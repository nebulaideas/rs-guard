# Performance & Binary Size

rs-guard is a single static binary that fetches a PR diff, calls an LLM, parses
a verdict, and submits a review. This document gives baselines for measuring
its two cost surfaces: **binary size** (affects download/cold-start in CI) and
**runtime performance** (local and in GitHub Actions).

---

## Table of Contents

- [Build profile](#build-profile)
- [Binary size](#binary-size)
- [Runtime performance](#runtime-performance)
- [GitHub Actions cold-start](#github-actions-cold-start)
- [Caching](#caching)

---

## Build profile

The release profile in `Cargo.toml` is already optimized for a small, stripped
binary:

```toml
[profile.release]
opt-level = "z"      # optimize for size
lto = true           # link-time optimization across crates
codegen-units = 1    # better optimization, slower compile
panic = "abort"      # smaller binary, no unwinding tables
strip = true         # strip debug symbols
```

To build the optimized binary:

```bash
cargo build --release
# binary at target/release/rs-guard
```

---

## Binary size

Measure with:

```bash
cargo build --release
ls -lh target/release/rs-guard
```

### Baseline (v1.8, macOS arm64, Rust 1.92)

**Machine:** Apple M1 Max, 32GB RAM, macOS 26.5.1

| Build | Size | Notes |
| Default `cargo build --release` | **4.6 MB** | profile.release applied (measured 2026-08-18) |
| `strip` (already on via `strip = true`) | included above | symbols removed |
| + `upx --best` (optional) | ~1.5-2 MB (typical UPX ratio) | runtime self-decompression; see note |

> **v1.8 size increase (3.9 → 4.6 MB):** The llm-kernel adoption (issue #142)
> pulled in `aws-lc-rs` as the rustls crypto provider (via reqwest 0.13's
> default `rustls` feature). `aws-lc-rs` is a C/C++ library that adds ~0.7 MB
> to the binary. An upstream issue has been filed
> ([epicsagas/llm-kernel#93](https://github.com/epicsagas/llm-kernel/issues/93))
> to add a `rustls-ring` feature that would eliminate this overhead. Until
> then, `aws-lc-rs` is accepted as a reasonable tradeoff for the functionality
> llm-kernel provides.

> **`upx` note:** UPX compresses the binary at rest and decompresses on
> launch, adding ~50-150ms of startup. It is useful when download bandwidth
> dominates (slow CI runners, containers pulled on every run) but is a net
> loss for fast local invocations. Do not enable it blindly — measure your
> actual CI.

### Tracking regressions

Binary size is sensitive to dependency additions. Before adding a dependency,
check its impact:

```bash
cargo bloat --release --crates
```

This inspects crate-level size against `[profile.release]` in `Cargo.toml`
(`opt-level`, `lto`, `strip`, `panic = "abort"`). Binary-size CI lives in
`.github/workflows/ci.yml` (`binary-size` job).

A size budget is enforced in CI: the `binary-size` job in
`.github/workflows/ci.yml` fails if the release binary exceeds **12 MB**.
The current baseline is 4.6 MB (v1.8, with aws-lc-rs). Adjust the
`BUDGET_MB` variable in the workflow if the budget needs to change.

---

## Runtime performance

rs-guard's wall-clock time is dominated by the **LLM API round-trip** (1-30s
typical). The local CPU work (diff fetch, verdict parse, cache I/O) is in the
tens of milliseconds. Measure the non-LLM portion with `--dry-run` (skips
GitHub submission but still calls the LLM) or by pointing `base_url` at a
local mock.

### Local benchmark

```bash
# Warm-up + timed run against a cached PR (cache hit skips the LLM call)
hyperfine --warmup 1 \
  'rs-guard --pr 42 --dry-run' \
  'rs-guard --pr 42 --dry-run --no-cache'
```

- **Cache hit:** ~50-150ms (diff fetch + cache lookup + verdict parse).
- **Cache miss / fresh LLM call:** dominated by the provider's latency.

### Verdict parsing microbench

The structured-verdict parser and findings stack are benchmarked with Criterion across 11 scenarios:

```bash
cargo bench --bench verdict -- --quick
```

This isolates the CPU-bound parsing path (~ns to µs scale) from the network path.

#### Parsing baselines (Criterion, Apple M-series / x86_64, August 2026)

| Benchmark Target | Scope | Typical Latency |
|---|---|---|
| `determine_review_state` | Review state decision from structured counts | ~1.4 ns |
| `merge_with_findings_20` | Max-rule merge of preliminary verdict + 20 findings | ~98 ns |
| `from_findings_50` | Constructing `Verdict` from 50 structured findings | ~164 ns |
| `evaluate_by_tags` | Tag-based fallback scanner (`[Critical]`, `[Security]`) | ~197 ns |
| `strip_findings_json` | Proactive findings JSON stripping before truncation | ~206 ns |
| `parse_no_metadata_fallback` | Response with no metadata block (fallback to tags) | ~258 ns |
| `parse_metadata_block` | Standard `[RS_GUARD_VERDICT_METADATA]` block parsing | ~812 ns |
| `parse_verdict` | Full pipeline entry point on standard response | ~983 ns |
| `parse_large_response` | True ~10 KB response with metadata block (no findings) | ~1.06 µs |
| `parse_findings_50` | Parsing `[RS_GUARD_VERDICT_FINDINGS]` JSON array (50 items) | ~6.7 µs |
| `parse_large_response_with_findings` | True ~10 KB prose + 50 findings through `parse_verdict()` | ~9.0 µs |

All parsing operations complete in **under 10 microseconds** even for large diff responses with 50 structured findings, making CPU overhead negligible compared to network and LLM latency.

---

## GitHub Actions cold-start

In CI the perceived latency is: **install + binary launch + diff fetch + LLM
call**. The binary (`src/main.rs`) itself launches in ~10-30ms; the install
step dominates. After launch, `run_pipeline()` in `src/pipeline.rs` owns
diff fetch + LLM + review submission.

### Baseline (GitHub-hosted `ubuntu-latest` runner)

| Phase | Typical | Notes |
| `cargo install rs-guard --locked` | 90-150s | compiles from source; cache it |
| Binary download (prebuilt) | 5-15s | if/when prebuilt binaries are published |
| `rs-guard` launch → first network byte | <100ms | binary startup is negligible |
| LLM round-trip | 1-30s | provider + model dependent |
| GitHub review submission | <1s | |

### Reducing install cost

Cache the compiled binary across runs:

```yaml
- uses: actions/cache@v4
  with:
    path: ~/.cargo/bin/rs-guard
    key: rs-guard-${{ runner.os }}-v1.2.1
- run: |
    if ! command -v rs-guard >/dev/null 2>&1; then
      cargo install rs-guard --locked
    fi
```

A warm cache drops the install phase to near-zero. (When prebuilt release
binaries are published, prefer a direct download over `cargo install`.)

---

## Caching

rs-guard caches LLM responses keyed on a SHA-256 over all parameters that affect
the outgoing request (see list below) in `.rs-guard/cache/`. Re-running on an
unchanged input with `--no-cache` unset is a cache hit and skips the LLM call
entirely — the single biggest performance lever for repeated runs.

### Cache key components

The cache key includes **all parameters that affect the outgoing request**:
- `diff_content` (SHA-256 hash)
- `prompt` (SHA-256 hash)
- `project_rules` content (SHA-256 hash when present; presence-tagged when absent)
- `provider` name
- `model` identifier
- `variant` (if set)
- `temperature`
- `base_url` (effective, including overrides)
- `max_tokens` (if set)
- `result_format` (if set; presence-tagged when absent)

**Important:** Changing any of these parameters will cause a cache miss. For
example:
- Overriding `base_url` to point at a local mock will create a separate cache
  entry from the real provider endpoint (prevents cache poisoning)
- Changing `max_tokens` will create a separate cache entry (prevents serving
  truncated responses to full-length requests)
- Changing project rules content invalidates the cache for that configuration
- Changing `result_format` creates a separate cache entry (response shape may differ)

This ensures cache correctness but means that configuration changes will
invalidate cached responses.

- Bypass with `--no-cache` for fresh reviews.
- The cache is size-bounded (100 MB default, LRU-evicted). It is not auto-added
  to `.gitignore` — prefer a [global gitignore](USAGE.md#global-gitignore), or
  opt in per-repo with `auto_gitignore = true`.
- See [docs/CONFIGURATION.md](CONFIGURATION.md) for cache tuning options.
