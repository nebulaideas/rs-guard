# diffguard-rs — Usage Guide

Complete reference for running diffguard-rs in all modes.

---

## Table of Contents

- [CLI Reference](#cli-reference)
- [Environment Variables](#environment-variables)
- [Exit Codes](#exit-codes)
- [Review State Logic](#review-state-logic)
- [GitHub Actions Integration](#github-actions-integration)
- [Local Pre-commit Setup](#local-pre-commit-setup)
- [Configuration File](#configuration-file)
- [Troubleshooting](#troubleshooting)

---

## CLI Reference

```bash
diffguard [OPTIONS]
```

### Options

| Flag | Short | Default | Description |
|---|---|---|---|
| `--prompt-file` | `-p` | `.github/review-prompt.md` | Path to the system prompt markdown file. Uses embedded default if not found. |
| `--model` | `-m` | *(provider default)* | LLM model identifier. Overrides TOML and provider defaults. |
| `--temperature` | `-t` | `0.1` | Sampling temperature (0.0 to 2.0). Lower values produce more deterministic output. |
| `--provider` | | `deepseek` | LLM provider: `deepseek`, `kimi`, `qwen`, `openrouter`, `openai`. |
| `--config` | `-c` | `.reviewer.toml` | Path to the configuration TOML file. |
| `--max-tokens` | | *(none)* | Maximum tokens for LLM completions. |
| `--diff-file` | — | *(none)* | Review a pre-existing diff file instead of fetching from GitHub API. |
| `--no-cache` | — | Off | Bypass the response cache and force a fresh LLM API call. |
| `--help` | `-h` | | Display usage information and exit. |
| `--version` | `-V` | | Display version and exit. |

### Mode Detection

diffguard detects the execution mode:

- **CI mode:** `GITHUB_ACTIONS` env var is set. Fetches PR diff and submits GitHub review.
- **Local mode:** `GITHUB_ACTIONS` absent. Runs `git diff --cached`, prints colored summary, exits code `2` if `REQUEST_CHANGES`.
- **File mode:** `--diff-file` or `DIFFGUARD_DIFF_FILE` set. Reads diff from file, prints colored summary.

### Examples

```bash
# CI mode reviews the PR from env vars
diffguard --provider deepseek --model deepseek-v4-flash

# Local mode with Kimi
diffguard --provider kimi --model kimi-k2.5

# Review a pre-existing diff file
diffguard --diff-file pr-diff.diff

# Bypass cache and use custom prompt
diffguard --no-cache --prompt-file .github/review-prompt.md
```

---

## Environment Variables

| Variable | Required By | Description |
|---|---|---|
| `DEEPSEEK_API_KEY` | DeepSeek | API key from [DeepSeek Platform](https://platform.deepseek.com) |
| `KIMI_API_KEY` | Kimi | API key from [Moonshot AI](https://platform.moonshot.cn) |
| `DASHSCOPE_API_KEY` | Qwen | API key from [Alibaba Cloud DashScope](https://dashscope.aliyun.com) |
| `OPENROUTER_API_KEY` | OpenRouter | API key from [OpenRouter](https://openrouter.ai) |
| `OPENAI_API_KEY` | OpenAI | API key from [OpenAI Platform](https://platform.openai.com) |
| `GITHUB_TOKEN` | CI mode | Auto-provided by GitHub Actions; alternatively set to a PAT with `pull-requests: write` |
| `PR_NUMBER` | CI mode | Pull request number |
| `REPO_FULL_NAME` | CI mode | Repository in `owner/repo` format |
| `GITHUB_ACTIONS` | Auto-detected | Presence indicates CI mode |
| `DIFFGUARD_PROVIDER` | Optional | Override default provider via environment variable |
| `DIFFGUARD_MODEL` | Optional | Override default model for the current provider |
| `DIFFGUARD_TEMPERATURE` | Optional | Override default temperature via environment variable |
| `DIFFGUARD_MAX_TOKENS` | Optional | Override max tokens via environment variable |
| `DIFFGUARD_DIFF_FILE` | Optional | Alias for `--diff-file` |
| `DIFFGUARD_METRICS_PATH` | Optional | Custom path for `diffguard-metrics.json` artifact |
| `GITHUB_API_URL` | Optional | Custom GitHub API base URL (e.g. GitHub Enterprise); default: `https://api.github.com` |

---

## Exit Codes

| Code | Meaning | When |
|---|---|---|
| `0` | Review completed successfully | Any mode, any verdict |
| `1` | Error occurred | API failure, config error, parse error, etc. |
| `2` | Local/file mode: `REQUEST_CHANGES` | Review returned `REQUEST_CHANGES`; commit blocked |

---

## Review State Logic

The internal review state is determined by the LLM verdict using an **asymmetric safety model**:

```
if verdict == "NEGATIVE" or security_issues > 0 or critical_bugs > 2:
    → REQUEST_CHANGES
else if critical_bugs == 0 and security_issues == 0:
    → APPROVE
else:
    → COMMENT
```

**Key principle:** Pessimistic signals are always trusted; optimistic signals require clean counts. A positive verdict with 1–2 critical bugs yields `COMMENT` (not auto-approve), giving a human a chance to decide.

### Permission Fallback

If `APPROVE` or `REQUEST_CHANGES` fails with HTTP 403 (insufficient token permissions), the state is automatically downgraded to `COMMENT` with a `[Bot fallback from {state}]` prefix. This ensures the review is recorded even with read-only tokens.

---

## GitHub Actions Integration

### Minimal Workflow

```yaml
name: AI Code Review
on:
  pull_request:
    types: [opened, synchronize]

permissions:
  pull-requests: write
  contents: read

jobs:
  review:
    runs-on: ubuntu-latest
    if: ${{ !github.event.pull_request.head.repo.fork }}
    steps:
      - uses: actions/checkout@v4

      - name: Download diffguard
        run: |
          curl -L -o diffguard \
            https://github.com/nebulaideas/diffguard-rs/releases/latest/download/diffguard
          chmod +x diffguard

      - name: AI Code Review
        run: ./diffguard
        env:
          DEEPSEEK_API_KEY: ${{ secrets.DEEPSEEK_API_KEY }}
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          PR_NUMBER: ${{ github.event.pull_request.number }}
          REPO_FULL_NAME: ${{ github.repository }}
```

### With `.reviewer.toml`

```yaml
name: AI Code Review
on:
  pull_request:
    types: [opened, synchronize]

permissions:
  pull-requests: write
  contents: read

jobs:
  review:
    runs-on: ubuntu-latest
    if: ${{ !github.event.pull_request.head.repo.fork }}
    steps:
      - uses: actions/checkout@v4

      - name: Download diffguard
        run: |
          curl -L -o diffguard \
            https://github.com/nebulaideas/diffguard-rs/releases/latest/download/diffguard
          chmod +x diffguard

      - name: AI Code Review
        run: ./diffguard --config .reviewer.toml
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          PR_NUMBER: ${{ github.event.pull_request.number }}
          REPO_FULL_NAME: ${{ github.repository }}

      - name: Upload review artifact
        uses: actions/upload-artifact@v4
        if: always()
        with:
          name: review-result
          path: |
            review-result.txt
            diffguard-metrics.json
```

### Workflow Notes

- **Fork safety:** `if: !github.event.pull_request.head.repo.fork` prevents running on forks where secrets are not available.
- **Token scope:** `GITHUB_TOKEN` has `pull-requests: write` scope by default. Request explicitly if needed.
- **Artifacts:** `review-result.txt` and `diffguard-metrics.json` are written by diffguard and can be uploaded as workflow artifacts.

---

## Local Pre-commit Setup

### Option 1: Manual Hook Installation

```bash
cp examples/local-review/pre-commit-hook.sh .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
git add -A
git commit
```

### Option 2: Inline Hook Script

Create `.git/hooks/pre-commit`:

```bash
#!/bin/sh
set -e
git diff --cached --quiet || ./diffguard

if [ $? -eq 2 ]; then
  echo "Commit blocked: diffguard requested changes."
  echo "Skip this check with:"
  echo "  git commit --no-verify"
  exit 1
fi
exit 0
```

### Bypass on Demand

```bash
git commit -m "docs: fix typo" --no-verify
```

---

## Configuration File

Create `.reviewer.toml` in your repository root.

### Full Schema

```toml
# Global defaults
provider = "deepseek"
model = "deepseek-v4-flash"
temperature = 0.1
max_tokens = 8192

# Per-provider configuration
[providers.deepseek]
api_key_env = "DEEPSEEK_API_KEY"
base_url = "https://api.deepseek.com"

[providers.openrouter]
api_key_env = "OPENROUTER_API_KEY"
base_url = "https://openrouter.ai/api/v1"
http_referer = "https://github.com/your-org/your-repo"
```

### Configuration Resolution Order

| Priority | Source | Example |
|---|---|---|
| 1 | CLI flags | `--provider kimi` |
| 2 | Environment variables | `DIFFGUARD_PROVIDER=kimi` |
| 3 | TOML file | `provider = "kimi"` in `.reviewer.toml` |
| 4 | Hardcoded defaults | `provider = "deepseek"` |

### Per-Provider TOML Fields

| Field | Required | Description |
|---|---|---|
| `providers.<name>.api_key_env` | No | Override env var name for API key. Defaults to standard mapping (e.g., `DEEPSEEK_API_KEY`). |
| `providers.<name>.base_url` | No | Override default base URL. In CI mode must be on allowlist. In local mode, warnings logged for non-standard/loopback. |
| `providers.<name>.http_referer` | No | HTTP referer header (e.g. OpenRouter attribution). |

### Provider Switching Behavior

When the provider changes via CLI or env var:
1. Resolves the API key from the appropriate env var (or TOML `api_key_env`).
2. Resets the model to the new provider default unless `--model` was passed.
3. Validates the provider URL against the allowlist (CI) or log warnings (local).

---

## Troubleshooting

### `GITHUB_TOKEN is required in CI mode`
Check that your workflow step includes `GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}` in `env`.

### `API key not found. Set DEEPSEEK_API_KEY for provider 'deepseek'`
Set the env var for your provider. Check [docs/PROVIDERS.md](PROVIDERS.md) for how to obtain keys.

### `Unknown provider: 'xxx'`
Supported providers: `deepseek`, `kimi`, `qwen`, `openrouter`, `openai`.

### `Provider base URL is not a recognized LLM provider endpoint`
In CI mode, URLs are checked against a SSRF allowlist. Use a known provider URL.

### `Diff too large`
The diff exceeds 100 KB / 1500 lines. In CI: an explanatory `COMMENT` is posted. In local/file mode: exits `0`.

### `Diff chunked: omitted {} middle lines`
The diff was truncated (50 head + 50 tail preserved). Expected for large PRs.

### `Cache hit — using cached LLM response`
The same diff+prompt+provider+model+temperature combination was cached within the 24-hour TTL. Pass `--no-cache` for a fresh call.

### Review posted as `COMMENT` instead of `APPROVE`/`REQUEST_CHANGES`
The token may lack `pull-requests: write` scope. See [Permission Fallback](#permission-fallback).

### Local mode produces no output
There may be no staged changes. Run `git add .` first.

### `Failed to read config file '.reviewer.toml'`
Check file permissions and TOML syntax. See `diffguard --help` for the expected config path.

---

## See Also

- [docs/ARCHITECTURE.md](ARCHITECTURE.md) — System design
- [docs/API.md](API.md) — Library module API reference
- [docs/PROVIDERS.md](PROVIDERS.md) — Per-provider setup
- [docs/CONFIGURATION.md](CONFIGURATION.md) — `.reviewer.toml` reference
- [docs/LOCAL_MODE.md](LOCAL_MODE.md) — Pre-commit hook setup
- [examples/github-actions-workflow/ai-review.yml](../examples/github-actions-workflow/ai-review.yml) — Complete CI workflow
