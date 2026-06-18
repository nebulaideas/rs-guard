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

### Baseline (v1.2, macOS arm64, Rust 1.82, measured)

**Machine:** Apple M1 Max, 32GB RAM, macOS 26.5.1

| Build | Size | Notes |
| Default `cargo build --release` | **3.9 MB** | profile.release applied (measured 2026-06-18) |
| `strip` (already on via `strip = true`) | included above | symbols removed |
| + `upx --best` (optional) | ~1.5-2 MB (typical UPX ratio) | runtime self-decompression; see note |

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

A size budget is not enforced in CI today; if size-sensitive, add a
`cargo bloat` or `ls -lh` step to `.github/workflows/ci.yml` and fail above a
threshold (e.g. 12 MB).

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

The structured-verdict parser is benchmarked with Criterion:

```bash
cargo bench --bench verdict -- --quick
```

This isolates the CPU-bound parsing path (~µs scale) from the network path.

---

## GitHub Actions cold-start

In CI the perceived latency is: **install + binary launch + diff fetch + LLM
call**. The binary itself launches in ~10-30ms; the install step dominates.

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
    key: rs-guard-${{ runner.os }}-v1.2.0
- run: |
    if ! command -v rs-guard >/dev/null 2>&1; then
      cargo install rs-guard --locked
    fi
```

A warm cache drops the install phase to near-zero. (When prebuilt release
binaries are published, prefer a direct download over `cargo install`.)

---

## Caching

rs-guard caches LLM responses keyed on `(diff, prompt, provider, model,
temperature)` in `.rs-guard/cache/` (SHA-256). Re-running on an unchanged diff
with `--no-cache` unset is a cache hit and skips the LLM call entirely — the
single biggest performance lever for repeated runs.

### Cache key components

The cache key includes **all parameters that affect the outgoing request**:
- `diff_content` (SHA-256 hash)
- `prompt` (SHA-256 hash)
- `provider` name
- `model` identifier
- `variant` (if set)
- `temperature`
- `base_url` (effective, including overrides)
- `max_tokens` (if set)

**Important:** Changing any of these parameters will cause a cache miss. For
example:
- Overriding `base_url` to point at a local mock will create a separate cache
  entry from the real provider endpoint (prevents cache poisoning)
- Changing `max_tokens` will create a separate cache entry (prevents serving
  truncated responses to full-length requests)

This ensures cache correctness but means that configuration changes will
invalidate cached responses.

- Bypass with `--no-cache` for fresh reviews.
- The cache is size-bounded (100 MB default, LRU-evicted) and auto-gitignored.
- See [docs/CONFIGURATION.md](CONFIGURATION.md) for cache tuning options.
