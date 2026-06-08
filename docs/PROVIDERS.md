# LLM Provider Setup Guide

This document covers how to configure each supported LLM provider for rs-guard.

---

## Table of Contents

- [DeepSeek](#deepseek)
- [Kimi (Moonshot AI)](#kimi-moonshot-ai)
- [Qwen (Alibaba Cloud)](#qwen-alibaba-cloud)
- [OpenRouter](#openrouter)
- [OpenAI](#openai)

---

## DeepSeek

**Phase:** 1 (default provider)

### Quick Start

```bash
export DEEPSEEK_API_KEY="your-api-key"
```

### Provider Details

| Key | Value |
|---|---|
| Base URL | `https://api.deepseek.com` |
| Default Model | `deepseek-v4-flash` |
| Auth Header | `Bearer {DEEPSEEK_API_KEY}` |

### CLI Usage

```bash
rs-guard --provider deepseek --model deepseek-v4-flash
```

### TOML Configuration

```toml
provider = "deepseek"
model = "deepseek-v4-flash"

[providers.deepseek]
api_key_env = "DEEPSEEK_API_KEY"
base_url = "https://api.deepseek.com"
```

### API Key Acquisition

1. Visit [platform.deepseek.com](https://platform.deepseek.com)
2. Create an account and navigate to **API Keys**
3. Generate a new key and copy it

---

## Kimi (Moonshot AI)

**Phase:** 2

### Quick Start

```bash
export KIMI_API_KEY="your-api-key"
```

### Provider Details

| Key | Value |
|---|---|
| Base URL | `https://api.moonshot.ai/v1` |
| Default Model | `kimi-k2.5` |
| Auth Header | `Bearer {KIMI_API_KEY}` |
| Special Feature | `reasoning_content` field support |

### CLI Usage

```bash
rs-guard --provider kimi --model kimi-k2.5
```

### TOML Configuration

```toml
provider = "kimi"
model = "kimi-k2.5"

[providers.kimi]
api_key_env = "KIMI_API_KEY"
base_url = "https://api.moonshot.ai/v1"
```

### API Key Acquisition

1. Visit [platform.moonshot.cn](https://platform.moonshot.cn) (or the international equivalent)
2. Sign up and go to **API Keys**
3. Create a new key

---

## Qwen (Alibaba Cloud)

**Phase:** 2

### Quick Start

```bash
export DASHSCOPE_API_KEY="your-api-key"
```

### Provider Details

| Key | Value |
|---|---|
| Base URL | `https://dashscope-intl.aliyuncs.com/compatible-mode/v1` |
| Default Model | `qwen-plus` |
| Auth Header | `Bearer {DASHSCOPE_API_KEY}` |
| Special Feature | Requires `result_format: "message"` in requests |

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

**Phase:** 2

### Quick Start

```bash
export OPENROUTER_API_KEY="your-api-key"
```

### Provider Details

| Key | Value |
|---|---|
| Base URL | `https://openrouter.ai/api/v1` |
| Default Model | `openai/gpt-4o-mini` |
| Auth Header | `Bearer {OPENROUTER_API_KEY}` |
| Extra Headers | `HTTP-Referer`, `X-Title` |

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

**Phase:** 2

### Quick Start

```bash
export OPENAI_API_KEY="your-api-key"
```

### Provider Details

| Key | Value |
|---|---|
| Base URL | `https://api.openai.com/v1` |
| Default Model | `gpt-4o-mini` |
| Auth Header | `Bearer {OPENAI_API_KEY}` |
| Note | Generic OpenAI-compatible; works with custom endpoints |

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

## Environment Variables Reference

| Variable | Provider | Required When |
|---|---|---|
| `DEEPSEEK_API_KEY` | DeepSeek | `--provider deepseek` (default) |
| `KIMI_API_KEY` | Kimi | `--provider kimi` |
| `DASHSCOPE_API_KEY` | Qwen | `--provider qwen` |
| `OPENROUTER_API_KEY` | OpenRouter | `--provider openrouter` |
| `OPENAI_API_KEY` | OpenAI | `--provider openai` |
