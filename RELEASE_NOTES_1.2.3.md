# rs-guard v1.2.3

**Released:** 2026-06-21

This release focuses on making `deepseek-v4-pro` (and other thinking/reasoning models) significantly more reliable in both local and CI environments.

## Highlights

- **Configurable LLM timeout** via CLI, environment variable, or `.reviewer.toml`
- **Smart defaults for thinking models**: `deepseek` and `kimi` now auto-raise the timeout to **180 seconds** when you don't set one explicitly
- **Greatly improved documentation** for using `deepseek-v4-pro`, including full TOML, environment, CLI, and GitHub Actions examples
- Better guidance to avoid the flakiness that was previously seen with heavy reasoning models

## Upgrade Notes

- New `--llm-timeout`, `RS_GUARD_LLM_TIMEOUT`, and `llm_timeout_secs` options are now available.
- Users of `deepseek` (especially `deepseek-v4-pro`) will see a higher default timeout (180s) automatically.
- Existing configurations are unaffected. The new behavior only kicks in when you have not specified a timeout.

## Full Changelog

See [CHANGELOG.md](CHANGELOG.md#123---2026-06-21) for the complete list of changes.

## Recommended Configuration for deepseek-v4-pro (CI)

```bash
./rs-guard \
  --prompt-file .github/review-prompt.md \
  --provider deepseek \
  --variant pro \
  --max-tokens 16384 \
  --llm-timeout 240
```

Or via `.reviewer.toml`:

```toml
provider = "deepseek"
variant = "pro"

max_tokens = 16384
llm_timeout_secs = 240
```

## Installation

```bash
cargo install rs-guard --version 1.2.3
```

Or download the pre-built binary from the [GitHub Releases](https://github.com/nebulaideas/rs-guard/releases/tag/v1.2.3) page.

---

**Full changelog and upgrade instructions**: https://github.com/nebulaideas/rs-guard/blob/main/CHANGELOG.md#123---2026-06-21
