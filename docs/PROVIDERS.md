# LLM Provider Setup Guide

This document covers how to configure each supported LLM provider for rs-guard.

---

## Table of Contents

- [Architecture: Hybrid Client Design](#architecture-hybrid-client-design)
- [DeepSeek](#deepseek)
- [Kimi (Moonshot AI)](#kimi-moonshot-ai)
- [Qwen (Alibaba Cloud)](#qwen-alibaba-cloud)
- [OpenRouter](#openrouter)
- [OpenAI](#openai)
- [Grok (xAI)](#grok-xai)
- [GLM (Zhipu AI)](#glm-zhipu-ai)
- [Ollama (Local)](#ollama-local)
- [Gemini (Google)](#gemini-google)

---

**Model Variants (generic mechanism):** Several providers support provider-specific "variants" (e.g. DeepSeek `flash`/`pro`). Set via `--variant`, `RS_GUARD_VARIANT`, or `variant` in `.reviewer.toml` (top-level or per-provider). See the per-provider sections below and `docs/CONFIGURATION.md`. Unknown variants for a provider that declares them produce a clear error listing the supported ones. Providers without registered variants silently ignore the setting for now.

---

## Architecture: Hybrid Client Design

As of v1.8, rs-guard uses two client implementations for its 9 providers:

| Client | Providers | Backing |
|--------|-----------|---------|
| `KernelBackedClient` | openai, grok, glm, ollama, gemini, openrouter (6) | `llm_kernel::llm::OpenAIClient` from the [llm-kernel](https://crates.io/crates/llm-kernel) crate |
| `GenericOpenAiCompatibleClient` | deepseek, qwen, kimi (3) | rs-guard's own data-driven client parameterized by `ProviderMeta` |

### Why two clients?

`llm_kernel::LLMRequest` has a fixed request body shape that cannot express:
- Qwen's `result_format` field
- Kimi's `extra_body` (thinking mode variants)

DeepSeek also uses `GenericOpenAiCompatibleClient` because llm-kernel 0.28.1
cannot deserialize V4 thinking responses with `"tool_calls": null` or
multimodal `content` arrays. Restore the kernel path after llm-kernel accepts
null `tool_calls`.

Providers that need custom request fields (or DeepSeek's loose JSON parsing)
use `GenericOpenAiCompatibleClient`. All others use `KernelBackedClient`, which
provides:
- **Reasoning-content promotion** (GLM-4.7)
- **Standardized error handling** with HTTP status preservation
- **Future compatibility** with llm-kernel's decorator stack (RetryClient, CacheClient, RouterClient)

DeepSeek's generic path does not promote `reasoning_content` into the verdict.
Empty assistant `content` with `reasoning_content` present is treated as a
retryable empty-content error (with `max_tokens` escalation), same as before.

### Factory routing

The factory (`create_provider`) matches on `ProviderMeta.strategy`
(`ClientStrategy::Kernel` or `ClientStrategy::Generic`). Config-level
`result_format` and metadata overrides (`force_generic_client`, ExtraBody
variants) still force `GenericOpenAiCompatibleClient`. Both clients implement
the `LlmProvider` trait, so the pipeline is agnostic to the underlying client.

### Shared TLS stack

rs-guard and llm-kernel share the same reqwest 0.13 stack (no duplicate TLS
dependencies). The TLS provider is `aws-lc-rs` (via rustls 0.23's default).
An upstream issue has been filed to evaluate switching to `ring` for simpler
cross-compilation ([epicsagas/llm-kernel#93](https://github.com/epicsagas/llm-kernel/issues/93)).

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
| Client         | `GenericOpenAiCompatibleClient` (until llm-kernel accepts null `tool_calls`) |

### Variants

DeepSeek V4 exposes multiple models. Use the generic `variant` mechanism (CLI `--variant`, `RS_GUARD_VARIANT` env, or `variant` / `[providers.deepseek].variant` in `.reviewer.toml`) instead of hard-coding raw model IDs.

| Variant | Description                          | Effective Model      |
| ------- | ------------------------------------ | -------------------- |
| `flash` | Fast, cost-effective (default)       | `deepseek-v4-flash`  |
| `pro`   | Most capable for complex reasoning   | `deepseek-v4-pro`    |

### Using deepseek-v4-pro (recommended for complex reviews)

`deepseek-v4-pro` is a powerful reasoning model. Because it performs extensive chain-of-thought internally, it often returns `"content": null` (or empty) while populating `reasoning_content`. rs-guard automatically:

- **Escalates `max_tokens`** when reasoning exhausts the output budget: the request is re-sent with a doubled limit (16,384 → 32,768 → 65,536 cap) instead of blindly retrying the identical request.
- Treats empty final content **without** reasoning as a transient error (up to 3 attempts with backoff).
- Skips caching the response until a successful verdict is parsed.
- Raises the `max_tokens` floor to **16,384** when you do not set an explicit value.
- Raises the LLM timeout floor to **240s** (from 120s) for `deepseek` / `kimi` when not explicitly set.

**Best practices for deepseek-v4-pro**
- Set `max_tokens` to at least 16,384 (or higher for very thorough reviews).
- Use a longer timeout (240–300s) because reasoning can take significant time.
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
  timeout-minutes: 15
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
5. Built-in defaults (`deepseek-v4-flash`, 120s / auto-raised 240s timeout for deepseek, auto 16k `max_tokens`)

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

| Variant       | Description                                                                 | Injected Request Field          | Temperature Override |
|---------------|-----------------------------------------------------------------------------|---------------------------------|----------------------|
| `thinking-on` | Enable Kimi thinking / chain-of-thought mode. The response may contain a `reasoning_content` field (rs-guard parses the final content and discards the reasoning). | `thinking: { "type": "enabled" }` | `1.0` |
| `thinking-off`| Explicitly disable thinking mode.                                           | `thinking: { "type": "disabled" }` | `0.6` |

> **Temperature constraints:** Kimi k2.5 only accepts specific temperature values
> depending on the thinking mode. The variant's `temperature_override` field
> automatically replaces the configured temperature — you do not need to set
> `--temperature` manually when using a variant. If you set `--temperature`
> explicitly, it will be overridden by the variant's required value.

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
# result_format = "message"  # Optional; rs-guard sends "message" by default for Qwen
```

To override the static default (for example on a custom compatible endpoint), set
`result_format` under `[providers.<name>]` in `.reviewer.toml`. Blank values are
ignored so the provider's built-in default still applies.

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

## Ollama (Local)

### Quick Start

```bash
# Install Ollama: https://ollama.com
ollama pull llama3.2
# No API key required — Ollama runs locally without authentication.
```

### Provider Details

| Key | Value |
| Base URL | `http://127.0.0.1:11434/v1` |
| Default Model | `llama3.2` |
| Context Window | 32,000 tokens |
| Auth Header | None (no API key required) |
| CI Mode | Not supported — loopback rejected in CI mode |
| Note | Local-only; run in local mode (unset `GITHUB_ACTIONS`) |

### CLI Usage

```bash
rs-guard --provider ollama --model llama3.2
# Or use a different model:
rs-guard --provider ollama --model qwen2.5-coder:7b
```

### TOML Configuration

```toml
provider = "ollama"
model = "llama3.2"

[providers.ollama]
base_url = "http://127.0.0.1:11434/v1"
# api_key_env is optional — Ollama does not require authentication.
```

### Provider Divergence

Ollama provides an OpenAI-compatible `/chat/completions` endpoint. No API key is needed by default. If you enable Ollama's auth proxy, set `OLLAMA_API_KEY` and it will be sent as a Bearer token. Ollama is **local-mode only** — the loopback address is rejected in CI mode to prevent token exfiltration.

---

## Gemini (Google)

### Quick Start

```bash
export GEMINI_API_KEY="your-api-key"
```

### Provider Details

| Key | Value |
| Base URL | `https://generativelanguage.googleapis.com/v1beta/openai` |
| Default Model | `gemini-2.5-flash` |
| Context Window | 1,000,000 tokens |
| Auth Header | `Bearer {GEMINI_API_KEY}` |
| Variants | `flash` (gemini-2.5-flash), `pro` (gemini-2.5-pro) |
| Note | Google's OpenAI-compatible endpoint |

### CLI Usage

```bash
rs-guard --provider gemini
# Or use the pro variant for complex reasoning:
rs-guard --provider gemini --variant pro
```

### TOML Configuration

```toml
provider = "gemini"
model = "gemini-2.5-flash"

[providers.gemini]
api_key_env = "GEMINI_API_KEY"
base_url = "https://generativelanguage.googleapis.com/v1beta/openai"
```

### API Key Acquisition

1. Visit [Google AI Studio](https://aistudio.google.com/apikey)
2. Sign in with a Google account
3. Create a new API key

### Provider Divergence

rs-guard uses Google's OpenAI-compatible endpoint (`/v1beta/openai/chat/completions`), not the native Gemini API. This means standard OpenAI request/response shapes work out of the box. Gemini-specific features (multimodal input, grounding, code execution) are not supported via this endpoint.

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
| `OLLAMA_API_KEY` | Ollama | Optional (only if Ollama auth proxy is enabled) |
| `GEMINI_API_KEY` | Gemini | `--provider gemini` |
