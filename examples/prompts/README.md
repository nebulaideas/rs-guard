# Review Prompt Templates

This directory contains language-agnostic review prompt templates for rs-guard.
Each template implements the five-axis review format used by the built-in default prompt,
and includes a `## Project-Specific Focus` section for customization.

## Available Prompts

- **[general-code-review.md](general-code-review.md)** — Canonical agnostic baseline
  - Mirrors the rs-guard built-in `DEFAULT_PROMPT` exactly
  - Suitable for any language or framework
  - Start here if you are unsure which template to use

- **[backend-api.md](backend-api.md)** — Backend services and APIs
  - Database safety, migrations, transactions, background jobs
  - API contracts, idempotency, HTTP semantics
  - Suitable for REST/GraphQL/gRPC services in any language

- **[frontend-spa.md](frontend-spa.md)** — Frontend single-page applications
  - Reactivity, stale closures, component lifecycle, bundle size
  - Client-side security (XSS, token storage), accessibility basics
  - Suitable for React, Vue, Svelte, Angular, or any SPA framework

- **[cli-tooling.md](cli-tooling.md)** — CLI tools and systems programs
  - Panics, unwrap discipline, resource cleanup, signal handling
  - CLI UX consistency, destructive-operation guards
  - Suitable for Rust, Go, C/C++, Python CLI tools, and daemons

## Usage

Copy the appropriate template to your repository:

```bash
# Ensure the .github directory exists first
mkdir -p .github

# General-purpose (any language/framework)
cp examples/prompts/general-code-review.md .github/review-prompt.md

# Backend / API service
cp examples/prompts/backend-api.md .github/review-prompt.md

# Frontend SPA
cp examples/prompts/frontend-spa.md .github/review-prompt.md

# CLI tool or system program
cp examples/prompts/cli-tooling.md .github/review-prompt.md
```

Then open the copied file and fill in the `## Project-Specific Focus` section with your
project's conventions, coding standards, and framework-specific rules.

## Customizing Prompts

Each template ends with a `## Project-Specific Focus` section containing commented examples.
Uncomment and adapt the examples to add:

- Project-specific conventions (naming, error handling patterns)
- Required tooling (ORM, test framework, linter rules)
- Architecture constraints (module boundaries, banned patterns)
- Team-specific quality gates (doc coverage, migration rules)

## Integration

The prompt is used by rs-guard when:

- Running in CI mode (GitHub Actions) with `--prompt-file .github/review-prompt.md`
- Running in local mode with a pre-commit hook passing `--prompt-file`
- Running manually: `rs-guard --prompt-file .github/review-prompt.md`

If the referenced file does not exist, rs-guard falls back to its built-in default prompt.

For pre-commit hook setup with Husky or Lefthook, see [`../local-review/husky-setup.md`](../local-review/husky-setup.md).
