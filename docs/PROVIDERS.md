# LLM Provider Setup Guide

This document covers how to configure each supported LLM provider for rs-guard.

---

## Table of Contents

- [DeepSeek](#deepseek)
- [Kimi (Moonshot AI)](#kimi-moonshot-ai)
- [Qwen (Alibaba Cloud)](#qwen-alibaba-cloud)
- [OpenRouter](#openrouter)
- [OpenAI](#openai)
- [Grok (xAI)](#grok-xai)
- [GLM (Zhipu AI)](#glm-zhipu-ai)

---

**Model Variants (generic mechanism):** Several providers support provider-specific "variants" (e.g. DeepSeek `flash`/`pro`). Set via `--variant`, `RS_GUARD_VARIANT`, or `variant` in `.reviewer.toml` (top-level or per-provider). See the per-provider sections below and `docs/CONFIGURATION.md`. Unknown variants for a provider that declares them produce a clear error listing the supported ones. Providers without registered variants silently ignore the setting for now.

---

## DeepSeek



### Quick Start

```bash
export DEEPSEEK_API_KEY="your-api-key"
```

### Provider Details

| Key            | Value                       |
| -------------- | --------------------------- |
| Base URL       | `https://api.deepseek.com`  |
| Default Model  | `deepseek-v4-flash`         |
| Context Window | 64,000 tokens               |
| Auth Header    | `Bearer {DEEPSEEK_API_KEY}` |

### Variants

DeepSeek V4 exposes multiple models. Use the generic `variant` mechanism (CLI `--variant`, `RS_GUARD_VARIANT` env, or `variant` / `[providers.deepseek].variant` in `.reviewer.toml`) instead of hard-coding raw model IDs.

| Variant | Description                          | Effective Model      |
| ------- | ------------------------------------ | -------------------- |
| `flash` | Fast, cost-effective (default)       | `deepseek-v4-flash`  |
| `pro`   | Most capable for complex reasoning   | `deepseek-v4-pro`    |

### Using deepseek-v4-pro (recommended for complex reviews)

`deepseek-v4-pro` is a powerful reasoning model. Because it performs extensive chain-of-thought internally, it often returns `"content": null` (or empty) while populating `reasoning_content`. rs-guard automatically:

- Treats empty final content as a **retryable** error (up to 3 attempts with backoff).
- Skips caching the response until a successful verdict is parsed.
- Raises the `max_tokens` floor to **16,384** when you do not set an explicit value.
- Raises the LLM timeout floor to **180s** (from 120s) for `deepseek` / `kimi` when not explicitly set.

**Best practices for deepseek-v4-pro**
- Set `max_tokens` to at least 16,384 (or higher for very thorough reviews).
- Use a longer timeout (180–300s) because reasoning can take significant time.
- Prefer the `pro` **variant** over the raw model name — it is clearer and future-proof.
- In CI (GitHub Actions), always pin explicit values and give the step enough `timeout-minutes`.

#### Recommended GitHub Actions usage (the pattern that was flaky)

```yaml
- name: AI Code Review (deepseek-v4-pro)
  run: |
    ./rs-guard \
      --prompt-file .github/review-prompt.md \
      --provider deepseek \
      --variant pro \
      --max-tokens 16384 \
      --llm-timeout 240
  env:
    DEEPSEEK_API_KEY: ${{ secrets.DEEPSEEK_API_KEY }}
  # Give the step headroom — the model itself can be slow
  timeout-minutes: 10
```

Or with environment variables (cleaner in workflows):

```yaml
env:
  DEEPSEEK_API_KEY: ${{ secrets.DEEPSEEK_API_KEY }}
  RS_GUARD_PROVIDER: deepseek
  RS_GUARD_VARIANT: pro
  RS_GUARD_MAX_TOKENS: 16384
  RS_GUARD_LLM_TIMEOUT: 240
```

If you want the review posted without failing the build on `REQUEST_CHANGES`, use:

```yaml
continue-on-error: true
```

#### CLI + parameters

```bash
# Recommended way (variant + explicit settings)
rs-guard \
  --provider deepseek \
  --variant pro \
  --max-tokens 16384 \
  --llm-timeout 180

# Alternative: specify the model directly
rs-guard --provider deepseek --model deepseek-v4-pro --max-tokens 20000
```

#### Environment variables (parameters)

```bash
export DEEPSEEK_API_KEY="sk-..."
export RS_GUARD_PROVIDER="deepseek"
export RS_GUARD_VARIANT="pro"
export RS_GUARD_MAX_TOKENS="16384"
export RS_GUARD_LLM_TIMEOUT="180"

rs-guard
```

#### TOML configuration

**Minimal .reviewer.toml using the variant (recommended):**

```toml
provider = "deepseek"
variant = "pro"                 # resolves to deepseek-v4-pro

# Important for reasoning models
max_tokens = 16384
llm_timeout_secs = 180

[providers.deepseek]
# You can also put variant here for per-provider override
# variant = "pro"
```

**Full example with per-provider section:**

```toml
provider = "deepseek"
model = "deepseek-v4-pro"       # you can also use model directly

max_tokens = 16384
llm_timeout_secs = 180

[providers.deepseek]
api_key_env = "DEEPSEEK_API_KEY"
base_url = "https://api.deepseek.com"
# variant = "pro"               # per-provider variant (takes precedence over top-level)
```

**Precedence (highest to lowest):**
1. CLI flags (`--variant`, `--model`, `--max-tokens`, `--llm-timeout`)
2. Environment variables (`RS_GUARD_VARIANT`, `RS_GUARD_MODEL`, ...)
3. `[providers.deepseek]` section in TOML
4. Top-level keys in TOML (`variant = "pro"`, `max_tokens = ...`)
5. Built-in defaults (`deepseek-v4-flash`, 120s / auto-raised 180s timeout for deepseek, auto 16k `max_tokens`)

### API Key Acquisition

1. Visit [platform.deepseek.com](https://platform.deepseek.com)
2. Create an account and navigate to **API Keys**
3. Generate a new key and copy it

---

## Kimi (Moonshot AI)



### Quick Start

```bash
export KIMI_API_KEY="your-api-key"
```

### Provider Details

| Key             | Value                             |
| --------------- | --------------------------------- |
| Base URL        | `https://api.moonshot.ai/v1`      |
| Default Model   | `kimi-k2.5`                       |
| Context Window  | 128,000 tokens                    |
| Auth Header     | `Bearer {KIMI_API_KEY}`           |
| Special Feature | `reasoning_content` field support (response); thinking mode via `variant` (request) |

### Variants

Kimi supports a thinking mode toggle via the generic variant mechanism.

| Variant       | Description                                                                 | Injected Request Field          |
|---------------|-----------------------------------------------------------------------------|---------------------------------|
| `thinking-on` | Enable Kimi thinking / chain-of-thought mode. The response may contain a `reasoning_content` field (rs-guard parses the final content and discards the reasoning). | `thinking: { "type": "enabled" }` |
| `thinking-off`| Explicitly disable thinking mode.                                           | `thinking: { "type": "disabled" }` |

Example:
```bash
rs-guard --provider kimi --variant thinking-on
# or
export RS_GUARD_VARIANT=thinking-on
```
In TOML:
```toml
provider = "kimi"
# variant = "thinking-on"          # top-level
[providers.kimi]
variant = "thinking-on"
```

### CLI Usage

```bash
rs-guard --provider kimi --model kimi-k2.5
# or use a thinking mode variant:
rs-guard --provider kimi --variant thinking-on
```

### TOML Configuration

```toml
provider = "kimi"
model = "kimi-k2.5"

[providers.kimi]
api_key_env = "KIMI_API_KEY"
base_url = "https://api.moonshot.ai/v1"
# variant = "thinking-on"
```

### API Key Acquisition

1. Visit [platform.moonshot.cn](https://platform.moonshot.cn) (or the international equivalent)
2. Sign up and go to **API Keys**
3. Create a new key

---

## Qwen (Alibaba Cloud)



### Quick Start

```bash
export DASHSCOPE_API_KEY="your-api-key"
```

### Provider Details

| Key             | Value                                                    |
| --------------- | -------------------------------------------------------- |
| Base URL        | `https://dashscope-intl.aliyuncs.com/compatible-mode/v1` |
| Default Model   | `qwen-plus`                                              |
| Context Window  | 128,000 tokens                                           |
| Auth Header     | `Bearer {DASHSCOPE_API_KEY}`                             |
| Special Feature | Requires `result_format: "message"` in requests          |

### CLI Usage

```bash
rs-guard --provider qwen --model qwen-plus
```

### TOML Configuration

```toml
provider = "qwen"
model = "qwen-plus"

[providers.qwen]
api_key_env = "DASHSCOPE_API_KEY"
base_url = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
```

### API Key Acquisition

1. Visit [dashscope.aliyun.com](https://dashscope.aliyun.com)
2. Create an Alibaba Cloud account
3. Navigate to **DashScope Console** → **API Keys**

---

## OpenRouter



### Quick Start

```bash
export OPENROUTER_API_KEY="your-api-key"
```

### Provider Details

| Key           | Value                          |
| ------------- | ------------------------------ |
| Base URL      | `https://openrouter.ai/api/v1` |
| Default Model | `openai/gpt-4o-mini`           |
| Context Window| 128,000 tokens                 |
| Auth Header   | `Bearer {OPENROUTER_API_KEY}`  |
| Extra Headers | `HTTP-Referer`, `X-Title`      |

### CLI Usage

```bash
# Route to any model via OpenRouter
rs-guard --provider openrouter --model anthropic/claude-3.5-sonnet
```

### TOML Configuration

```toml
provider = "openrouter"
model = "openai/gpt-4o-mini"

[providers.openrouter]
api_key_env = "OPENROUTER_API_KEY"
base_url = "https://openrouter.ai/api/v1"
http_referer = "https://github.com/YOUR_ORG/rs-guard"
```

### API Key Acquisition

1. Visit [openrouter.ai](https://openrouter.ai)
2. Sign up and go to **Keys**
3. Generate an API key

### Attribution Headers

OpenRouter requires `HTTP-Referer` and `X-Title` headers for attribution and rate-limit tracking. rs-guard sends these automatically:

- `HTTP-Referer`: `https://github.com/nebulaideas/rs-guard` (default)
- `X-Title`: `rs-guard`

Override via `.reviewer.toml`:

```toml
[providers.openrouter]
http_referer = "https://your-site.com"
```

---

## OpenAI



### Quick Start

```bash
export OPENAI_API_KEY="your-api-key"
```

### Provider Details

| Key           | Value                                                  |
| ------------- | ------------------------------------------------------ |
| Base URL      | `https://api.openai.com/v1`                            |
| Default Model | `gpt-4o-mini`                                          |
| Context Window| 128,000 tokens                                         |
| Auth Header   | `Bearer {OPENAI_API_KEY}`                              |
| Note          | Generic OpenAI-compatible; works with custom endpoints |

### CLI Usage

```bash
# Standard OpenAI
rs-guard --provider openai --model gpt-4o

# Custom OpenAI-compatible endpoint
rs-guard --provider openai --model custom-model
```

### TOML Configuration

```toml
provider = "openai"
model = "gpt-4o-mini"

[providers.openai]
api_key_env = "OPENAI_API_KEY"
base_url = "https://api.openai.com/v1"
```

### Custom Endpoint Example

For OpenAI-compatible proxies or local servers (e.g., Ollama, LM Studio):

```toml
provider = "openai"
model = "llama3.1"

[providers.openai]
api_key_env = "OPENAI_API_KEY"
base_url = "http://localhost:11434/v1"
# result_format = "json_object"  # Only if the endpoint requires it
```

If your custom endpoint requires a specific `result_format` (for example,
`"json_object"`), set it per-provider in `.reviewer.toml`. rs-guard will send
that value in the request body instead of the provider's static default.

### API Key Acquisition

1. Visit [platform.openai.com](https://platform.openai.com)
2. Go to **API Keys** in your account settings
3. Create a new secret key

---

## Grok (xAI)

### Quick Start

```bash
export XAI_API_KEY="your-api-key"
```

### Provider Details

| Key | Value |
| Base URL | `https://api.x.ai/v1` |
| Default Model | `grok-3` |
| Context Window | 128,000 tokens |
| Auth Header | `Bearer {XAI_API_KEY}` |
| Note | OpenAI-compatible endpoint |

### CLI Usage

```bash
rs-guard --provider grok --model grok-3
```

### TOML Configuration

```toml
provider = "grok"
model = "grok-3"

[providers.grok]
api_key_env = "XAI_API_KEY"
base_url = "https://api.x.ai/v1"
```

### API Key Acquisition

1. Visit [console.x.ai](https://console.x.ai)
2. Sign in with your xAI account
3. Navigate to **API Keys** and create a new key

### Provider Divergence

rs-guard uses the standard non-streaming `/chat/completions` endpoint. Advanced xAI-specific features (tool calling, function calling, streaming responses, web search integration) are not supported. If you need these features, consider using the xAI SDK directly.

---

## GLM (Zhipu AI)

### Quick Start

```bash
export ZHIPUAI_API_KEY="your-api-key"
```

### Provider Details

| Key | Value |
| Base URL | `https://open.bigmodel.cn/api/paas/v4` |
| Default Model | `glm-4` |
| Context Window | 128,000 tokens |
| Auth Header | `Bearer {ZHIPUAI_API_KEY}` |
| Note | OpenAI-compatible endpoint (Zhipu/z.ai GLM-4) |

### CLI Usage

```bash
rs-guard --provider glm --model glm-4
```

### TOML Configuration

```toml
provider = "glm"
model = "glm-4"

[providers.glm]
api_key_env = "ZHIPUAI_API_KEY"
base_url = "https://open.bigmodel.cn/api/paas/v4"
```

### API Key Acquisition

1. Visit [open.bigmodel.cn](https://open.bigmodel.cn)
2. Sign up for a Zhipu AI account
3. Navigate to **API Keys** and create a new key

### Provider Divergence

rs-guard uses the standard non-streaming `/chat/completions` endpoint. Advanced Zhipu-specific features (tool calling, function calling, streaming responses, plugin system) are not supported. If you need these features, consider using the Zhipu SDK directly.

---

## Environment Variables Reference

| Variable | Provider | Required When |
| `DEEPSEEK_API_KEY` | DeepSeek | `--provider deepseek` (default) |
| `KIMI_API_KEY` | Kimi | `--provider kimi` |
| `DASHSCOPE_API_KEY` | Qwen | `--provider qwen` |
| `OPENROUTER_API_KEY` | OpenRouter | `--provider openrouter` |
| `OPENAI_API_KEY` | OpenAI | `--provider openai` |
| `XAI_API_KEY` | Grok | `--provider grok` |
| `ZHIPUAI_API_KEY` | GLM | `--provider glm` |
