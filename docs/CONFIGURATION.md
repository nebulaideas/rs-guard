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
provider = "deepseek"           # LLM provider: deepseek | kimi | qwen | openrouter | openai
model = "deepseek-v4-flash"     # Model identifier (provider-specific)
temperature = 0.1               # Sampling temperature (0.0 to 2.0)
max_tokens = 8192               # Maximum tokens for LLM completion

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

[providers.openrouter]
api_key_env = "OPENROUTER_API_KEY"
base_url = "https://openrouter.ai/api/v1"
http_referer = "https://github.com/nebulaideas/rs-guard"

[providers.openai]
api_key_env = "OPENAI_API_KEY"
base_url = "https://api.openai.com/v1"
```

### Field Reference

#### Top-Level Fields

| Field               | Type    | Default           | Description                                                                                                                       |
| ------------------- | ------- | ----------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `provider`          | string  | `"deepseek"`      | LLM provider to use.                                                                                                              |
| `model`             | string  | provider-specific | Model identifier. See [PROVIDERS.md](PROVIDERS.md) for defaults.                                                                  |
| `temperature`       | float   | `0.1`             | Sampling temperature (0.0 = deterministic, 2.0 = very random).                                                                    |
| `max_tokens`        | integer | `4096`            | Maximum tokens in the LLM response. Defaults to 4096 to prevent the verdict block from being truncated by the provider.           |
| `chunk_head_lines`  | integer | `400`             | Lines preserved from the **start** of the diff when chunking. Increase for providers with large context windows (e.g. 128K).      |
| `chunk_tail_lines`  | integer | `400`             | Lines preserved from the **end** of the diff when chunking. Combined default of 800 covers most PRs without truncation.           |
| `cache_dir`         | string  | `.rs-guard/cache` | Custom cache directory path. Defaults to git-root (or CWD) relative `.rs-guard/cache`.                                           |

#### Provider Section Fields

| Field          | Type   | Required | Description                                                                     |
| -------------- | ------ | -------- | ------------------------------------------------------------------------------- |
| `api_key_env`  | string | no       | Environment variable name for the API key. Defaults to provider-standard names. |
| `base_url`     | string | no       | Custom API base URL. Defaults to provider's official endpoint.                  |
| `http_referer` | string | no       | Attribution referer (OpenRouter only).                                          |

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

## CLI Flags

| Flag            | Short | Default                    | Description                          |
| --------------- | ----- | -------------------------- | ------------------------------------ |
| `--prompt-file` | `-p`  | `.github/review-prompt.md` | Path to system prompt markdown file. |
| `--model`       | `-m`  | provider-specific          | LLM model identifier.                |
| `--temperature` | `-t`  | `0.1`                      | Sampling temperature (0.0 - 2.0).    |
| `--provider`    |       | `deepseek`                 | LLM provider to use.                 |
| `--config`      | `-c`  | `.reviewer.toml`           | Path to configuration TOML file.     |
| `--max-tokens`  |       | none                       | Maximum tokens for LLM completions.  |
| `--help`        | `-h`  |                            | Display help.                        |
| `--version`     | `-V`  |                            | Display version.                     |

---

## Environment Variables

| Variable                | Required By         | Description                              |
| ----------------------- | ------------------- | ---------------------------------------- |
| `DEEPSEEK_API_KEY`      | DeepSeek provider   | API key from DeepSeek platform.          |
| `KIMI_API_KEY`          | Kimi provider       | API key from Moonshot AI platform.       |
| `DASHSCOPE_API_KEY`     | Qwen provider       | API key from Alibaba Cloud DashScope.    |
| `OPENROUTER_API_KEY`    | OpenRouter provider | API key from OpenRouter.                 |
| `OPENAI_API_KEY`        | OpenAI provider     | API key from OpenAI.                     |
| `GITHUB_TOKEN`          | GitHub mode         | Auto-provided by GitHub Actions.         |
| `PR_NUMBER`             | GitHub mode         | Pull request number.                     |
| `REPO_FULL_NAME`        | GitHub mode         | Repository in `owner/repo` format.       |
| `GITHUB_ACTIONS`        | Auto-detected       | Presence indicates CI mode.              |
| `RS_GUARD_PROVIDER`     | Optional            | Override TOML/default provider.          |
| `RS_GUARD_MODEL`        | Optional            | Override TOML/default model.             |
| `RS_GUARD_TEMPERATURE`  | Optional            | Override TOML/default temperature.       |
| `RS_GUARD_MAX_TOKENS`   | Optional            | Override TOML/default max tokens.        |
| `GITHUB_API_URL`        | Optional            | Custom GitHub API base URL (Enterprise). |
| `RS_GUARD_DIFF_FILE`    | Optional            | Path to a pre-existing diff file.        |
| `RS_GUARD_METRICS_PATH` | Optional            | Path for the metrics JSON artifact.      |

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
