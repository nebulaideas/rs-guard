# Configuration Reference

This document describes the `.reviewer.toml` configuration file and the configuration resolution order used by diffguard-rs.

---

## Configuration Resolution Order

diffguard-rs resolves configuration values in the following priority (highest to lowest):

```
CLI flags > Environment variables > TOML file > Hardcoded defaults
```

### Example

If your `.reviewer.toml` sets `provider = "kimi"`, but you run:

```bash
export DIFFGUARD_PROVIDER="openai"
diffguard --provider qwen
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
http_referer = "https://github.com/nebulaideas/diffguard-rs"

[providers.openai]
api_key_env = "OPENAI_API_KEY"
base_url = "https://api.openai.com/v1"
```

### Field Reference

#### Top-Level Fields

| Field | Type | Default | Description |
|---|---|---|---|
| `provider` | string | `"deepseek"` | LLM provider to use. |
| `model` | string | provider-specific | Model identifier. See [PROVIDERS.md](PROVIDERS.md) for defaults. |
| `temperature` | float | `0.1` | Sampling temperature (0.0 = deterministic, 2.0 = very random). |
| `max_tokens` | integer | none | Maximum tokens in the LLM response. |

#### Provider Section Fields

| Field | Type | Required | Description |
|---|---|---|---|
| `api_key_env` | string | no | Environment variable name for the API key. Defaults to provider-standard names. |
| `base_url` | string | no | Custom API base URL. Defaults to provider's official endpoint. |
| `http_referer` | string | no | Attribution referer (OpenRouter only). |

---

## CLI Flags

| Flag | Short | Default | Description |
|---|---|---|---|
| `--prompt-file` | `-p` | `.github/review-prompt.md` | Path to system prompt markdown file. |
| `--model` | `-m` | provider-specific | LLM model identifier. |
| `--temperature` | `-t` | `0.1` | Sampling temperature (0.0 - 2.0). |
| `--provider` | | `deepseek` | LLM provider to use. |
| `--config` | `-c` | `.reviewer.toml` | Path to configuration TOML file. |
| `--max-tokens` | | none | Maximum tokens for LLM completions. |
| `--help` | `-h` | | Display help. |
| `--version` | `-V` | | Display version. |

---

## Environment Variables

| Variable | Required By | Description |
|---|---|---|
| `DEEPSEEK_API_KEY` | DeepSeek provider | API key from DeepSeek platform. |
| `KIMI_API_KEY` | Kimi provider | API key from Moonshot AI platform. |
| `DASHSCOPE_API_KEY` | Qwen provider | API key from Alibaba Cloud DashScope. |
| `OPENROUTER_API_KEY` | OpenRouter provider | API key from OpenRouter. |
| `OPENAI_API_KEY` | OpenAI provider | API key from OpenAI. |
| `GITHUB_TOKEN` | GitHub mode | Auto-provided by GitHub Actions. |
| `PR_NUMBER` | GitHub mode | Pull request number. |
| `REPO_FULL_NAME` | GitHub mode | Repository in `owner/repo` format. |
| `GITHUB_ACTIONS` | Auto-detected | Presence indicates CI mode. |
| `DIFFGUARD_PROVIDER` | Optional | Override TOML/default provider. |
| `DIFFGUARD_MODEL` | Optional | Override TOML/default model. |
| `DIFFGUARD_TEMPERATURE` | Optional | Override TOML/default temperature. |
| `DIFFGUARD_MAX_TOKENS` | Optional | Override TOML/default max tokens. |
| `GITHUB_API_URL` | Optional | Custom GitHub API base URL (Enterprise). |

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

[providers.openrouter]
api_key_env = "OPENROUTER_API_KEY"
base_url = "https://openrouter.ai/api/v1"
http_referer = "https://github.com/my-org/my-repo"
```
