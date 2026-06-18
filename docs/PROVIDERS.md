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

Example:
```bash
rs-guard --provider deepseek --variant pro
# or
export RS_GUARD_VARIANT=flash
```
In TOML:
```toml
provider = "deepseek"
# variant = "pro"                # top-level
[providers.deepseek]
variant = "pro"
```

### CLI Usage

```bash
rs-guard --provider deepseek --model deepseek-v4-flash
# or use the higher-level variant (recommended when available):
rs-guard --provider deepseek --variant flash
```

### TOML Configuration

```toml
provider = "deepseek"
model = "deepseek-v4-flash"
# variant = "pro"                # top level (applies to selected provider)

[providers.deepseek]
api_key_env = "DEEPSEEK_API_KEY"
base_url = "https://api.deepseek.com"
# variant = "pro"                # per-provider override (highest TOML precedence)
```

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
```

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
