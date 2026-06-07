# diffguard-rs

AI-powered code review CLI for GitHub Pull Requests. Multi-provider LLM support, GitHub Actions integration, and local pre-commit execution — all in a single Rust binary.

[![CI](https://github.com/nebulaideas/diffguard-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/nebulaideas/diffguard-rs/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

---

## Features

- **Multi-provider LLM support** — DeepSeek, Kimi (Moonshot AI), Qwen (Alibaba Cloud), OpenRouter, OpenAI
- **In-memory verdict parsing** — Structured metadata block extraction, no intermediate comments
- **GitHub Actions + local pre-commit** — CI mode submits reviews; local mode blocks commits on issues
- **Configurable prompts** — Per-repository prompt files (`.github/review-prompt.md`)
- **Single binary** — Fast execution, no runtime dependencies
- **Permission fallback** — Downgrades to `COMMENT` when `APPROVE`/`REQUEST_CHANGES` is not permitted
- **TOML configuration** — `.reviewer.toml` for team-wide defaults

---

## Quick Start

### 1. Download the binary

```bash
curl -L -o diffguard \
  https://github.com/nebulaideas/diffguard-rs/releases/latest/download/diffguard
chmod +x diffguard
```

### 2. Set your API key

```bash
export DEEPSEEK_API_KEY="your-api-key"
```

### 3. Run locally

```bash
# Review staged changes
diffguard

# Use a different provider
export KIMI_API_KEY="your-api-key"
diffguard --provider kimi
```

### 4. Add to GitHub Actions

See [`examples/github-actions-workflow/`](examples/github-actions-workflow/) for sample workflows.

---

## Installation

### Pre-built binary

Download from [GitHub Releases](https://github.com/nebulaideas/diffguard-rs/releases).

### Build from source

```bash
git clone https://github.com/nebulaideas/diffguard-rs.git
cd diffguard-rs
cargo build --release
```

Requires Rust 1.82+.

---

## Usage

### CLI

```bash
diffguard [OPTIONS]

Options:
  -p, --prompt-file <PATH>    Path to system prompt markdown file [default: .github/review-prompt.md]
  -m, --model <MODEL>         LLM model identifier (default: provider-specific)
  -t, --temperature <TEMP>    Sampling temperature (0.0 - 2.0) [default: 0.1]
      --provider <PROVIDER>   LLM provider to use [default: deepseek]
  -c, --config <PATH>         Path to configuration TOML file [default: .reviewer.toml]
      --max-tokens <N>        Maximum tokens for LLM completions
  -h, --help                  Print help
  -V, --version               Print version
```

### Environment Variables

| Variable | Required | Description |
|---|---|---|
| `DEEPSEEK_API_KEY` | For DeepSeek | API key from [DeepSeek](https://platform.deepseek.com) |
| `KIMI_API_KEY` | For Kimi | API key from [Moonshot AI](https://platform.moonshot.cn) |
| `DASHSCOPE_API_KEY` | For Qwen | API key from [Alibaba Cloud](https://dashscope.aliyun.com) |
| `OPENROUTER_API_KEY` | For OpenRouter | API key from [OpenRouter](https://openrouter.ai) |
| `OPENAI_API_KEY` | For OpenAI | API key from [OpenAI](https://platform.openai.com) |
| `GITHUB_TOKEN` | In CI mode | Auto-provided by GitHub Actions |
| `PR_NUMBER` | In CI mode | Pull request number |
| `REPO_FULL_NAME` | In CI mode | Repository in `owner/repo` format |

See [docs/PROVIDERS.md](docs/PROVIDERS.md) for per-provider setup details.

### Configuration File

Create `.reviewer.toml` in your repository root:

```toml
provider = "deepseek"
model = "deepseek-v4-flash"
temperature = 0.1

[providers.deepseek]
api_key_env = "DEEPSEEK_API_KEY"
base_url = "https://api.deepseek.com"
```

Resolution order: **CLI flags > Environment variables > TOML file > Defaults**

See [docs/CONFIGURATION.md](docs/CONFIGURATION.md) for the full reference.

---

## Local Mode (Pre-commit)

Local mode is auto-detected when `GITHUB_ACTIONS` is absent. It analyzes `git diff --cached` and prints a colored terminal summary.

### Setup as a pre-commit hook

```bash
cp examples/local-review/pre-commit-hook.sh .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

If the review returns `REQUEST_CHANGES`, the commit is aborted. Bypass with:

```bash
git commit --no-verify
```

See [docs/LOCAL_MODE.md](docs/LOCAL_MODE.md) for details.

---

## Review State Logic

```
if verdict == "NEGATIVE" || security_issues > 0 || critical_bugs > 2:
    state = REQUEST_CHANGES
else if critical_bugs == 0 && security_issues == 0:
    state = APPROVE
else:
    state = COMMENT
```

If `REQUEST_CHANGES` or `APPROVE` fails due to GitHub permissions, the system falls back to `COMMENT` with a `[Bot fallback from {state}]` prefix.

---

## Architecture

```
[Fetch PR Diff] → [Call LLM] → [Parse Response] → [Determine State] → [Submit Review]
```

All processing happens in-memory. No intermediate comments are posted. Dismisses previous diffguard `CHANGES_REQUESTED` reviews when a new non-blocking review is submitted.

---

## Exit Codes

| Code | Meaning |
|---|---|
| `0` | Review completed successfully |
| `1` | Error occurred (API failure, config error, etc.) |
| `2` | Local mode: review returned `REQUEST_CHANGES` (blocks commit) |

---

## Development

```bash
cargo build
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all
cargo deny check
```

---

## License

MIT License — see [LICENSE](LICENSE) for details.
