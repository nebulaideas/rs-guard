# GitHub Actions Workflow Examples

This directory contains ready-to-use GitHub Actions workflows for integrating **rs-guard** into your CI pipeline.

## Available Workflows

| Workflow | File | Use Case |
|----------|------|----------|
| Basic AI Review | [`ai-review.yml`](ai-review.yml) | General-purpose review for any repository |
| Rails | [`rails.yml`](rails.yml) | Ruby on Rails projects with path filtering |
| React + Vite | [`react-vite.yml`](react-vite.yml) | Frontend projects with path filtering |
| Fork-Safe | [`fork-safe.yml`](fork-safe.yml) | Public repos that receive fork PRs |

## Common Features

All workflows include:

- **Concurrency groups** — Only one review runs per PR at a time. New pushes cancel in-progress reviews.
- **Draft PR skipping** — Reviews are skipped while a PR is in draft state.
- **Minimal permissions** — Uses `contents: read` and `pull-requests: write` only.
- **Latest release download** — Fetches the newest `rs-guard` binary automatically.

## Quick Start

1. Copy the desired workflow file into `.github/workflows/` in your repository.
2. Ensure your repository has the required secrets configured (`DEEPSEEK_API_KEY`, `GITHUB_TOKEN`).
3. Customize the provider or model by editing the `env` section or adding CLI flags.

## Custom Prompts

Framework-specific workflows (Rails, React/Vite) reference a custom prompt file:

```bash
./rs-guard --prompt-file .github/rails-review-prompt.md
```

Create this file in your repository to tailor the review focus to your stack.

## Fork Safety

Public repositories that accept pull requests from forks should use [`fork-safe.yml`](fork-safe.yml). See the security comments at the top of that file for important warnings about `pull_request_target`.
