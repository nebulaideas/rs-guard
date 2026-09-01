# rs-guard — Usage Guide

Complete reference for running rs-guard in all modes.

---

## Table of Contents

- [CLI Reference](#cli-reference)
- [Environment Variables](#environment-variables)
- [JSON output (`--format json`)](#json-output-format-json)
- [Structured Findings (v1.7)](#structured-findings-v17)
- [Inline Comments (v1.7)](#inline-comments-v17)
- [Check Runs (v1.7)](#check-runs-v17)
- [Multi-pass Review (v1.8)](#multi-pass-review-v18)
- [Ignore File (v1.8)](#ignore-file-v18)
- [Language-aware Prompt Selection (v1.8)](#language-aware-prompt-selection-v18)
- [Exit Codes](#exit-codes)
- [Review State Logic](#review-state-logic)
- [GitHub Actions Integration](#github-actions-integration)
- [GitLab CI and other forges](#gitlab-ci-and-other-forges)
- [Local Pre-commit Setup](#local-pre-commit-setup)
- [Configuration File](#configuration-file)
- [Customizing the Review Prompt](#customizing-the-review-prompt)
- [Project Rules Injection](#project-rules-injection)
- [Troubleshooting](#troubleshooting)

---

## CLI Reference

Flags are defined by `Cli` / `ReviewArgs` in `src/cli.rs`.

```bash
rs-guard [OPTIONS]
```

### Options

| Flag            | Short | Default                    | Description                                                                        |
| --------------- | ----- | -------------------------- | ---------------------------------------------------------------------------------- |
| `--prompt-file` | `-p`  | `.github/review-prompt.md` | Path to the system prompt markdown file. Uses embedded default if not found. A loaded custom prompt file suppresses the interactive project-rules picker. |
| `--model`       | `-m`  | _(provider default)_       | LLM model identifier. Overrides TOML and provider defaults.                        |
| `--temperature` | `-t`  | `0.1`                      | Sampling temperature (0.0 to 2.0). Lower values produce more deterministic output. |
| `--provider`    |       | `deepseek`                 | LLM provider: `deepseek`, `kimi`, `qwen`, `openrouter`, `openai`, `grok`, `glm`, `ollama`, `gemini`. |
| `--variant`     |       | (none)                     | Provider-specific model variant (e.g. `flash`/`pro` for deepseek). See PROVIDERS.md and CONFIGURATION.md. |
| `--config`      | `-c`  | `.reviewer.toml`           | Path to the configuration TOML file.                                               |
| `--max-tokens`  |       | `4096`                     | Maximum tokens for LLM completions.                                                |
| `--llm-timeout` |       | `120` (`240` deepseek/kimi) | Total timeout in seconds for LLM API requests. Auto-raised to 240s for thinking providers when unset. |
| `--important-threshold` | | `3`                    | Number of `[Important]` issues required to `REQUEST_CHANGES`.                      |
| `--diff-file`   | —     | _(none)_                   | Review a pre-existing diff file instead of fetching from GitHub API.               |
| `--no-project-rules` | — | Off                    | Disable project rules auto-detection.                                            |
| `--rules-file`  | —     | _(none)_                   | Path to an explicit project rules file. Overrides auto-detection.                  |
| `--no-cache`    | —     | Off                        | Bypass the response cache and force a fresh LLM API call.                          |
| `--format`      |       | `text`                     | Output format: `text` (human-readable) or `json` (machine-readable).               |
| `--findings`    | —     | Off                        | Enable structured JSON findings from the LLM (opt-in). See [Structured Findings](#structured-findings-v17). |
| `--inline-comments` | — | Off                        | Post inline review comments on the diff (implies `--findings`). See [Inline Comments](#inline-comments-v17). |
| `--check-run`   | —     | Off                        | Publish a GitHub Check Run in addition to the PR review. See [Check Runs](#check-runs-v17). |
| `--check-run-name` | —  | `rs-guard`                 | Custom name for the GitHub Check Run.                                               |
| `--dry-run`     | —     | Off                        | Run the full pipeline without submitting reviews or blocking commits.              |
| `--multi-pass`  | —     | Off                        | Enable multi-pass review for large diffs. See [Multi-pass Review](#multi-pass-review-v18). |
| `--multi-pass-max-chunks` | — | `10`                 | Maximum chunks to split the diff into. |
| `--multi-pass-max-concurrent` | — | `3`              | Maximum concurrent LLM calls during multi-pass. |
| `--multi-pass-max-cost-cents` | — | (disabled)         | Abort if estimated total cost exceeds this cap (in cents). |
| `--ignore-file` | —     | `.rs-guardignore`          | Path to a gitignore-style file for excluding paths from review. See [Ignore File](#ignore-file-v18). |
| `--no-auto-prompt` | —  | Off                        | Disable language-aware prompt auto-selection. See [Language-aware Prompt Selection](#language-aware-prompt-selection-v18). |
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
| `RS_GUARD_FINDINGS` | Optional      | Set to `true` to enable structured findings mode. Alias for `--findings`.              |
| `RS_GUARD_INLINE_COMMENTS` | Optional | Set to `true` to enable inline review comments. Implies `--findings`. Alias for `--inline-comments`. |
| `RS_GUARD_CHECK_RUN` | Optional     | Set to `true` to publish a GitHub Check Run. Alias for `--check-run`.                  |
| `RS_GUARD_CHECK_RUN_NAME` | Optional | Custom name for the GitHub Check Run. Alias for `--check-run-name`.                    |
| `GITHUB_API_URL`        | Optional      | Custom GitHub API base URL (e.g. GitHub Enterprise); default: `https://api.github.com`  |

---

## JSON output (`--format json`)

`OutputFormat::Json` in `src/cli.rs`, rendered by `src/pipeline.rs` when
`config.output_format` is JSON.

Emit a single JSON object on stdout (progress on stderr):

```bash
rs-guard --format json --dry-run
# or
export RS_GUARD_FORMAT=json
```

Fields include `verdict`, severity counts, `state`, `provider`, `model`, `estimated_tokens_in`, `estimated_tokens_out`, `token_source` (`"api"`, `"mixed"`, or `"estimate"`), `latency_secs`, `estimated_cost_cents`, `diff_lines`, `project_rules_file`, and `dry_run`. Default remains `text`.

**`token_source`** indicates where the token counts came from: `"api"` when the provider returned `usage` data with both prompt and completion tokens, `"mixed"` when only one direction was reported (the other is estimated), or `"estimate"` when no usage data was available (character-based heuristic). This lets CI dashboards distinguish real token costs from estimates.

---

## Structured Findings (v1.7)

When `--findings` is enabled, rs-guard asks the LLM to emit a structured JSON array of findings instead of (or in addition to) free-form prose. Each finding includes the file path, line number, severity, and a human-readable message.

**Enable via:**

| Source | Value |
|--------|-------|
| CLI | `--findings` |
| Environment | `RS_GUARD_FINDINGS=true` |
| TOML | _(not available — CLI/env only)_ |

When findings are present, rs-guard derives `critical`, `security`, `important`, and `suggestion` counts using a **max-rule** merge: `max(metadata_count, findings_count)` per severity. This means findings can add evidence but never suppress a blocking preliminary verdict or down-count a blocking severity. When no findings block is present, the existing metadata tag counting is used. Findings that cannot be mapped to a diff position are appended to the overview body — they are never silently dropped.

**JSON output integration:** When `--format json` is also enabled, the `ReviewResultJson` object includes a `findings` array alongside the existing severity counts.

---

## Inline Comments (v1.7)

When `--inline-comments` is enabled, rs-guard posts each structured finding as an inline review comment on the specific file and line in the PR diff. A prose overview comment is posted as the review body.

**Enable via:**

| Source | Value |
|--------|-------|
| CLI | `--inline-comments` |
| Environment | `RS_GUARD_INLINE_COMMENTS=true` |
| TOML | _(not available — CLI/env only)_ |

`--inline-comments` implies `--findings` — you do not need to pass both.

Findings that cannot be mapped to a diff position (e.g., the file or line is not in the diff) are appended to the review body as a bulleted list, prefixed with the file path.

---

## Check Runs (v1.7)

When `--check-run` is enabled, rs-guard creates a GitHub Check Run **in addition to** the PR review. This is useful when your GitHub token cannot `APPROVE` or `REQUEST_CHANGES` (e.g., the default `GITHUB_TOKEN` in many workflows), because Check Runs can be used as branch protection status checks without requiring review permissions.

**Enable via:**

| Source | Value |
|--------|-------|
| CLI | `--check-run` |
| Environment | `RS_GUARD_CHECK_RUN=true` |
| TOML | `check_run = true` |

**Custom Check Run name:**

| Source | Value |
|--------|-------|
| CLI | `--check-run-name "My Review"` |
| Environment | `RS_GUARD_CHECK_RUN_NAME="My Review"` |
| TOML | `check_run_name = "My Review"` |

**Conclusion mapping:**

| Review State | Check Run Conclusion |
|---|---|
| `APPROVE` | `success` |
| `REQUEST_CHANGES` | `failure` |
| `COMMENT` | `neutral` |

A Check Run creation failure is logged as a warning but does **not** fail the pipeline — the PR review is still submitted normally.

**Permissions:** Check Runs require the `checks: write` permission on the GitHub token. See [docs/GITHUB_BOT_SETUP.md](GITHUB_BOT_SETUP.md) for the full permission matrix.

---

## Multi-pass Review (v1.8)

For large diffs that exceed a single context window, rs-guard can split the
diff by file sections into chunks, review each chunk independently with
bounded concurrency, and aggregate the per-chunk verdicts into a single
GitHub review.

### Enabling multi-pass

```bash
# CLI
rs-guard --multi-pass --multi-pass-max-chunks 10 --multi-pass-max-concurrent 3

# Environment variable
RS_GUARD_MULTI_PASS=1 rs-guard

# TOML
multi_pass = true
multi_pass_max_chunks = 10
multi_pass_max_concurrent = 3
multi_pass_max_cost_cents = 50.0  # optional cost guard
```

### Controls

| Option | Default | Description |
|--------|---------|-------------|
| `--multi-pass` | `false` | Enable multi-pass review |
| `--multi-pass-max-chunks` | `10` | Maximum number of chunks to split the diff into |
| `--multi-pass-max-concurrent` | `3` | Maximum concurrent LLM calls |
| `--multi-pass-max-cost-cents` | (disabled) | Abort before LLM calls if estimated total cost exceeds this cap |

### Aggregation

Severity counts are summed across chunks, findings are concatenated, and the
final verdict is `NEGATIVE` if any chunk is blocking. Partial failures are
tolerated — successful chunks are reviewed and the failure count is noted in
the review body.

### Metrics

The `rs-guard-metrics.json` file includes `multi_pass_chunk_count` and
`multi_pass_failed_chunks` fields when multi-pass is active. Every run also
records error-path counters: `verdict_parse_errors`, `budget_escalations`,
`cache_hits`, `cache_misses`, `diff_chunked`, and `diff_removed_lines`.

### Cost warning

Multi-pass increases LLM API costs proportionally to the number of chunks.
Use `--multi-pass-max-cost-cents` to set a hard cap. The cost estimate is
based on diff size and provider pricing; if pricing is unknown, the guard
is skipped.

---

## Ignore File (v1.8)

rs-guard supports a `.rs-guardignore` file (gitignore syntax) for excluding
paths from the review diff before size checks and LLM review. Patterns are
parsed by `parse_rs_guard_ignore()` and applied by
`apply_path_filters_with_ignore()` in `src/diff.rs` (`ignore_file` in
`docs/CONFIGURATION.md`).

### Usage

```bash
# Auto-loaded from repo root in local mode
echo "Cargo.lock" >> .rs-guardignore
echo "src/generated/" >> .rs-guardignore

# Explicit path via CLI
rs-guard --ignore-file .rs-guardignore

# Via environment variable
RS_GUARD_IGNORE_FILE=.rs-guardignore rs-guard

# Via TOML
ignore_file = ".rs-guardignore"
```

### Pattern syntax

Supports gitignore-style patterns: globs (`*`, `**`), directory suffixes
(`/`), and negation (`!`).

### Security in CI mode

**In CI mode, the repo-root `.rs-guardignore` and TOML-sourced `ignore_file`
are NOT loaded** — only `--ignore-file` / `RS_GUARD_IGNORE_FILE` from the
trusted workflow are honored. This prevents PR authors from suppressing
review of their own changes.

### Scaffolding

`rs-guard init` scaffolds a starter `.rs-guardignore` with common lockfile
and generated-code patterns.

---

## Language-aware Prompt Selection (v1.8)

When no explicit `--prompt-file` is provided, rs-guard inspects changed file
extensions and auto-selects a built-in prompt template:

| Detected language | Template |
|---|---|
| Frontend (JS/TS/CSS/HTML) | Frontend SPA review prompt |
| Backend (Python/Ruby/Go/Java) | Backend API review prompt |
| CLI (Rust/Shell) | CLI tooling review prompt |
| Mixed / unknown | General review prompt |

### Disabling

```bash
rs-guard --no-auto-prompt
RS_GUARD_NO_AUTO_PROMPT=1 rs-guard
# TOML
auto_prompt = false
```

Explicit `--prompt-file` always takes precedence over auto-selection.

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
    timeout-minutes: 15
    if: ${{ !github.event.pull_request.head.repo.fork }}
    steps:
      # Pinned from actions/checkout@v5 (93cb6efe) to avoid Node.js 20 deprecation.
      - uses: actions/checkout@93cb6efe18208431cddfb8368fd83d5badbf9bfd

      - uses: dtolnay/rust-toolchain@e97e2d8cc328f1b50210efc529dca0028893a2d9
        with:
          toolchain: stable
      - name: Cache cargo build
        uses: Swatinem/rust-cache@49a0bdc70d2e1b713ca9e2869b211fcce03d3c1c
      - name: Install rs-guard
        run: cargo install rs-guard --locked --version "1.8.2"

      - name: AI Code Review
        run: rs-guard --llm-timeout 240
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
    timeout-minutes: 15
    if: ${{ !github.event.pull_request.head.repo.fork }}
    steps:
      # Pinned from actions/checkout@v5 (93cb6efe) to avoid Node.js 20 deprecation.
      - uses: actions/checkout@93cb6efe18208431cddfb8368fd83d5badbf9bfd

      - uses: dtolnay/rust-toolchain@e97e2d8cc328f1b50210efc529dca0028893a2d9
        with:
          toolchain: stable
      - name: Cache cargo build
        uses: Swatinem/rust-cache@49a0bdc70d2e1b713ca9e2869b211fcce03d3c1c
      - name: Install rs-guard
        run: cargo install rs-guard --locked --version "1.8.2"

      - name: AI Code Review
        run: rs-guard --config .reviewer.toml
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
- **Job timeout:** set `timeout-minutes: 15` on the review job. DeepSeek/Kimi thinking can take up to 240s (v1.8.3 auto-floor); full HTTP timeouts are not retried.

### GitHub Check Runs (branch protection)

By default rs-guard submits a PR review (`APPROVE` / `REQUEST_CHANGES` / `COMMENT`). `GITHUB_TOKEN` often cannot `APPROVE` or `REQUEST_CHANGES`, which blocks branch-protection rules that require a review state. Enable Check Runs to publish a status check that branch protection can require independently:

```yaml
permissions:
  pull-requests: write
  contents: read
  checks: write          # required for Check Runs

jobs:
  review:
    runs-on: ubuntu-latest
    timeout-minutes: 15
    if: ${{ !github.event.pull_request.head.repo.fork }}
    steps:
      - uses: actions/checkout@93cb6efe18208431cddfb8368fd83d5badbf9bfd
      - uses: dtolnay/rust-toolchain@e97e2d8cc328f1b50210efc529dca0028893a2d9
        with:
          toolchain: stable
      - name: Cache cargo build
        uses: Swatinem/rust-cache@49a0bdc70d2e1b713ca9e2869b211fcce03d3c1c
      - name: Install rs-guard
        run: cargo install rs-guard --locked --version "1.8.2"
      - name: AI Code Review
        run: rs-guard --check-run --llm-timeout 240
        env:
          DEEPSEEK_API_KEY: ${{ secrets.DEEPSEEK_API_KEY }}
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          PR_NUMBER: ${{ github.event.pull_request.number }}
          REPO_FULL_NAME: ${{ github.repository }}
```

The Check Run conclusion is derived from the verdict: `APPROVE`→`success`, `REQUEST_CHANGES`→`failure`, `COMMENT`→`neutral`. Check Run creation failure is **non-fatal** — it is logged as a warning and the review still completes. If you rely on the Check Run for a required branch-protection check, monitor creation failures.

The head SHA is resolved automatically from `GITHUB_EVENT_PATH` (`pull_request.head.sha`) for PR events — you do not need to pass it explicitly. Use `--check-run-sha` (or `RS_GUARD_CHECK_RUN_SHA`) only for non-GitHub-Actions CI. Customize the name with `--check-run-name` (default `rs-guard`).

> **Caveat:** because Check Run creation is non-fatal, a required branch-protection check may silently fail to be created, leaving PRs blocked or without the expected status. Alert on the `Failed to create Check Run` log line if you depend on it.

---

## GitLab CI and other forges

CI mode is **GitHub-only**. It is detected via `GITHUB_ACTIONS=true` and talks to the GitHub API (fetch PR diff, post a review / Check Run). There is no GitLab Merge Request client.

On GitLab CI you can still fail the pipeline on a blocking verdict using **diff-file / local** mode (exit code `2` = `REQUEST_CHANGES`). This does **not** post a comment on the MR:

```yaml
review:
  stage: test
  rules:
    - if: $CI_PIPELINE_SOURCE == "merge_request_event"
  script:
    - cargo install rs-guard --locked --version "1.8.2"
    - git fetch origin $CI_MERGE_REQUEST_TARGET_BRANCH_NAME
    - git diff origin/$CI_MERGE_REQUEST_TARGET_BRANCH_NAME...HEAD > /tmp/mr.diff
    - rs-guard --diff-file /tmp/mr.diff --llm-timeout 240
  artifacts:
    when: always
    paths:
      - rs-guard-metrics.json
      - review-result.txt
```

Set the provider API key as a GitLab CI/CD variable (e.g. `DEEPSEEK_API_KEY`). Do not set `GITHUB_ACTIONS`.

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

rs-guard
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

### Prompt Composition Model

rs-guard composes the final review prompt in three layers. Each layer can be used independently or combined:

```
┌─────────────────────────────────────────────────────┐
│  Layer 3: Project Rules (auto-detected or --rules-file)  │
│  AGENTS.md, CLAUDE.md, .cursor/rules/*.md, etc.     │
│  → Appended as "Project Conventions" section        │
├─────────────────────────────────────────────────────┤
│  Layer 2: Custom Prompt (--prompt-file)             │
│  .github/review-prompt.md or any path               │
│  → Replaces the default entirely                    │
├─────────────────────────────────────────────────────┤
│  Layer 1: DEFAULT_PROMPT (built-in, src/config.rs)  │
│  Generic five-axis review, language-agnostic        │
│  → Used when no --prompt-file is specified          │
└─────────────────────────────────────────────────────┘
```

| Layer | What it is | When it applies | How to customize |
|---|---|---|---|
| **1. Default** | `DEFAULT_PROMPT` in `src/config.rs` — generic five-axis review | Always, unless overridden by Layer 2 | Copy a template from `examples/prompts/` and adapt |
| **2. Custom prompt** | A markdown file with project-specific review rules | When `--prompt-file` is set or `.github/review-prompt.md` exists | Write your own or start from a template |
| **3. Project rules** | Auto-detected convention files (`AGENTS.md`, `CLAUDE.md`, etc.) | Always, unless `--no-project-rules` is set | Maintain your existing agent instruction files |

**Think of Layer 2 as a review skill** — it focuses the LLM on what matters for *your* project.
A generic prompt reviews any codebase; a custom prompt reviews *yours*. The rs-guard repository
itself uses `.github/review-prompt.md` as its own review skill (see it for a real-world example
of a specialized prompt with architecture invariants, security rules, and blocking conditions).

**When to use each layer:**

- **Just Layer 1** — quick start, no customization needed. Works for any language.
- **Layers 1 + 3** — generic review + your project conventions. Good for teams that already
  maintain `AGENTS.md` or similar files.
- **Layers 2 + 3** — specialized review + project conventions. Best for projects with
  domain-specific review concerns (security-critical code, performance-sensitive paths,
  framework-specific anti-patterns).
- **Layer 2 only** — specialized review without auto-detected conventions. Use `--no-project-rules`.

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

The picker is skipped in CI mode (`is_ci = true`), when `--rules-file` is set, when `--no-project-rules` is set, when stdin is not a TTY, or when a custom prompt file is loaded (via `--prompt-file` or the default `.github/review-prompt.md`). In those cases, rs-guard falls back to first-match priority silently.

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

      - uses: dtolnay/rust-toolchain@e97e2d8cc328f1b50210efc529dca0028893a2d9
        with:
          toolchain: stable
      - name: Cache cargo build
        uses: Swatinem/rust-cache@49a0bdc70d2e1b713ca9e2869b211fcce03d3c1c
      - name: Install rs-guard
        run: cargo install rs-guard --locked --version "1.8.2"

      - name: AI Code Review
        run: rs-guard
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

Install rs-guard:

```bash
cargo install rs-guard --locked --version "1.8.2"
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

### `Review body was truncated …`

GitHub has a 65536 character limit for review bodies. As of v1.7, rs-guard
**truncates** the review body on a UTF-8 char boundary and appends a visible
`…[truncated: review body exceeds GitHub's 65,536 character limit]` notice so
the review is always submitted (never failed solely for being too long). The
findings JSON block is stripped before truncation, so only prose is cut.

If you see this notice frequently:

- Use a shorter prompt (e.g., remove detailed instructions)
- The diff will be chunked automatically for large PRs
- Consider using `--max-tokens` to limit LLM output length

### `Cache hit — using cached LLM response`

The same diff+prompt+provider+model+temperature combination was cached within the 24-hour TTL. Pass `--no-cache` for a fresh call.

### Empty assistant content / "reasoning may have consumed the token budget" (DeepSeek / Kimi)

Thinking models return `content: null` (or empty) + `reasoning_content` when the output budget is exhausted by chain-of-thought before the final answer.

rs-guard detects this shape and **automatically escalates**: instead of blindly retrying the identical request, `max_tokens` is doubled (16,384 → 32,768 → 65,536 cap) and the request is re-sent. Empty content **without** any `reasoning_content` is treated as a plain transient error and retried up to 3 times with backoff. Responses are **never cached** until a successful verdict parse, and a successful escalated response is cached under your original configuration so the escalation is not repeated.

**Fixes if escalation still fails at the cap:**
- Raise `max_tokens` explicitly (env `RS_GUARD_MAX_TOKENS`, `--max-tokens`, or TOML). For deepseek/kimi the default is auto-raised to 16,384 **only** when you have not set an explicit value.
- Consider `--llm-timeout 240` or `RS_GUARD_LLM_TIMEOUT=240` if the model needs more wall time for reasoning.
- Use a cheaper/faster variant when possible (e.g. `flash`).

You will see a warning in logs containing the length of `reasoning_content` and the escalation steps.

### LLM request timing out

Increase `--llm-timeout` / `RS_GUARD_LLM_TIMEOUT` (seconds). The default is 120s; **deepseek** and **kimi** auto-raise to **240s** when unset (v1.8.3). Also set the GitHub Actions job `timeout-minutes` above that (15 is enough). Logs distinguish `Request timed out` from `Failed to decode LLM response body (not a timeout)`.

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
