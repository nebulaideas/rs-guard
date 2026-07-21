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
- [Project Rules Injection](#project-rules-injection)
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
| `--provider`    |       | `deepseek`                 | LLM provider: `deepseek`, `kimi`, `qwen`, `openrouter`, `openai`, `grok`, `glm`.     |
| `--variant`     |       | (none)                     | Provider-specific model variant (e.g. `flash`/`pro` for deepseek). See PROVIDERS.md and CONFIGURATION.md. |
| `--config`      | `-c`  | `.reviewer.toml`           | Path to the configuration TOML file.                                               |
| `--max-tokens`  |       | `4096`                     | Maximum tokens for LLM completions.                                                |
| `--llm-timeout` |       | `120`                      | Total timeout in seconds for LLM API requests. Raise for thinking models.          |
| `--important-threshold` | | `3`                    | Number of `[Important]` issues required to `REQUEST_CHANGES`.                      |
| `--diff-file`   | —     | _(none)_                   | Review a pre-existing diff file instead of fetching from GitHub API.               |
| `--no-project-rules` | — | Off                    | Disable project rules auto-detection.                                            |
| `--rules-file`  | —     | _(none)_                   | Path to an explicit project rules file. Overrides auto-detection.                  |
| `--no-cache`    | —     | Off                        | Bypass the response cache and force a fresh LLM API call.                          |
| `--dry-run`     | —     | Off                        | Run the full pipeline without submitting reviews or blocking commits.              |
| `--help`        | `-h`  |                            | Display usage information and exit.                                                |
| `--version`     | `-V`  |                            | Display version and exit.                                                          |

### Subcommands

rs-guard provides setup-automation subcommands to scaffold configuration and
workflow files without copying examples by hand:

| Subcommand        | Description                                                                      |
| ----------------- | -------------------------------------------------------------------------------- |
| `init`            | Scaffold `.github/workflows/rs-guard-review.yml`, `.github/review-prompt.md`, and `.reviewer.toml` in the current repository. |
| `generate-prompt` | Generate a review prompt from a template with optional focus items and language guardrails. |
| `generate-workflow` | Generate a GitHub Actions workflow pinned to the current release version.       |
| `validate-config` | Load and validate `.reviewer.toml` without calling any external API.             |

Examples:

```bash
# Scaffold rs-guard for a Rust project using Kimi
rs-guard init --type rust --provider kimi

# Generate a backend-focused prompt with custom focus items
rs-guard generate-prompt --template backend-api \
  --focus "No N+1 queries" \
  --focus "Use parameterized queries" \
  --language rust \
  --output .github/review-prompt.md

# Generate a fork-safe workflow for OpenAI
rs-guard generate-workflow --provider openai --model gpt-4o-mini --fork-safe

# Validate configuration before committing
rs-guard validate-config
```

### Project Type Detection

`rs-guard init` tries to detect your project type from files in the working directory:

| Detected files | Project type | Notes |
| -------------- | ------------ | ----- |
| `Cargo.toml` | `rust` | Detects Rust crates and workspaces. |
| `package.json` | `frontend-spa` or `backend-api` | Inspects dependencies for React, Vue, Express, Fastify, NestJS, etc. Defaults to `frontend-spa` when ambiguous. |
| `go.mod` | `cli-tooling` | Go module. |
| `pyproject.toml` / `requirements.txt` | `backend-api` | Python project. |
| none of the above | `general` | Language-agnostic review. |

Override auto-detection with `--type`:

```bash
rs-guard init --type backend-api --provider openai
```

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

# DeepSeek with explicit variant (higher-level than --model for supported providers)
# For deepseek-v4-pro (reasoning model) also raise max-tokens and timeout
rs-guard --provider deepseek --variant pro --max-tokens 16384 --llm-timeout 180

# Review a pre-existing diff file
rs-guard --diff-file pr-diff.diff

# Bypass cache and use custom prompt
rs-guard --no-cache --prompt-file .github/review-prompt.md

# Test configuration without submitting or blocking
rs-guard --dry-run
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
| `XAI_API_KEY`           | Grok          | API key from [xAI](https://x.ai)                                                       |
| `ZHIPUAI_API_KEY`       | GLM           | API key from [Zhipu AI](https://open.bigmodel.cn)                                      |
| `GITHUB_TOKEN`          | CI mode       | Auto-provided by GitHub Actions; alternatively set to a PAT with `pull-requests: write` |
| `PR_NUMBER`             | CI mode       | Pull request number                                                                     |
| `REPO_FULL_NAME`        | CI mode       | Repository in `owner/repo` format                                                       |
| `GITHUB_ACTIONS`        | Auto-detected | Presence indicates CI mode                                                              |
| `RS_GUARD_PROVIDER`     | Optional      | Override default provider via environment variable                                      |
| `RS_GUARD_MODEL`        | Optional      | Override default model for the current provider                                         |
| `RS_GUARD_TEMPERATURE`  | Optional      | Override default temperature via environment variable                                   |
| `RS_GUARD_MAX_TOKENS`    | Optional      | Override max tokens via environment variable                                            |
| `RS_GUARD_LLM_TIMEOUT`   | Optional      | LLM request timeout seconds (default 120; raise for thinking models)                    |
| `RS_GUARD_IMPORTANT_THRESHOLD` | Optional | `[Important]` issues threshold (default 3)                                       |
| `RS_GUARD_DIFF_FILE`     | Optional      | Alias for `--diff-file`                                                                 |
| `RS_GUARD_METRICS_PATH` | Optional      | Custom path for `rs-guard-metrics.json` artifact                                        |
| `RS_GUARD_NO_PROJECT_RULES` | Optional | Set to `true` to disable project rules auto-detection. Alias for `--no-project-rules`. |
| `RS_GUARD_RULES_FILE` | Optional      | Path to an explicit project rules file. Alias for `--rules-file`.                       |
| `GITHUB_API_URL`        | Optional      | Custom GitHub API base URL (e.g. GitHub Enterprise); default: `https://api.github.com`  |

---

## Exit Codes

| Code | Meaning                            | When                                              |
| ---- | ---------------------------------- | ------------------------------------------------- |
| `0`  | Review completed successfully      | Any mode, any verdict                             |
| `1`  | Error occurred                     | API failure, config error, parse error, etc.      |
| `2`  | Local/file mode: `REQUEST_CHANGES` | Review returned `REQUEST_CHANGES`; commit blocked |

---

## Review Axes

The default prompt directs the LLM to review every diff across five structured axes, in priority order:

| Axis | Focus |
| ---- | ----- |
| **Correctness** | Logic bugs, incorrect output, edge-case failures, broken invariants |
| **Security** | Injection, auth bypass, secrets in code, unsafe deserialization |
| **Performance** | Algorithmic regressions, unnecessary allocations, blocking I/O |
| **Maintainability** | Dead code, unclear naming, missing docs on public APIs, tight coupling |
| **Test Coverage** | Missing tests for new branches, untested error paths, fragile assertions |

---

## Severity Levels

Each finding is tagged with one of four severity labels that drive the review state:

| Label | Merge impact | Example |
| ----- | ------------ | ------- |
| `[Critical]` | Always blocks — `REQUEST_CHANGES` | Panic at runtime, data loss, broken invariant |
| `[Security]` | Always blocks — `REQUEST_CHANGES` | SQL injection, leaked secret, missing auth |
| `[Important]` | Blocks when ≥ 3 accumulated; otherwise `COMMENT` | Missing error handling, logic edge case |
| `[Suggestion]` | Never blocks — advisory only | Naming, style, optional refactor |

---

## Review State Logic

The internal review state is determined by the LLM verdict using an **asymmetric safety model**:

```text
if verdict == "NEGATIVE" or security_issues > 0 or critical_issues > 0:
    → REQUEST_CHANGES
else if important_issues >= 3:
    → REQUEST_CHANGES
else if important_issues > 0:
    → COMMENT
else:
    → APPROVE
```

**Key principle:** Pessimistic signals are always trusted. `[Critical]` and `[Security]` block unconditionally. `[Important]` findings accumulate — one or two prompt human review (`COMMENT`) while three or more block the merge (`REQUEST_CHANGES`). `[Suggestion]` items are always advisory and never affect the state.

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
      # Pinned from actions/checkout@v5 (93cb6efe) to avoid Node.js 20 deprecation.
      - uses: actions/checkout@93cb6efe18208431cddfb8368fd83d5badbf9bfd

      - name: Download rs-guard
        run: |
          set -euo pipefail
          BINARY="rs-guard-x86_64-unknown-linux-gnu"
          curl -L --fail -o "${BINARY}" \
            "https://github.com/nebulaideas/rs-guard/releases/latest/download/${BINARY}"
          curl -L --fail -o "${BINARY}.sha256" \
            "https://github.com/nebulaideas/rs-guard/releases/latest/download/${BINARY}.sha256"
          sha256sum -c "${BINARY}.sha256"
          chmod +x "${BINARY}"
          mv "${BINARY}" rs-guard

      - name: AI Code Review
        run: ./rs-guard
        env:
          # Set the env var for your chosen provider:
          DEEPSEEK_API_KEY: ${{ secrets.DEEPSEEK_API_KEY }}
          # KIMI_API_KEY: ${{ secrets.KIMI_API_KEY }}
          # OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
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
      # Pinned from actions/checkout@v5 (93cb6efe) to avoid Node.js 20 deprecation.
      - uses: actions/checkout@93cb6efe18208431cddfb8368fd83d5badbf9bfd

      - name: Download rs-guard
        run: |
          set -euo pipefail
          BINARY="rs-guard-x86_64-unknown-linux-gnu"
          curl -L --fail -o "${BINARY}" \
            "https://github.com/nebulaideas/rs-guard/releases/latest/download/${BINARY}"
          curl -L --fail -o "${BINARY}.sha256" \
            "https://github.com/nebulaideas/rs-guard/releases/latest/download/${BINARY}.sha256"
          sha256sum -c "${BINARY}.sha256"
          chmod +x "${BINARY}"
          mv "${BINARY}" rs-guard

      - name: AI Code Review
        run: ./rs-guard --config .reviewer.toml
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          PR_NUMBER: ${{ github.event.pull_request.number }}
          REPO_FULL_NAME: ${{ github.repository }}

      - name: Upload review artifact
        # Pinned from actions/upload-artifact@v5 (330a01c4) to avoid Node.js 20 deprecation.
        uses: actions/upload-artifact@330a01c490aca151604b8cf639adc76d48f6c5d4
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

# Skip if nothing is staged
if git diff --cached --quiet; then
  exit 0
fi

./rs-guard
EXIT_CODE=$?

if [ "$EXIT_CODE" -eq 2 ]; then
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

rs-guard uses a system prompt sent alongside the diff to the LLM. The embedded default works out-of-the-box, but tailoring it to your project produces better, more relevant reviews. A well-crafted prompt reduces false positives, catches domain-specific bugs a generic reviewer would miss, and respects your team's conventions.

Create `.github/review-prompt.md` in your repository root (or pass `--prompt-file`).

### Prompt Templates

Four ready-to-use, language-agnostic templates are provided in [`examples/prompts/`](../examples/prompts/):

| Template | Best for |
| -------- | -------- |
| [`general-code-review.md`](../examples/prompts/general-code-review.md) | Any language or framework — mirrors the embedded default |
| [`backend-api.md`](../examples/prompts/backend-api.md) | REST/GraphQL APIs, database access, auth middleware |
| [`frontend-spa.md`](../examples/prompts/frontend-spa.md) | SPA/component frameworks, state management, accessibility |
| [`cli-tooling.md`](../examples/prompts/cli-tooling.md) | CLI tools, systems programs, exit codes, structured logging |

Copy the closest template and customise the `## Project-Specific Focus` section for your stack.

### Required Metadata Block

Every custom prompt **must** instruct the LLM to end its response with the following block so
rs-guard can parse the verdict:

```text
[RS_GUARD_VERDICT_METADATA]
Verdict: POSITIVE or NEGATIVE
CriticalIssues: <count>
SecurityIssues: <count>
ImportantIssues: <count>
Suggestions: <count>
```

### Best Practices for LLM Code Review Prompts

1. **Anchor the role with stack expertise.** "You are a senior Rust engineer who maintains a `tokio`-based gRPC service" is far more effective than "you are a code reviewer."
2. **Define severity with falsifiable criteria.** "A Critical issue means the code will panic at runtime or produce incorrect output under valid input" — not "bugs are bad."
3. **List concrete signal patterns.** The LLM needs specific code smells to pattern-match against. `?` operator without `.context()` is actionable; "check error handling" is not.
4. **Tell the model what NOT to flag.** Explicitly exclude style preferences, naming conventions, and formatting — the linter covers those. This keeps the review focused.
5. **Include anti-patterns from your tech debt log.** If your team bans `Arc<Mutex<T>>` in hot paths or `after_save` callbacks across bounded contexts, encode that in the prompt.
6. **Keep it under 1,000 words.** The prompt and diff share the model's context window. Every word in the prompt is a word the diff can't use.

---

## Project Rules Injection

rs-guard can automatically layer your project's conventions into the review prompt. When a rules file is found, its content is appended as a **Project Conventions** section that takes precedence over the base review guidance. This is useful for encoding conventions that are too specific for a generic prompt — for example, "All public functions must have doc comments" or "Never call `unwrap` in application code".

### Auto-Detection Priority Order

If no explicit rules file is configured, rs-guard scans the repository root in the following order and uses the first match:

1. `AGENTS.md`
2. `CLAUDE.md`
3. `.github/copilot-instructions.md`
4. `.gemini/styleguide.md`
5. `.cursor/rules/*.md` (first file alphabetically)
6. `.windsurfrules`

Only one rules file is loaded per review run. If multiple files exist, the highest-priority file wins.

### Opting Out

Project rules detection is on by default. Disable it with any of the following, listed by precedence (CLI > env > TOML):

- CLI: `--no-project-rules`
- Environment: `RS_GUARD_NO_PROJECT_RULES=true`
- TOML: `project_rules_enabled = false`

### Explicit Override

Point rs-guard at a specific rules file with any of the following, listed by precedence (CLI > env > TOML):

- CLI: `--rules-file docs/my-rules.md`
- Environment: `RS_GUARD_RULES_FILE=docs/my-rules.md`
- TOML: `rules_file = "docs/my-rules.md"`

The path may be relative to the current working directory or absolute. An explicit file overrides auto-detection entirely. It is mutually exclusive with `--no-project-rules`.

### Interactive Picker (Local Mode)

When running locally with two or more rules files detected, rs-guard prompts you to select one:

```text
info: Multiple project rules files detected:
  [1] AGENTS.md
  [2] CLAUDE.md
Multiple project rules files detected. Select one:
> AGENTS.md
  CLAUDE.md
```

The picker is skipped in CI mode (`is_ci = true`), when `--rules-file` is set, when `--no-project-rules` is set, or when stdin is not a TTY. In those cases, rs-guard falls back to first-match priority silently.

### Soft Cap and Truncation

Rules files are read with a 32 KB soft cap. If a file exceeds the cap, only the first 32 KB are kept and a truncation warning banner is appended so the LLM knows the rules are incomplete. The full file remains on disk; only the prompt content is truncated.

### Relationship to `review-prompt.md`

Project rules are **primary** conventions — they override the base review guidance. The `.github/review-prompt.md` file (or `--prompt-file`) provides the review structure and default focus areas. Use the rules file for project-wide conventions and the prompt file for review mechanics and per-run focus.

### Example

A repo with an `AGENTS.md` containing:

```markdown
# Project Conventions

- All public functions must have doc comments.
- Do not use `unwrap` in application code; use `?` or `expect` with a message.
```

will cause rs-guard to flag missing doc comments or bare `unwrap` calls in the diff, even if the custom prompt does not mention them.

---

## Installation and Setup

rs-guard can be installed in three ways: download a pre-built binary (recommended for CI),
install via cargo, or build from source.

### Quick Start — GitHub Actions (Copy-Paste)

Create `.github/workflows/ai-review.yml`:

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
      # Pinned from actions/checkout@v5 (93cb6efe) to avoid Node.js 20 deprecation.
      - uses: actions/checkout@93cb6efe18208431cddfb8368fd83d5badbf9bfd

      - name: Download rs-guard
        run: |
          set -euo pipefail
          BINARY="rs-guard-x86_64-unknown-linux-gnu"
          curl -L --fail -o "${BINARY}" \
            "https://github.com/nebulaideas/rs-guard/releases/latest/download/${BINARY}"
          curl -L --fail -o "${BINARY}.sha256" \
            "https://github.com/nebulaideas/rs-guard/releases/latest/download/${BINARY}.sha256"
          sha256sum -c "${BINARY}.sha256"
          chmod +x "${BINARY}"
          mv "${BINARY}" rs-guard

      - name: AI Code Review
        run: ./rs-guard
        env:
          # Set the env var for your chosen provider:
          DEEPSEEK_API_KEY: ${{ secrets.DEEPSEEK_API_KEY }}
          # KIMI_API_KEY: ${{ secrets.KIMI_API_KEY }}
          # OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          PR_NUMBER: ${{ github.event.pull_request.number }}
          REPO_FULL_NAME: ${{ github.repository }}
```

Then add your API key in **Settings → Secrets and variables → Actions → `DEEPSEEK_API_KEY`**.

### Local Setup (Pre-commit Hook)

Install the binary:

```bash
# Option A: Pre-built binary (Linux x86_64)
curl -L -o /usr/local/bin/rs-guard \
  https://github.com/nebulaideas/rs-guard/releases/latest/download/rs-guard-x86_64-unknown-linux-gnu
chmod +x /usr/local/bin/rs-guard

# Option B: cargo install
cargo install rs-guard
```

Create `.git/hooks/pre-commit`:

```bash
#!/bin/sh
# Save this as .git/hooks/pre-commit and make it executable: chmod +x .git/hooks/pre-commit
set -e

export DEEPSEEK_API_KEY="${DEEPSEEK_API_KEY:-}"
if [ -z "$DEEPSEEK_API_KEY" ]; then
  echo "⚠️  DEEPSEEK_API_KEY not set — skipping AI review"
  exit 0
fi

# Skip if nothing is staged
if git diff --cached --quiet; then
  exit 0
fi

rs-guard --prompt-file .github/review-prompt.md
if [ $? -eq 2 ]; then
  echo ""
  echo "🚫 Commit blocked: review returned REQUEST_CHANGES"
  echo "   Skip with: git commit --no-verify"
  exit 1
fi
exit 0
```

Create `.github/review-prompt.md` by copying the template for your stack from above.

To bypass the hook on a single commit:

```bash
git commit -m "skip review" --no-verify
```

### Verifying the Setup

```bash
# Check the binary is installed
rs-guard --version

# Run a test review on a local diff file
rs-guard --diff-file /path/to/test.diff --no-cache
```

---

## Outbound secret redaction

Before sending a diff to the LLM, rs-guard redacts common secret patterns (API keys,
GitHub tokens, private keys, password assignments, etc.). In local mode a notice is
printed when redactions occur. The count is recorded in `rs-guard-metrics.json` as
`secrets_redacted_count`.

## Troubleshooting

### `GITHUB_TOKEN is required in CI mode`

Check that your workflow step includes `GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}` in `env`.

### `API key not found. Set DEEPSEEK_API_KEY for provider 'deepseek'`

Set the env var for your provider. Check [docs/PROVIDERS.md](PROVIDERS.md) for how to obtain keys.

### `Unknown provider: 'xxx'`

Supported providers: `deepseek`, `kimi`, `qwen`, `openrouter`, `openai`, `grok`, `glm`.

### `Provider base URL is not a recognized LLM provider endpoint`

In CI mode, URLs are checked against a SSRF allowlist. Use a known provider URL.

### `Diff too large`

The diff exceeds 100 KB / 1500 lines. In CI: an explanatory `COMMENT` is posted. In local/file mode: exits `0`.

### `Diff chunked: omitted {} middle lines`

The diff was truncated (400 head + 400 tail preserved). Expected for large PRs.

### `Review body exceeds GitHub's character limit`

GitHub has a 65536 character limit for review bodies. If your review exceeds this:

- Use a shorter prompt (e.g., remove detailed instructions)
- The diff will be chunked automatically for large PRs
- Consider using `--max-tokens` to limit LLM output length

### `Cache hit — using cached LLM response`

The same diff+prompt+provider+model+temperature combination was cached within the 24-hour TTL. Pass `--no-cache` for a fresh call.

### Empty assistant content / "reasoning may have consumed the token budget" (DeepSeek / Kimi)

Thinking models return `content: null` (or empty) + `reasoning_content` when the output budget is exhausted by chain-of-thought before the final answer.

rs-guard treats this as a transient (retryable) error and will retry up to 3 times with backoff. The response is **never cached** until a successful verdict parse.

**Fixes:**
- Raise `max_tokens` (env `RS_GUARD_MAX_TOKENS`, `--max-tokens`, or TOML). For deepseek/kimi the default is auto-raised to 16,384 **only** when you have not set an explicit value.
- Consider `--llm-timeout 180` or `RS_GUARD_LLM_TIMEOUT=180` if the model needs more wall time for reasoning.
- Use a cheaper/faster variant when possible (e.g. `flash`).

You will see a warning in logs containing the length of `reasoning_content`.

### LLM request timing out

Increase `--llm-timeout` / `RS_GUARD_LLM_TIMEOUT` (seconds). The default is 120s as of v1.2.3 (up from 60s) specifically to help thinking models.

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
