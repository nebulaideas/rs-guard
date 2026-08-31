# Configuration Reference

This document describes the `.reviewer.toml` configuration file and the configuration resolution order used by rs-guard.

---

## Configuration Resolution Order

rs-guard resolves configuration values in the following priority (highest to lowest):

```
CLI flags > Environment variables > TOML file > Hardcoded defaults
```

### Example

If your `.reviewer.toml` sets `provider = "kimi"`, but you run:

```bash
export RS_GUARD_PROVIDER="openai"
rs-guard --provider qwen
```

The effective provider will be `qwen` (CLI flag wins).

---

## `.reviewer.toml` Schema

Place `.reviewer.toml` in your repository root (or pass `--config /path/to/config.toml`).

```toml
# Top-level settings
provider = "deepseek"           # LLM provider: deepseek | kimi | qwen | openrouter | openai | grok | glm | ollama | gemini
model = "deepseek-v4-flash"     # Model identifier (provider-specific)
variant = "flash"               # Provider-specific model variant (e.g. "flash", "pro" for deepseek). Optional.
temperature = 0.1               # Sampling temperature (0.0 to 2.0)
max_tokens = 8192               # Maximum tokens for LLM completion
llm_timeout_secs = 180          # Total timeout for LLM HTTP calls in seconds (default 120)

# GitHub-native UX (v1.7)
# check_run = true               # Publish a GitHub Check Run (requires checks: write)
# check_run_name = "rs-guard"    # Custom Check Run name
# Note: --findings and --inline-comments are CLI/env only (NOT TOML keys).

# Example for deepseek-v4-pro (complex reasoning)
# variant = "pro"
# max_tokens = 16384
# llm_timeout_secs = 180

# Per-provider configuration
[providers.deepseek]
api_key_env = "DEEPSEEK_API_KEY"
base_url = "https://api.deepseek.com"

[providers.kimi]
api_key_env = "KIMI_API_KEY"
base_url = "https://api.moonshot.ai/v1"

[providers.qwen]
api_key_env = "DASHSCOPE_API_KEY"
base_url = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
# result_format = "message"  # Optional override; Qwen defaults to "message" in code

[providers.openrouter]
api_key_env = "OPENROUTER_API_KEY"
base_url = "https://openrouter.ai/api/v1"
http_referer = "https://github.com/nebulaideas/rs-guard"

[providers.openai]
api_key_env = "OPENAI_API_KEY"
base_url = "https://api.openai.com/v1"
# result_format = "json_object"  # Only for custom OpenAI-compatible endpoints

[providers.grok]
api_key_env = "XAI_API_KEY"
base_url = "https://api.x.ai/v1"
# model = "grok-3"

[providers.glm]
api_key_env = "ZHIPUAI_API_KEY"
base_url = "https://open.bigmodel.cn/api/paas/v4"
# model = "glm-4"

[providers.ollama]
# Ollama runs locally — no API key required by default.
# Loopback only; rejected in CI mode (unset GITHUB_ACTIONS to use locally).
base_url = "http://127.0.0.1:11434/v1"
# model = "llama3.2"
# api_key_env = "OLLAMA_API_KEY"  # Only needed if Ollama auth proxy is enabled

[providers.gemini]
api_key_env = "GEMINI_API_KEY"
base_url = "https://generativelanguage.googleapis.com/v1beta/openai"
# model = "gemini-2.5-flash"
```

### Field Reference

#### Top-Level Fields

| Field               | Type    | Default           | Description                                                                                                                       |
| ------------------- | ------- | ----------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `provider`          | string  | `"deepseek"`      | LLM provider to use.                                                                                                              |
| `model`             | string  | provider-specific | Model identifier. See [PROVIDERS.md](PROVIDERS.md) for defaults.                                                                  |
| `variant`           | string  | (none)            | Provider-specific model variant (e.g. "flash" / "pro" for deepseek). See [PROVIDERS.md](PROVIDERS.md). CLI/env/TOML precedence applies. |
| `temperature`       | float   | `0.1`             | Sampling temperature (0.0 = deterministic, 2.0 = very random).                                                                    |
| `max_tokens`        | integer | `4096`            | Maximum tokens in the LLM response. Defaults to 4096 (or 16,384 for deepseek/kimi when not explicit) to prevent the verdict block from being truncated. |
| `llm_timeout_secs`  | integer | `120` (180 for deepseek/kimi) | Total timeout (seconds) for LLM `/chat/completions` HTTP requests. Auto-raised to 180s for `deepseek` and `kimi` when not explicitly set (to support reasoning models like deepseek-v4-pro). Increase further for complex PRs. |
| `chunk_head_lines`  | integer | `400`             | Lines preserved from the **start** of the diff when chunking. Increase for providers with large context windows (e.g. 128K).      |
| `chunk_tail_lines`  | integer | `400`             | Lines preserved from the **end** of the diff when chunking. Combined default of 800 covers most PRs without truncation.           |
| `cache_dir`         | string  | `.rs-guard/cache` | Custom cache directory path. Defaults to git-root (or CWD) relative `.rs-guard/cache`.                                           |
| `auto_gitignore`    | boolean | `true`            | Whether to automatically add the cache directory to `.gitignore`.                                                                |
| `important_issues_threshold` | integer | `3`      | Number of `[Important]` issues required to trigger `REQUEST_CHANGES`. `0` disables blocking on important issues (they still surface as `COMMENT`). |
| `project_rules_enabled` | boolean | `true` | Whether to scan for and load project rules files. Set to `false` to disable auto-detection. Can also be disabled via `--no-project-rules` CLI flag or `RS_GUARD_NO_PROJECT_RULES` env var (any non-empty value disables). |
| `rules_file` | string | (none) | Path to an explicit project rules file. Overrides auto-detection. Mutually exclusive with `--no-project-rules` / `RS_GUARD_NO_PROJECT_RULES`. Can also be set via `--rules-file` CLI flag or `RS_GUARD_RULES_FILE` env var. |
| `diff_base` | string | (none) | Local mode only: git base ref for three-dot range review (`git diff <base>...HEAD`) instead of staged changes. Equivalent to `--base` / `RS_GUARD_BASE`. Ignored in CI mode. Blank values are treated as unset. |
| `output_format` | string | `\"text\"` | Output format: `\"text\"` or `\"json\"`. Equivalent to `--format` / `RS_GUARD_FORMAT`. |
| `check_run` | boolean | `false` | Create a GitHub Check Run in addition to the PR review. Requires `checks: write` permission. Equivalent to `--check-run` / `RS_GUARD_CHECK_RUN`. |
| `check_run_name` | string | `\"rs-guard\"` | Custom name for the GitHub Check Run. Equivalent to `--check-run-name` / `RS_GUARD_CHECK_RUN_NAME`. |
| `ignore_file` | string | `.rs-guardignore` | Path to a `.rs-guardignore` file with gitignore-style patterns for excluding paths from the review diff. Parsed by `parse_rs_guard_ignore()` and applied by `apply_path_filters_with_ignore()` in `src/diff.rs`. Equivalent to `--ignore-file` / `RS_GUARD_IGNORE_FILE`. **In CI mode, the default repo-root path is NOT auto-loaded** — an explicit path must be provided to prevent PR-controlled ignore patterns from bypassing review. See `docs/USAGE.md` §Ignore File. |
| `auto_prompt` | boolean | `true` | Whether to auto-select a language-aware prompt template based on changed file extensions. Disabled via `--no-auto-prompt` / `RS_GUARD_NO_AUTO_PROMPT=1`. Explicit `--prompt-file` always wins. |

#### Provider Section Fields

| Field            | Type   | Required | Description                                                                     |
| ---------------- | ------ | -------- | ------------------------------------------------------------------------------- |
| `api_key_env`    | string | no       | Environment variable name for the API key. Defaults to provider-standard names. |
| `base_url`       | string | no       | Custom API base URL. Defaults to provider's official endpoint.                  |
| `http_referer`   | string | no       | Attribution referer (OpenRouter only).                                          |
| `variant`        | string | no       | Provider-specific model variant override for this provider.                     |
| `result_format`  | string | no       | Override the `result_format` field sent to the provider (e.g. `"message"`, `"json_object"`). Useful for custom OpenAI-compatible endpoints. |

> **Note on `api_key_required`:** Whether a provider requires an API key is determined by its `ProviderMeta` entry in `src/llm/providers.rs`, not by TOML. Providers with `api_key_required = false` (e.g. Ollama) treat a missing or empty env var as an empty string and skip the `Authorization` header. Providers with `api_key_required = true` (all others) error if the key is missing or empty.

#### Circuit Breaker Section (`[circuit_breaker]`)

Optional. Enables a circuit breaker to stop retrying after repeated LLM failures. Disabled by default.

| Field           | Type    | Default | Description                                                    |
| --------------- | ------- | ------- | -------------------------------------------------------------- |
| `enabled`       | boolean | `false` | Whether the circuit breaker is active.                         |
| `threshold`     | integer | `3`     | Consecutive failures required to open the circuit.             |
| `cooldown_secs` | integer | `60`    | Seconds before the open circuit auto-resets to closed.         |

Example:
```toml
[circuit_breaker]
enabled = true
threshold = 3
cooldown_secs = 60
```

#### Pricing Section (`[pricing.<provider>]`)

Optional. Override default cost estimates for providers. Prices are in **cents per million tokens**.
Consumed by `estimate_cost_cents()` / `default_pricing()` in `src/pipeline.rs`
(see `docs/implementation-guide.md` §Integer Cents for Cost Calculation).

| Field               | Type    | Default            | Description                              |
| ------------------- | ------- | ------------------ | ---------------------------------------- |
| `input_per_million` | integer | provider-specific  | Cost in cents per 1M input tokens.       |
| `output_per_million`| integer | provider-specific  | Cost in cents per 1M output tokens.      |

Example:
```toml
[pricing.deepseek]
input_per_million = 7    # $0.07 per 1M input tokens
output_per_million = 27  # $0.27 per 1M output tokens

[pricing.openai]
input_per_million = 15   # $0.15 per 1M input tokens
output_per_million = 60  # $0.60 per 1M output tokens
```

---

## Common Configuration Mistakes

rs-guard validates `.reviewer.toml` and reports friendly errors for the following mistakes:

### `[provider.X]` instead of `provider = "X"`

**Incorrect:**

```toml
[provider.deepseek]
api_key_env = "DEEPSEEK_API_KEY"
```

**Correct:**

```toml
provider = "deepseek"

[providers.deepseek]
api_key_env = "DEEPSEEK_API_KEY"
```

`provider` is a top-level string that selects the default provider. Per-provider overrides use
the plural table name `[providers.<name>]`.

### Unknown top-level keys

Typos such as `providor = "deepseek"` are detected and rs-guard suggests the closest valid key
(`provider`). The full list of valid top-level keys is shown in the error message.

### Non-string `provider`

`provider` must be a quoted string:

```toml
provider = "deepseek"  # correct
provider = deepseek    # incorrect
```

---

## Verdict Behavior

The review state submitted to GitHub is determined by counting severity-tagged findings in the
LLM response:

| Condition | GitHub event |
| --------- | ------------ |
| `NEGATIVE` verdict, or any `[Critical]` / `[Security]` finding | `REQUEST_CHANGES` |
| `important_issues >= important_issues_threshold` (default `3`, configurable) | `REQUEST_CHANGES` |
| `important_issues` is between `1` and `important_issues_threshold - 1` | `COMMENT` |
| No issues | `APPROVE` |

Configure the threshold via:

- CLI: `--important-threshold 1`
- Environment: `RS_GUARD_IMPORTANT_THRESHOLD=1`
- TOML: `important_issues_threshold = 1`

Setting `important_issues_threshold = 0` disables blocking on `[Important]` issues — they will
still surface as `COMMENT`, but will never trigger `REQUEST_CHANGES`.

---

## Structured Findings & Inline Comments

rs-guard can ask the LLM for a structured `[RS_GUARD_VERDICT_FINDINGS]` JSON block at the end of
its response, enabling per-file, per-line issue tracking. Two flags control this behavior:

| Flag / Env | Description |
| ---------- | ----------- |
| `--findings` / `RS_GUARD_FINDINGS` | Appends findings-format instructions to the review prompt. The LLM is asked to emit a `[RS_GUARD_VERDICT_FINDINGS]` JSON array. When findings are present, severity counts are derived from them (overriding metadata-block counts) using a **max-rule** merge: findings can add evidence but never suppress a blocking preliminary verdict or down-count a blocking severity. |
| `--inline-comments` / `RS_GUARD_INLINE_COMMENTS` | Maps structured findings to diff positions and submits them as inline GitHub review comments. Unmappable findings are appended to the review body as bullet points. **Implies `--findings`.** |

### Implication rule

`--inline-comments` implies `--findings`. The implication is enforced in `Config::apply_args`
after CLI/env resolution, so setting `--inline-comments` (or `RS_GUARD_INLINE_COMMENTS`) always
turns on `findings` as well. The reverse is not true: `--findings` alone does not enable inline
comments.

### Cache interaction

The `findings` flag changes the review prompt, so it is part of the cache key. A response cached
with `findings=false` is never returned when `findings=true` is requested (and vice versa).
`--inline-comments` only affects post-LLM submission, so it is **not** part of the cache key.

### TOML

These flags are CLI/env only; they are **not** available in `.reviewer.toml`. Setting `findings`
or `inline_comments` as top-level TOML keys is rejected as an unknown key.

---

## GitHub Check Runs

rs-guard can publish a GitHub Check Run in addition to the PR review, so branch
protection does not depend on `APPROVE` / `REQUEST_CHANGES` permissions (which
`GITHUB_TOKEN` often cannot grant). The Check Run conclusion is derived from the
review state: `APPROVE`→`success`, `REQUEST_CHANGES`→`failure`, `COMMENT`→`neutral`.

| Flag / Env / TOML | Description |
| ----------------- | ----------- |
| `--check-run` / `RS_GUARD_CHECK_RUN` / `check_run` | Create a GitHub Check Run after submitting the review. Check Run failure is non-blocking (logged as a warning). Requires `checks: write` permission. |
| `--check-run-name` / `RS_GUARD_CHECK_RUN_NAME` / `check_run_name` | Name for the Check Run (default: `rs-guard`). Validated non-empty and ≤ 255 chars. |
| `--check-run-sha` / `RS_GUARD_CHECK_RUN_SHA` | Explicit commit SHA for the Check Run. When omitted, rs-guard resolves the SHA from `GITHUB_EVENT_PATH` (`pull_request.head.sha`) for PR events, falling back to `GITHUB_SHA`. |
| `--ignore-file` / `RS_GUARD_IGNORE_FILE` / `ignore_file` | Path to a `.rs-guardignore` file with gitignore-style patterns. Matching paths are excluded from the review diff before size checks and LLM review. Defaults to `.rs-guardignore` in the repo root. |
| `--no-auto-prompt` / `RS_GUARD_NO_AUTO_PROMPT=1` / `auto_prompt` | Disable language-aware prompt auto-selection. When enabled (default), rs-guard inspects changed file extensions and selects a built-in prompt template (frontend, backend, CLI, or general). Explicit `--prompt-file` always takes precedence. |

### SHA resolution

For `pull_request` / `pull_request_target` events, GitHub Actions sets
`GITHUB_SHA` to the **synthetic merge commit** (`refs/pull/<n>/merge`), not the
PR head SHA. Check Runs must target the PR head SHA to attach to the PR's Checks
tab, so rs-guard reads `pull_request.head.sha` from the event payload at
`GITHUB_EVENT_PATH` first. Use `--check-run-sha` to override (e.g. for
non-GitHub-Actions CI). `GITHUB_SHA` is the final fallback for push events.

### Idempotent retries

The Check Run is created with a stable `external_id`
(`rs-guard:<sha>:<conclusion>`), so a request that succeeds but whose response
is lost will not create a duplicate on retry — GitHub deduplicates by
`external_id`.

### Precedence

`--check-run` respects CLI > env > TOML > defaults. An explicit
`RS_GUARD_CHECK_RUN=false` overrides a TOML `check_run = true`.

### TOML

```toml
check_run = true
check_run_name = "rs-guard"
```

---

## CLI Flags

These flags are available at the top level for the default review command:

| Flag            | Short | Default                    | Description                          |
| --------------- | ----- | -------------------------- | ------------------------------------ |
| `--prompt-file` | `-p`  | `.github/review-prompt.md` | Path to system prompt markdown file. A loaded custom prompt file suppresses the interactive project-rules picker. |
| `--model`       | `-m`  | provider-specific          | LLM model identifier.                |
| `--temperature` | `-t`  | `0.1`                      | Sampling temperature (0.0 - 2.0).    |
| `--provider`    |       | `deepseek`                 | LLM provider to use.                 |
| `--variant`     |       | (none)                     | Provider-specific model variant (e.g. flash/pro). Has no effect if provider does not support it. |
| `--config`      | `-c`  | `.reviewer.toml`           | Path to configuration TOML file.     |
| `--max-tokens`  |       | `4096`                     | Maximum tokens for LLM completions.  |
| `--llm-timeout` |       | `120`                      | Timeout in seconds for LLM API requests. |
| `--important-threshold` | | `3`                    | `[Important]` issues required to `REQUEST_CHANGES`. |
| `--no-cache`    |       | Off                        | Bypass response cache.               |
| `--dry-run`     |       | Off                        | Run without submitting or blocking.  |
| `--base`        |       | (none)                     | Local mode: review `git diff <base>...HEAD` instead of staged changes. |
| `--findings`    |       | Off                        | Request a `[RS_GUARD_VERDICT_FINDINGS]` JSON block from the LLM. Findings override metadata-block severity counts when present. Also set via `RS_GUARD_FINDINGS`. |
| `--inline-comments` | | Off                     | Submit inline review comments on the PR diff mapped from structured findings. Implies `--findings`. Also set via `RS_GUARD_INLINE_COMMENTS`. |
| `--check-run`   |       | Off                        | Create a GitHub Check Run (conclusion derived from verdict). Also set via `RS_GUARD_CHECK_RUN` / `check_run` in TOML. Requires `checks: write`. |
| `--check-run-name` |   | `rs-guard`                | Name for the Check Run. Also set via `RS_GUARD_CHECK_RUN_NAME` / `check_run_name` in TOML. |
| `--check-run-sha` |    | (auto)                    | Commit SHA for the Check Run (overrides auto-detection from `GITHUB_EVENT_PATH` / `GITHUB_SHA`). Also set via `RS_GUARD_CHECK_RUN_SHA`. |
| `--help`        | `-h`  |                            | Display help.                        |
| `--version`     | `-V`  |                            | Display version.                     |

### Subcommands

rs-guard also provides setup-automation subcommands:

```bash
rs-guard init                              # Scaffold workflow, prompt, and config
rs-guard generate-prompt --template rust   # Generate a review prompt
rs-guard generate-workflow --provider kimi # Generate a GitHub Actions workflow
rs-guard validate-config                   # Preflight configuration check
```

Run `rs-guard <subcommand> --help` for details on each subcommand.

---

## Environment Variables

| Variable                | Required By         | Description                              |
| ----------------------- | ------------------- | ---------------------------------------- |
| `DEEPSEEK_API_KEY`      | DeepSeek provider   | API key from DeepSeek platform.          |
| `KIMI_API_KEY`          | Kimi provider       | API key from Moonshot AI platform.       |
| `DASHSCOPE_API_KEY`     | Qwen provider       | API key from Alibaba Cloud DashScope.    |
| `OPENROUTER_API_KEY`    | OpenRouter provider | API key from OpenRouter.                 |
| `OPENAI_API_KEY`        | OpenAI provider     | API key from OpenAI.                     |
| `XAI_API_KEY`           | Grok provider       | API key from xAI.                       |
| `ZHIPUAI_API_KEY`       | GLM provider        | API key from Zhipu AI.                  |
| `OLLAMA_API_KEY`        | Ollama provider     | Optional. Only needed if Ollama auth proxy is enabled. |
| `GEMINI_API_KEY`        | Gemini provider     | API key from Google AI Studio.          |
| `GITHUB_TOKEN`          | GitHub mode         | Auto-provided by GitHub Actions.         |
| `PR_NUMBER`             | GitHub mode         | Pull request number.                     |
| `REPO_FULL_NAME`        | GitHub mode         | Repository in `owner/repo` format.       |
| `GITHUB_ACTIONS`        | Auto-detected       | Presence indicates CI mode.              |
| `RS_GUARD_PROVIDER`     | Optional            | Override TOML/default provider.          |
| `RS_GUARD_MODEL`        | Optional            | Override TOML/default model.             |
| `RS_GUARD_VARIANT`      | Optional            | Provider-specific model variant (CLI --variant equivalent). |
| `RS_GUARD_TEMPERATURE`  | Optional            | Override TOML/default temperature.       |
| `RS_GUARD_MAX_TOKENS`   | Optional            | Override TOML/default max tokens.        |
| `RS_GUARD_LLM_TIMEOUT`  | Optional            | Override TOML/default LLM timeout.       |
| `RS_GUARD_IMPORTANT_THRESHOLD` | Optional     | Override TOML/default important-issues threshold. |
| `GITHUB_API_URL`        | Optional            | Custom GitHub API base URL (Enterprise). |
| `RS_GUARD_DIFF_FILE`    | Optional            | Path to a pre-existing diff file.        |
| `RS_GUARD_BASE`         | Optional            | Local mode: base ref for `git diff <base>...HEAD` (same as `--base` / TOML `diff_base`). |
| `RS_GUARD_METRICS_PATH` | Optional            | Path for the metrics JSON artifact.      |
| `RS_GUARD_NO_PROJECT_RULES` | Optional        | Disables project rules auto-detection when set to **any non-empty value** (including `"false"` or `"0"`). To keep rules enabled, leave this variable unset. This matches the pattern used by `RS_GUARD_NO_CACHE`. |
| `RS_GUARD_RULES_FILE`   | Optional            | Path to an explicit project rules file. Overrides auto-detection. Mutually exclusive with `RS_GUARD_NO_PROJECT_RULES` / `--no-project-rules`. |
| `RS_GUARD_FINDINGS`     | Optional            | Request structured findings from the LLM (CLI `--findings` equivalent). |
| `RS_GUARD_INLINE_COMMENTS` | Optional         | Submit inline review comments on the PR diff (CLI `--inline-comments` equivalent). Implies `RS_GUARD_FINDINGS`. |
| `RS_GUARD_CHECK_RUN`    | Optional            | Create a GitHub Check Run (CLI `--check-run` equivalent). Accepts `true`/`false`/`1`/`0`; an explicit `false` overrides a TOML `check_run = true`. |
| `RS_GUARD_CHECK_RUN_NAME` | Optional          | Name for the Check Run (CLI `--check-run-name` equivalent). Default `rs-guard`. |
| `RS_GUARD_CHECK_RUN_SHA` | Optional           | Explicit commit SHA for the Check Run (CLI `--check-run-sha` equivalent). Overrides auto-detection from `GITHUB_EVENT_PATH` / `GITHUB_SHA`. |

---

## Minimal Configuration Example

For a team using Kimi:

```toml
# .reviewer.toml
provider = "kimi"
model = "kimi-k2.5"
temperature = 0.1
```

Team members only need to set their API key:

```bash
export KIMI_API_KEY="sk-..."
```

---

## Full Configuration Example

```toml
# .reviewer.toml
provider = "openrouter"
model = "anthropic/claude-3.5-sonnet"
temperature = 0.1
max_tokens = 8192
chunk_head_lines = 600   # Preserve more context for large PRs
chunk_tail_lines = 600

# GitHub-native UX (v1.7)
check_run = true             # Publish a GitHub Check Run
check_run_name = "rs-guard"  # Custom Check Run name
# Note: --findings and --inline-comments are CLI/env only, NOT TOML keys.

[providers.openrouter]
api_key_env = "OPENROUTER_API_KEY"
base_url = "https://openrouter.ai/api/v1"
http_referer = "https://github.com/my-org/my-repo"

# Stop retrying after 3 consecutive LLM failures
[circuit_breaker]
enabled = true
threshold = 3
cooldown_secs = 60

# Override cost estimates (cents per million tokens)
[pricing.openrouter]
input_per_million = 15
output_per_million = 60
```


## Diff size and path filters (v1.6)

| Key | Env | Default | Description |
|-----|-----|---------|-------------|
| `max_diff_bytes` | `RS_GUARD_MAX_DIFF_BYTES` | `512000` (500 KB) | Hard reject above this size |
| `max_diff_lines` | `RS_GUARD_MAX_DIFF_LINES` | `5000` | Hard reject above this line count |
| `include_paths` | `RS_GUARD_INCLUDE_PATHS` (comma-separated) | all | Only keep matching file sections |
| `exclude_paths` | `RS_GUARD_EXCLUDE_PATHS` (comma-separated) | none | Drop matching file sections |

Path filters run after the raw fetch and **before** the user-facing size gate
(`max_diff_bytes` / `max_diff_lines`) and chunking. The raw fetch only applies a
high safety ceiling (10 MB / 100k lines) so excluding large lockfiles can keep a
PR reviewable under the configured limits.

### Supported path pattern constructs

rs-guard uses a **small custom matcher** (not full gitignore / full glob):

| Pattern | Meaning | Example matches |
|---------|---------|-----------------|
| `path/to/file` | Exact path | `src/main.rs` |
| `*.ext` | Basename suffix | `Cargo.lock` via `*.lock` |
| `dir/**` | Directory prefix | `src/**` → `src/a.rs` |
| `**/name` | Any-depth suffix | `**/Cargo.lock` |
| `**/foo*` | Any-depth prefix on a segment | `pkg/foo_bar.rs` |
| `a/*/b` | Single `*` = exactly one path segment | `src/*/lib.rs` (not multi-level) |
| `*` or `**` alone | Match every path | Use carefully with include/exclude |

Patterns are case-sensitive and use `/` separators. Leading `./` is ignored.

Patterns that contain `/` (and no `*` / `**` operators already handled above) are
**exact path matches only** — `src/main.rs` does **not** match `vendor/src/main.rs`.
Basename-only patterns without `/` (e.g. `Cargo.lock`) match the final path component
at any depth.

**Unsupported:** patterns with more than one single-`*` wildcard (e.g. `**/foo*bar*`
or `a*b*c`) never match. Prefer simple documented forms only.


## Local branch-range review (`diff_base`)

In **local mode**, you can review a branch range instead of staged changes.
Implemented by `fetch_range_diff()` in `src/diff.rs`, which returns a
`DiffResult` (same type as `fetch_local_diff()`).

| Source | Key / flag |
|--------|------------|
| CLI | `--base <ref>` |
| Environment | `RS_GUARD_BASE` |
| TOML | `diff_base = "<ref>"` |

Precedence for the base ref value itself: CLI `--base` > `RS_GUARD_BASE` > TOML `diff_base`.  
Diff source precedence: `--diff-file` > CI mode > base range > staged diff.

See [LOCAL_MODE.md — Branch-range review](LOCAL_MODE.md#branch-range-review---base) for validation rules and examples.


## GitHub-native UX features (v1.7)

### Structured findings

| Source | Key |
|--------|-----|
| CLI | `--findings` |
| Environment | `RS_GUARD_FINDINGS` |
| TOML | _(not available — CLI/env only; rejected as unknown key)_ |

When enabled, rs-guard instructs the LLM to emit a `[RS_GUARD_VERDICT_FINDINGS]` JSON block after the prose overview. Severity counts (`critical`, `security`, `important`, `suggestion`) are derived from the findings array rather than tag counting. Findings counts use a **max-rule** merge: findings can add evidence but never suppress a blocking preliminary verdict.

### Inline review comments

| Source | Key |
|--------|-----|
| CLI | `--inline-comments` |
| Environment | `RS_GUARD_INLINE_COMMENTS` |
| TOML | _(not available — CLI/env only; rejected as unknown key)_ |

Posts each structured finding as an inline comment on the diff. Implies `--findings`. Findings that cannot be mapped to a diff position are appended to the review body.

### GitHub Check Runs

| Source | Key |
|--------|-----|
| CLI | `--check-run` / `--check-run-name` |
| Environment | `RS_GUARD_CHECK_RUN` / `RS_GUARD_CHECK_RUN_NAME` |
| TOML | `check_run = true` / `check_run_name = "rs-guard"` |

Creates a GitHub Check Run in addition to the PR review. Conclusion mapping: `APPROVE` → `success`, `REQUEST_CHANGES` → `failure`, `COMMENT` → `neutral`. Requires `checks: write` on the GitHub token.

## Multi-pass review (v1.8)

Multi-pass review splits the diff by file sections into chunks, reviews each chunk independently with bounded concurrency, and aggregates the per-chunk verdicts into a single review. This improves review quality for large diffs that would otherwise be truncated by head/tail chunking.

| Source | Key | Default |
|--------|-----|---------|
| CLI | `--multi-pass` | `false` |
| Environment | `RS_GUARD_MULTI_PASS=1` | `false` |
| TOML | `multi_pass = true` | `false` |
| CLI | `--multi-pass-max-chunks <N>` | `10` |
| Environment | `RS_GUARD_MULTI_PASS_MAX_CHUNKS` | `10` |
| TOML | `multi_pass_max_chunks = 10` | `10` |
| CLI | `--multi-pass-max-concurrent <N>` | `3` |
| Environment | `RS_GUARD_MULTI_PASS_MAX_CONCURRENT` | `3` |
| TOML | `multi_pass_max_concurrent = 3` | `3` |
| CLI | `--multi-pass-max-cost-cents <N>` | (disabled) |
| Environment | `RS_GUARD_MULTI_PASS_MAX_COST_CENTS` | (disabled) |
| TOML | `multi_pass_max_cost_cents = 50.0` | (disabled) |

### How it works

1. The filtered diff is split by `diff --git` file sections into chunks.
2. If the number of file sections exceeds `max_chunks`, sections are merged round-robin into fewer chunks.
3. Each chunk is reviewed independently using the same composed prompt.
4. LLM calls run concurrently, bounded by `max_concurrent`.
5. Per-chunk verdicts are aggregated: severity counts are summed, findings are concatenated, and the final verdict is `NEGATIVE` if any chunk is blocking.
6. A single aggregated review is submitted to GitHub.

### Cost guard

When `multi_pass_max_cost_cents` is set, rs-guard estimates the total cost of all chunk reviews before making any LLM calls. If the estimate exceeds the cap, the review aborts with an error. This prevents unexpected cost spikes on large PRs.

### Partial failure

If some chunks fail (LLM error, parse error, etc.), rs-guard continues with the successful chunks and notes the failure count in the review body. If all chunks fail, the pipeline returns an error.

### Metrics

The `rs-guard-metrics.json` file includes two multi-pass fields:

- `multi_pass_chunk_count` — Number of chunks reviewed (1 for single-pass).
- `multi_pass_failed_chunks` — Number of chunks that failed (0 for single-pass or full success).
