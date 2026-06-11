# GitHub Actions Workflow Examples

This directory contains ready-to-use GitHub Actions workflows for integrating **rs-guard** into your CI pipeline.

## Available Workflows

| Workflow | File | Use Case |
|----------|------|----------|
| Basic AI Review | [`ai-review.yml`](ai-review.yml) | General-purpose review for any repository |
| Backend / API | [`backend-api.yml`](backend-api.yml) | Backend services and APIs with path filtering |
| Frontend / SPA | [`frontend-spa.yml`](frontend-spa.yml) | Frontend SPAs and component libraries with path filtering |
| Fork-Safe | [`fork-safe.yml`](fork-safe.yml) | Public repos that receive fork PRs |

## Common Features

All workflows include:

- **Concurrency groups** — Only one review runs per PR at a time. New pushes cancel in-progress reviews.
- **Draft PR skipping** — Reviews are skipped while a PR is in draft state.
- **Minimal permissions** — Uses `contents: read` and `pull-requests: write` only.
- **SHA-256 integrity check** — The downloaded binary is verified against the release's `*.sha256` file when available; otherwise a warning is emitted.

## Quick Start

1. Copy the desired workflow file into `.github/workflows/` in your repository.
2. Ensure your repository has the required secrets configured (`DEEPSEEK_API_KEY`, `GITHUB_TOKEN`).
3. Customize the provider or model by editing the `env` section or adding CLI flags.

## Custom Prompts

The `backend-api.yml` and `frontend-spa.yml` workflows reference a custom prompt file:

```bash
./rs-guard --prompt-file .github/review-prompt.md
```

Create this file in your repository to tailor the review focus to your stack.
Start from one of the templates in [`../prompts/`](../prompts/):

```bash
# Backend / API service
cp examples/prompts/backend-api.md .github/review-prompt.md

# Frontend SPA
cp examples/prompts/frontend-spa.md .github/review-prompt.md
```

Then fill in the `## Project-Specific Focus` section with your conventions.
The included [`review-prompt.md`](review-prompt.md) is a Rust-backend example.
If the referenced prompt file is missing, `rs-guard` falls back to its built-in
default prompt — the workflow will not fail.

## Security

The example workflows use the following security defaults:

- **Minimal permissions** — `contents: read`, `pull-requests: write`. Never use
  the default `write-all` token scope.
- **Concurrency groups** — Prevents stale reviews and resource waste.
- **SHA-256 verification** — The `Download rs-guard` step downloads the
  release binary with its original filename (e.g., `rs-guard-x86_64-unknown-linux-gnu`),
  fetches the corresponding `*.sha256` file (if published), and verifies the binary.
  The downloaded file is then renamed to `rs-guard` for subsequent steps. If the
  checksums file is missing, a warning is emitted and the workflow continues —
  tighten this in production by failing the build when no `*.sha256` is
  available, or by switching to a pinned release tag with a hard-coded hash.
- **Pinned release tag** — For production use, replace
  `releases/latest/download/...` with a specific tag (e.g. `releases/v0.7.0/...`)
  so you have a reproducible build.

> ⚠️ The `pull_request_target` event in `fork-safe.yml` runs in the **base
> branch** context and has access to repository secrets. The example workflow
> checks out the base SHA (not the PR head) and only reads the diff via the
> GitHub API — rs-guard does not build or execute the PR code. See the
> security warning at the top of `fork-safe.yml`.

## Fork Safety: Choosing the Right Workflow

Public repositories that accept pull requests from forks face a trade-off:

- **`pull_request`** workflows (used by `ai-review.yml`, `backend-api.yml`,
  `frontend-spa.yml`) **cannot access secrets** because they run in the
  untrusted fork context. Without secrets, you cannot call the LLM API.
  This is the safest option if you can route LLM calls through a
  comment-triggered bot or a webhook.

- **`pull_request_target`** workflows (used by `fork-safe.yml`) **can
  access secrets** because they run in the trusted base context — but they
  are also a known attack vector if the workflow executes untrusted code.

  The shipped `fork-safe.yml` adds the `if: … head.repo.full_name == github.repository`
  guard, which **only runs the workflow for non-fork PRs**. This is
  intentionally conservative. If you need to review fork PRs, consider one
  of these patterns:

  1. **Trusted-authors allowlist** — Replace the `if:` condition with a
     check against a hardcoded list of trusted GitHub usernames or org-team
     membership.
  2. **Comment-triggered `workflow_run`** — Use a `pull_request` workflow
     to post a comment, and a separate `workflow_run` workflow (with
     secrets) to react to that comment. This is the safest pattern and
     the one GitHub recommends for fork PRs.
  3. **Read-only rs-guard** — rs-guard only reads the diff and never
     executes the PR code, so the attack surface is small. For many
     organizations, the `pull_request_target` workflow is acceptable as-is
     after removing the `if:` guard. Audit your threat model before doing
     this.

## Event Trigger Behaviour

The `pull_request` workflows listen for `opened`, `synchronize`, and
`reopened` events. Note that:

- A `reopened` event is also delivered as a `synchronize` event, so the
  review may run twice on a single reopen. The `concurrency` group cancels
  the in-flight review, so the second run is usually no-op.
- Forks cannot trigger `pull_request` workflows with secrets, so fork PRs
  are skipped automatically by GitHub.

If you only want to review on the first push, use `types: [opened]`. If you
only want to review on subsequent commits, use `types: [synchronize]`.
