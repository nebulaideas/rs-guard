# rs-guard — Usage Guide

Complete reference for running rs-guard in all modes.

---

## Table of Contents

- [CLI Reference](#cli-reference)
- [Environment Variables](#environment-variables)
- [Exit Codes](#exit-codes)
- [Review State Logic](#review-state-logic)
- [GitHub Actions Integration](#github-actions-integration)
- [Local Pre-commit Setup](#local-pre-commit-setup)
- [Configuration File](#configuration-file)
- [Customizing the Review Prompt](#customizing-the-review-prompt)
- [Troubleshooting](#troubleshooting)

---

## CLI Reference

```bash
rs-guard [OPTIONS]
```

### Options

| Flag            | Short | Default                    | Description                                                                        |
| --------------- | ----- | -------------------------- | ---------------------------------------------------------------------------------- |
| `--prompt-file` | `-p`  | `.github/review-prompt.md` | Path to the system prompt markdown file. Uses embedded default if not found.       |
| `--model`       | `-m`  | _(provider default)_       | LLM model identifier. Overrides TOML and provider defaults.                        |
| `--temperature` | `-t`  | `0.1`                      | Sampling temperature (0.0 to 2.0). Lower values produce more deterministic output. |
| `--provider`    |       | `deepseek`                 | LLM provider: `deepseek`, `kimi`, `qwen`, `openrouter`, `openai`.                  |
| `--config`      | `-c`  | `.reviewer.toml`           | Path to the configuration TOML file.                                               |
| `--max-tokens`  |       | _(none)_                   | Maximum tokens for LLM completions.                                                |
| `--diff-file`   | —     | _(none)_                   | Review a pre-existing diff file instead of fetching from GitHub API.               |
| `--no-cache`    | —     | Off                        | Bypass the response cache and force a fresh LLM API call.                          |
| `--help`        | `-h`  |                            | Display usage information and exit.                                                |
| `--version`     | `-V`  |                            | Display version and exit.                                                          |

### Mode Detection

rs-guard detects the execution mode:

- **CI mode:** `GITHUB_ACTIONS` env var is set. Fetches PR diff and submits GitHub review.
- **Local mode:** `GITHUB_ACTIONS` absent. Runs `git diff --cached`, prints colored summary, exits code `2` if `REQUEST_CHANGES`.
- **File mode:** `--diff-file` or `RS_GUARD_DIFF_FILE` set. Reads diff from file, prints colored summary.

### Examples

```bash
# CI mode reviews the PR from env vars
rs-guard --provider deepseek --model deepseek-v4-flash

# Local mode with Kimi
rs-guard --provider kimi --model kimi-k2.5

# Review a pre-existing diff file
rs-guard --diff-file pr-diff.diff

# Bypass cache and use custom prompt
rs-guard --no-cache --prompt-file .github/review-prompt.md
```

---

## Environment Variables

| Variable                | Required By   | Description                                                                             |
| ----------------------- | ------------- | --------------------------------------------------------------------------------------- |
| `DEEPSEEK_API_KEY`      | DeepSeek      | API key from [DeepSeek Platform](https://platform.deepseek.com)                         |
| `KIMI_API_KEY`          | Kimi          | API key from [Moonshot AI](https://platform.moonshot.cn)                                |
| `DASHSCOPE_API_KEY`     | Qwen          | API key from [Alibaba Cloud DashScope](https://dashscope.aliyun.com)                    |
| `OPENROUTER_API_KEY`    | OpenRouter    | API key from [OpenRouter](https://openrouter.ai)                                        |
| `OPENAI_API_KEY`        | OpenAI        | API key from [OpenAI Platform](https://platform.openai.com)                             |
| `GITHUB_TOKEN`          | CI mode       | Auto-provided by GitHub Actions; alternatively set to a PAT with `pull-requests: write` |
| `PR_NUMBER`             | CI mode       | Pull request number                                                                     |
| `REPO_FULL_NAME`        | CI mode       | Repository in `owner/repo` format                                                       |
| `GITHUB_ACTIONS`        | Auto-detected | Presence indicates CI mode                                                              |
| `RS_GUARD_PROVIDER`     | Optional      | Override default provider via environment variable                                      |
| `RS_GUARD_MODEL`        | Optional      | Override default model for the current provider                                         |
| `RS_GUARD_TEMPERATURE`  | Optional      | Override default temperature via environment variable                                   |
| `RS_GUARD_MAX_TOKENS`   | Optional      | Override max tokens via environment variable                                            |
| `RS_GUARD_DIFF_FILE`    | Optional      | Alias for `--diff-file`                                                                 |
| `RS_GUARD_METRICS_PATH` | Optional      | Custom path for `rs-guard-metrics.json` artifact                                        |
| `GITHUB_API_URL`        | Optional      | Custom GitHub API base URL (e.g. GitHub Enterprise); default: `https://api.github.com`  |

---

## Exit Codes

| Code | Meaning                            | When                                              |
| ---- | ---------------------------------- | ------------------------------------------------- |
| `0`  | Review completed successfully      | Any mode, any verdict                             |
| `1`  | Error occurred                     | API failure, config error, parse error, etc.      |
| `2`  | Local/file mode: `REQUEST_CHANGES` | Review returned `REQUEST_CHANGES`; commit blocked |

---

## Review State Logic

The internal review state is determined by the LLM verdict using an **asymmetric safety model**:

```bash
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

      - name: Download rs-guard
        run: |
          curl -L -o rs-guard \
            https://github.com/nebulaideas/rs-guard/releases/latest/download/rs-guard
          chmod +x rs-guard

      - name: AI Code Review
        run: ./rs-guard
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

      - name: Download rs-guard
        run: |
          curl -L -o rs-guard \
            https://github.com/nebulaideas/rs-guard/releases/latest/download/rs-guard
          chmod +x rs-guard

      - name: AI Code Review
        run: ./rs-guard --config .reviewer.toml
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
            rs-guard-metrics.json
```

### Workflow Notes

- **Fork safety:** `if: !github.event.pull_request.head.repo.fork` prevents running on forks where secrets are not available.
- **Token scope:** `GITHUB_TOKEN` has `pull-requests: write` scope by default. Request explicitly if needed.
- **Artifacts:** `review-result.txt` and `rs-guard-metrics.json` are written by rs-guard and can be uploaded as workflow artifacts.

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
git diff --cached --quiet || ./rs-guard

if [ $? -eq 2 ]; then
  echo "Commit blocked: rs-guard requested changes."
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

| Priority | Source                | Example                                 |
| -------- | --------------------- | --------------------------------------- |
| 1        | CLI flags             | `--provider kimi`                       |
| 2        | Environment variables | `RS_GUARD_PROVIDER=kimi`                |
| 3        | TOML file             | `provider = "kimi"` in `.reviewer.toml` |
| 4        | Hardcoded defaults    | `provider = "deepseek"`                 |

### Per-Provider TOML Fields

| Field                           | Required | Description                                                                                                           |
| ------------------------------- | -------- | --------------------------------------------------------------------------------------------------------------------- |
| `providers.<name>.api_key_env`  | No       | Override env var name for API key. Defaults to standard mapping (e.g., `DEEPSEEK_API_KEY`).                           |
| `providers.<name>.base_url`     | No       | Override default base URL. In CI mode must be on allowlist. In local mode, warnings logged for non-standard/loopback. |
| `providers.<name>.http_referer` | No       | HTTP referer header (e.g. OpenRouter attribution).                                                                    |

### Provider Switching Behavior

When the provider changes via CLI or env var:

1. Resolves the API key from the appropriate env var (or TOML `api_key_env`).
2. Resets the model to the new provider default unless `--model` was passed.
3. Validates the provider URL against the allowlist (CI) or log warnings (local).

---

## Customizing the Review Prompt

rs-guard uses a system prompt sent alongside the diff to the LLM. The embedded default works out-of-the-box, but tailoring it to your project produces better, more relevant reviews.

Create `.github/review-prompt.md` in your repository root (or pass `--prompt-file`).

### Best Practices for LLM Code Review Prompts

1. **Anchor the role.** Give the LLM a specific seniority level and domain: "You are a senior Go engineer reviewing a backend PR for a fintech codebase."
2. **Define severity explicitly.** Explain what constitutes Critical vs. style nit. The LLM can't calibrate without definitions.
3. **Give falsifiable instructions.** "Flag any `.unwrap()` in non-test code" is better than "check for error handling."
4. **Prioritize focus areas.** List 5–7 areas in priority order — the LLM will weigh them accordingly.
5. **Keep it concise.** The prompt and diff must fit in the model's context window together. Aim for under 800 words.
6. **Trust signals, not intent.** Don't ask the LLM to guess developer intent; give it objective patterns to match.

### Template: Backend (Rust/Go/Node)

```markdown
# rs-guard Review Prompt — Backend

## Role

You are a senior backend engineer reviewing a Pull Request. You have deep expertise
in production systems, reliability, and secure coding. You treat every PR as if it
will deploy directly to production.

## Focus Areas (in priority order)

1. **Correctness:** logic errors, off-by-one, missing edge cases, broken control flow
2. **Security:** injection vectors, missing auth checks, exposed secrets, unsafe deserialization
3. **Error handling:** swallowed errors, missing propagation, unhandled failure modes
4. **Concurrency:** race conditions, deadlocks, missing synchronization, shared mutable state
5. **Resource management:** connections/goroutines not released, file descriptor leaks, OOM risk
6. **API contracts:** breaking changes, missing validation, inconsistent error responses

## Signal Patterns

Flag these immediately as Critical:

- SQL/SQL-like string concatenation or interpolation
- `unsafe`, `.unwrap()`, `.expect()` in non-test code paths
- Catch-all error handlers that discard error values
- Hardcoded credentials, tokens, or internal URLs
- Unbounded allocations with user-controlled size

## Verdict Guidelines

- **POSITIVE** if none of the signal patterns are found and the diff is production-ready.
- **NEGATIVE** if any Critical signal pattern is present or the code would cause a
  runtime failure.

At the end of your response, include exactly this metadata block:

[RS_GUARD_VERDICT_METADATA]
Verdict: POSITIVE or NEGATIVE
CriticalBugs: <count>
SecurityIssues: <count>
```

### Template: Frontend (React/Vue/Svelte)

```markdown
# rs-guard Review Prompt — Frontend

## Role

You are a senior frontend engineer reviewing a Pull Request. You care about user
experience, accessibility, performance, and maintainable component architecture.

## Focus Areas (in priority order)

1. **Security:** XSS via `dangerouslySetInnerHTML`/`v-html`, client-side secrets, unsafe
   `eval`, open redirects from URL params
2. **Accessibility:** missing `alt`/`aria-*`, keyboard traps, unlabeled form controls,
   color-only information
3. **State management:** stale closures, missing dependencies in `useEffect`/`watch`,
   mutating state directly, improper key usage in lists
4. **API contracts:** mismatched response shapes, missing error/null states, loading
   states not handled
5. **Performance:** unnecessary re-renders, missing memoization, large bundle imports,
   unoptimized images, missing lazy loading
6. **Error boundaries:** unhandled promise rejections, router errors not caught,
   graceful fallback components

## Signal Patterns

Flag these immediately as Critical:

- `dangerouslySetInnerHTML`, `v-html` with user-controlled content
- Secrets or API keys in client-side code
- `eval()`, `new Function()`, `document.write()`
- Unbounded loops or recursion in render path

## Verdict Guidelines

- **POSITIVE** if the component tree is safe, accessible, and will render correctly
  under all expected states.
- **NEGATIVE** if any Critical signal pattern is present or there are client-side
  security vulnerabilities.

At the end of your response, include exactly this metadata block:

[RS_GUARD_VERDICT_METADATA]
Verdict: POSITIVE or NEGATIVE
CriticalBugs: <count>
SecurityIssues: <count>
```

### Template: Monolith (Rails/Django/Laravel)

```markdown
# rs-guard Review Prompt — Monolith/Full-Stack

## Role

You are a senior full-stack engineer reviewing a Pull Request in a monolithic
codebase. You understand that changes in one module can cascade into others
through shared models, callbacks, and background jobs.

## Focus Areas (in priority order)

1. **Database safety:** irreversible migrations, missing indexes on foreign keys,
   default values that break existing rows, non-concurrent index creation
2. **Data integrity:** missing transaction boundaries, partial updates, race conditions
   between web and background jobs, `save!`/`update!` without exception handling
3. **Coupling:** cross-model callbacks that span bounded contexts, fat models (>300 lines),
   controllers with business logic, view templates hitting the database
4. **N+1 queries:** missing `includes`/`prefetch_related`/eager loading, queries in loops
5. **Authorization:** missing policy/ability checks, authorization bypass via nested
   resource access, admin-only actions exposed to authenticated users
6. **Background jobs:** idempotency gaps, missing retry/discard strategies, job arguments
   that can't be serialized, jobs that depend on transient state

## Signal Patterns

Flag these immediately as Critical:

- Migrations that drop/rename columns without safety checks
- `Model.find(params[:id])` without ownership scoping
- Cross-service callbacks (e.g., `after_save` in User that touches Billing)
- Views that call database queries
- Background jobs without idempotency guarantee

## Verdict Guidelines

- **POSITIVE** if changes are safe to deploy and don't introduce coupling or
  degrade data integrity.
- **NEGATIVE** if any Critical signal pattern is present, or the migration cannot
  be safely rolled back.

At the end of your response, include exactly this metadata block:

[RS_GUARD_VERDICT_METADATA]
Verdict: POSITIVE or NEGATIVE
CriticalBugs: <count>
SecurityIssues: <count>
```

### When to Use Which Template

- **Backend** — APIs, services, CLI tools, databases, infrastructure code
- **Frontend** — SPAs, SSR apps, component libraries, design systems
- **Monolith** — Rails, Django, Laravel, or any framework where models, views, controllers,
  and jobs share a single codebase

If your project spans multiple domains, pick the template that covers the majority
of the code in the diff. The prompts are designed to be combined — you can mix focus
areas from different templates.

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

Check file permissions and TOML syntax. See `rs-guard --help` for the expected config path.

---

## See Also

- [docs/ARCHITECTURE.md](ARCHITECTURE.md) — System design
- [docs/API.md](API.md) — Library module API reference
- [docs/PROVIDERS.md](PROVIDERS.md) — Per-provider setup
- [docs/CONFIGURATION.md](CONFIGURATION.md) — `.reviewer.toml` reference
- [docs/LOCAL_MODE.md](LOCAL_MODE.md) — Pre-commit hook setup
- [examples/github-actions-workflow/ai-review.yml](../examples/github-actions-workflow/ai-review.yml) — Complete CI workflow
