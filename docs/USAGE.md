# rs-guard — Usage Guide

Complete reference for running rs-guard in all modes.

---

## Table of Contents

- [CLI Reference](#cli-reference)
- [Environment Variables](#environment-variables)
- [Exit Codes](#exit-codes)
- [Review State Logic](#review-state-logic)
- [GitHub Actions Integration](#github-actions-integration)
- [Local Pre-commit Setup](#local-pre-commit-setup)
- [Configuration File](#configuration-file)
- [Customizing the Review Prompt](#customizing-the-review-prompt)
- [Troubleshooting](#troubleshooting)

---

## CLI Reference

```bash
rs-guard [OPTIONS]
```

### Options

| Flag            | Short | Default                    | Description                                                                        |
| --------------- | ----- | -------------------------- | ---------------------------------------------------------------------------------- |
| `--prompt-file` | `-p`  | `.github/review-prompt.md` | Path to the system prompt markdown file. Uses embedded default if not found.       |
| `--model`       | `-m`  | _(provider default)_       | LLM model identifier. Overrides TOML and provider defaults.                        |
| `--temperature` | `-t`  | `0.1`                      | Sampling temperature (0.0 to 2.0). Lower values produce more deterministic output. |
| `--provider`    |       | `deepseek`                 | LLM provider: `deepseek`, `kimi`, `qwen`, `openrouter`, `openai`.                  |
| `--config`      | `-c`  | `.reviewer.toml`           | Path to the configuration TOML file.                                               |
| `--max-tokens`  |       | `4096`                     | Maximum tokens for LLM completions.                                                |
| `--diff-file`   | —     | _(none)_                   | Review a pre-existing diff file instead of fetching from GitHub API.               |
| `--no-cache`    | —     | Off                        | Bypass the response cache and force a fresh LLM API call.                          |
| `--dry-run`     | —     | Off                        | Run the full pipeline without submitting reviews or blocking commits.              |
| `--help`        | `-h`  |                            | Display usage information and exit.                                                |
| `--version`     | `-V`  |                            | Display version and exit.                                                          |

### Mode Detection

rs-guard detects the execution mode:

- **CI mode:** `GITHUB_ACTIONS` env var is set. Fetches PR diff and submits GitHub review.
- **Local mode:** `GITHUB_ACTIONS` absent. Runs `git diff --cached`, prints colored summary, exits code `2` if `REQUEST_CHANGES`.
- **File mode:** `--diff-file` or `RS_GUARD_DIFF_FILE` set. Reads diff from file, prints colored summary.

### Examples

```bash
# CI mode reviews the PR from env vars
rs-guard --provider deepseek --model deepseek-v4-flash

# Local mode with Kimi
rs-guard --provider kimi --model kimi-k2.5

# Review a pre-existing diff file
rs-guard --diff-file pr-diff.diff

# Bypass cache and use custom prompt
rs-guard --no-cache --prompt-file .github/review-prompt.md

# Test configuration without submitting or blocking
rs-guard --dry-run
```

---

## Environment Variables

| Variable                | Required By   | Description                                                                             |
| ----------------------- | ------------- | --------------------------------------------------------------------------------------- |
| `DEEPSEEK_API_KEY`      | DeepSeek      | API key from [DeepSeek Platform](https://platform.deepseek.com)                         |
| `KIMI_API_KEY`          | Kimi          | API key from [Moonshot AI](https://platform.moonshot.cn)                                |
| `DASHSCOPE_API_KEY`     | Qwen          | API key from [Alibaba Cloud DashScope](https://dashscope.aliyun.com)                    |
| `OPENROUTER_API_KEY`    | OpenRouter    | API key from [OpenRouter](https://openrouter.ai)                                        |
| `OPENAI_API_KEY`        | OpenAI        | API key from [OpenAI Platform](https://platform.openai.com)                             |
| `GITHUB_TOKEN`          | CI mode       | Auto-provided by GitHub Actions; alternatively set to a PAT with `pull-requests: write` |
| `PR_NUMBER`             | CI mode       | Pull request number                                                                     |
| `REPO_FULL_NAME`        | CI mode       | Repository in `owner/repo` format                                                       |
| `GITHUB_ACTIONS`        | Auto-detected | Presence indicates CI mode                                                              |
| `RS_GUARD_PROVIDER`     | Optional      | Override default provider via environment variable                                      |
| `RS_GUARD_MODEL`        | Optional      | Override default model for the current provider                                         |
| `RS_GUARD_TEMPERATURE`  | Optional      | Override default temperature via environment variable                                   |
| `RS_GUARD_MAX_TOKENS`   | Optional      | Override max tokens via environment variable                                            |
| `RS_GUARD_DIFF_FILE`    | Optional      | Alias for `--diff-file`                                                                 |
| `RS_GUARD_METRICS_PATH` | Optional      | Custom path for `rs-guard-metrics.json` artifact                                        |
| `GITHUB_API_URL`        | Optional      | Custom GitHub API base URL (e.g. GitHub Enterprise); default: `https://api.github.com`  |

---

## Exit Codes

| Code | Meaning                            | When                                              |
| ---- | ---------------------------------- | ------------------------------------------------- |
| `0`  | Review completed successfully      | Any mode, any verdict                             |
| `1`  | Error occurred                     | API failure, config error, parse error, etc.      |
| `2`  | Local/file mode: `REQUEST_CHANGES` | Review returned `REQUEST_CHANGES`; commit blocked |

---

## Review State Logic

The internal review state is determined by the LLM verdict using an **asymmetric safety model**:

```bash
if verdict == "NEGATIVE" or security_issues > 0 or critical_bugs > 2:
    → REQUEST_CHANGES
else if critical_bugs == 0 and security_issues == 0:
    → APPROVE
else:
    → COMMENT
```

**Key principle:** Pessimistic signals are always trusted; optimistic signals require clean counts. A positive verdict with 1–2 critical bugs yields `COMMENT` (not auto-approve), giving a human a chance to decide.

### Permission Fallback

If `APPROVE` or `REQUEST_CHANGES` fails with HTTP 403 (insufficient token permissions), the state is automatically downgraded to `COMMENT` with a `[Bot fallback from {state}]` prefix. This ensures the review is recorded even with read-only tokens.

---

## GitHub Actions Integration

### Minimal Workflow

```yaml
name: AI Code Review
on:
  pull_request:
    types: [opened, synchronize]

permissions:
  pull-requests: write
  contents: read

jobs:
  review:
    runs-on: ubuntu-latest
    if: ${{ !github.event.pull_request.head.repo.fork }}
    steps:
      - uses: actions/checkout@v4

      - name: Download rs-guard
        run: |
          curl -L -o rs-guard \
            https://github.com/nebulaideas/rs-guard/releases/latest/download/rs-guard
          chmod +x rs-guard

      - name: AI Code Review
        run: ./rs-guard
        env:
          DEEPSEEK_API_KEY: ${{ secrets.DEEPSEEK_API_KEY }}
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          PR_NUMBER: ${{ github.event.pull_request.number }}
          REPO_FULL_NAME: ${{ github.repository }}
```

### With `.reviewer.toml`

```yaml
name: AI Code Review
on:
  pull_request:
    types: [opened, synchronize]

permissions:
  pull-requests: write
  contents: read

jobs:
  review:
    runs-on: ubuntu-latest
    if: ${{ !github.event.pull_request.head.repo.fork }}
    steps:
      - uses: actions/checkout@v4

      - name: Download rs-guard
        run: |
          curl -L -o rs-guard \
            https://github.com/nebulaideas/rs-guard/releases/latest/download/rs-guard
          chmod +x rs-guard

      - name: AI Code Review
        run: ./rs-guard --config .reviewer.toml
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          PR_NUMBER: ${{ github.event.pull_request.number }}
          REPO_FULL_NAME: ${{ github.repository }}

      - name: Upload review artifact
        uses: actions/upload-artifact@v4
        if: always()
        with:
          name: review-result
          path: |
            review-result.txt
            rs-guard-metrics.json
```

### Workflow Notes

- **Fork safety:** `if: !github.event.pull_request.head.repo.fork` prevents running on forks where secrets are not available.
- **Token scope:** `GITHUB_TOKEN` has `pull-requests: write` scope by default. Request explicitly if needed.
- **Artifacts:** `review-result.txt` and `rs-guard-metrics.json` are written by rs-guard and can be uploaded as workflow artifacts.

---

## Local Pre-commit Setup

### Option 1: Manual Hook Installation

```bash
cp examples/local-review/pre-commit-hook.sh .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
git add -A
git commit
```

### Option 2: Inline Hook Script

Create `.git/hooks/pre-commit`:

```bash
#!/bin/sh

# Skip if nothing is staged
if git diff --cached --quiet; then
  exit 0
fi

./rs-guard
EXIT_CODE=$?

if [ "$EXIT_CODE" -eq 2 ]; then
  echo "Commit blocked: rs-guard requested changes."
  echo "Skip this check with:"
  echo "  git commit --no-verify"
  exit 1
fi
exit 0
```

### Bypass on Demand

```bash
git commit -m "docs: fix typo" --no-verify
```

---

## Configuration File

Create `.reviewer.toml` in your repository root.

### Full Schema

```toml
# Global defaults
provider = "deepseek"
model = "deepseek-v4-flash"
temperature = 0.1
max_tokens = 8192

# Per-provider configuration
[providers.deepseek]
api_key_env = "DEEPSEEK_API_KEY"
base_url = "https://api.deepseek.com"

[providers.openrouter]
api_key_env = "OPENROUTER_API_KEY"
base_url = "https://openrouter.ai/api/v1"
http_referer = "https://github.com/your-org/your-repo"
```

### Configuration Resolution Order

| Priority | Source                | Example                                 |
| -------- | --------------------- | --------------------------------------- |
| 1        | CLI flags             | `--provider kimi`                       |
| 2        | Environment variables | `RS_GUARD_PROVIDER=kimi`                |
| 3        | TOML file             | `provider = "kimi"` in `.reviewer.toml` |
| 4        | Hardcoded defaults    | `provider = "deepseek"`                 |

### Per-Provider TOML Fields

| Field                           | Required | Description                                                                                                           |
| ------------------------------- | -------- | --------------------------------------------------------------------------------------------------------------------- |
| `providers.<name>.api_key_env`  | No       | Override env var name for API key. Defaults to standard mapping (e.g., `DEEPSEEK_API_KEY`).                           |
| `providers.<name>.base_url`     | No       | Override default base URL. In CI mode must be on allowlist. In local mode, warnings logged for non-standard/loopback. |
| `providers.<name>.http_referer` | No       | HTTP referer header (e.g. OpenRouter attribution).                                                                    |

### Provider Switching Behavior

When the provider changes via CLI or env var:

1. Resolves the API key from the appropriate env var (or TOML `api_key_env`).
2. Resets the model to the new provider default unless `--model` was passed.
3. Validates the provider URL against the allowlist (CI) or log warnings (local).

---

## Customizing the Review Prompt

rs-guard uses a system prompt sent alongside the diff to the LLM. The embedded default works out-of-the-box, but tailoring it to your project produces better, more relevant reviews. A well-crafted prompt reduces false positives, catches domain-specific bugs a generic reviewer would miss, and respects your team's conventions.

Create `.github/review-prompt.md` in your repository root (or pass `--prompt-file`). Start from the template that matches your stack below, then adjust the signal patterns to match your project's specific rules.

### Best Practices for LLM Code Review Prompts

1. **Anchor the role with stack expertise.** "You are a senior Rust engineer who maintains a `tokio`-based gRPC service" is far more effective than "you are a code reviewer."
2. **Define severity with falsifiable criteria.** "A Critical Bug means the code will panic at runtime or produce incorrect output under valid input" — not "bugs are bad."
3. **List concrete signal patterns.** The LLM needs specific code smells to pattern-match against. `?` operator without `.context()` is actionable; "check error handling" is not.
4. **Tell the model what NOT to flag.** Explicitly exclude style preferences, naming conventions, and formatting — the linter covers those. This keeps the review focused.
5. **Include anti-patterns from your tech debt log.** If your team bans `Arc<Mutex<T>>` in hot paths or `after_save` callbacks across bounded contexts, encode that in the prompt.
6. **Keep it under 1,000 words.** The prompt and diff share the model's context window. Every word in the prompt is a word the diff can't use.

---

### Template: Rust Backend (tokio / sqlx / actix-web / axum)

```markdown
# rs-guard Review Prompt — Rust Backend

## Role
You are a senior Rust systems engineer reviewing a Pull Request for a production
backend service. The codebase uses tokio for async I/O, thiserror/anyhow for error
handling, and sqlx for database access. You treat every PR as if it will deploy to
production immediately.

## Focus Areas (in priority order)

1. **Memory safety and ownership** — Borrow checker violations that might have been
   worked around with `.clone()` or `Arc` unnecessarily. Double-check `unsafe` blocks
   for missing `// SAFETY:` comments and unproven invariants. Verify `Pin` usage is
   correct when dealing with async streams and `Future` combinators.

2. **Async correctness** — Verify that `.await` is called inside a `tokio::spawn` or
   an async fn, not dropped silently. Check for missing `select!` cancel-safety:
   futures that are dropped mid-operation must handle cancellation correctly. Ensure
   `JoinHandle` results are not silently discarded.

3. **Error handling** — Every `?` propagation should have `.context()` or
   `.with_context()` when crossing module boundaries. `unwrap()` and `expect()` are
   only acceptable in test code (`#[cfg(test)]`) or one-time initialization. Catch
   `.unwrap_or_default()` on fallible operations that should propagate errors.

4. **Security** — No hardcoded credentials or tokens. No `env::var()` without proper
   validation. SQL queries built via format strings (sqlx provides compile-time
   checking, but raw `query()` / `query_as()` should be preferred). Verify auth
   middleware is applied to every new route. Check that error responses don't leak
   internal paths, stack traces, or database schema details.

5. **Concurrency** — Mutex guards must not be held across `.await` points (this is a
   compile error with tokio::sync::Mutex but not std::sync::Mutex). Avoid
   `std::sync::Mutex` in async contexts. Check for missing `Send + Sync` bounds on
   types passed across `tokio::spawn`. Verify `Arc` / `RwLock` usage doesn't create
   deadlocks via lock ordering.

6. **Resource management** — Connection pools, file handles, and network sockets must
   be properly closed. Look for `BufReader`/`BufWriter` not flushed. Check that
   graceful shutdown propagates to all spawned tasks. Large allocations with
   user-controlled sizes should have bounds.

7. **API contracts** — Breaking changes to public types must be intentional. Serialize
   / Deserialize derives should use `#[serde(rename_all = "camelCase")]` consistently.
   New endpoints need OpenAPI/schema documentation. Error response shapes must match
   the project's error envelope pattern.

## Signal Patterns — Flag as Critical

- `unsafe { }` without a `// SAFETY:` comment explaining each invariant
- `.unwrap()` or `.expect()` outside of `#[cfg(test)]` or startup code
- `std::sync::Mutex` anywhere in async functions
- `std::env::var()` with `.unwrap()` or unvalidated input
- `.clone()` used to bypass a borrow checker error without a comment explaining why
- `String` / `Vec` allocations inside hot loops without capacity pre-allocation
- `tokio::spawn` whose `JoinHandle` is not `await`ed or stored for later
- `format!()` used to build SQL or shell commands

## Signal Patterns — Do NOT flag

- Code that passes `cargo clippy` and `cargo fmt` — style is handled by tooling
- `#[allow(dead_code)]` and `#[allow(unused)]` in WIP / draft modules
- Use of `anyhow` in application code or `Box<dyn Error>` in library code
- `debug!()` / `trace!()` calls left in production paths (they're compiled out)

## Verdict Guidelines

- **POSITIVE** if no Critical signal patterns are present, error handling is complete,
  and the diff would survive a production deploy.
- **NEGATIVE** if any Critical signal pattern is found, or there is a logic bug that
  would cause incorrect behavior at runtime.

At the end of your response, include exactly this metadata block:

[RS_GUARD_VERDICT_METADATA]
Verdict: POSITIVE or NEGATIVE
CriticalBugs: <count>
SecurityIssues: <count>
```

---

### Template: Frontend React + TypeScript (Next.js / Vite)

```markdown
# rs-guard Review Prompt — React + TypeScript Frontend

## Role
You are a senior frontend engineer reviewing a Pull Request for a React + TypeScript
application. The project uses Next.js App Router, React Server Components, and React
Query for data fetching. You care about correctness, security, accessibility, and
the user experience across slow networks and assistive technologies.

## Focus Areas (in priority order)

1. **Security** — `dangerouslySetInnerHTML` must only receive sanitized content from
   a library like DOMPurify, never raw user input or API responses. `'use server'`
   functions must validate and authorize every parameter — they are public RPC
   endpoints. Environment variables with `NEXT_PUBLIC_` prefix are shipped to the
   browser; secrets must never use this prefix. Check for API keys, tokens, or
   internal URLs in client components.

2. **React Correctness** — `useEffect` must have a complete dependency array or a
   comment explaining the omission. Cleanup functions must cancel subscriptions and
   abort fetches. `useCallback` / `useMemo` should only wrap values that depend on
   state/props that change. Server Components cannot use hooks, `useState`, `useEffect`,
   or browser-only APIs. `'use client'` boundaries must be intentional and minimal.

3. **Data fetching** — Every `useQuery` / `useSuspenseQuery` must handle: loading
   state (skeleton), error state (retry UI), empty state, and the success path.
   `staleTime` and `gcTime` must be appropriate for the data's freshness
   requirements. Mutations must invalidate or update the cache after success. Avoid
   `queryClient.refetchQueries` inside a mutation's `onSuccess` — use `invalidateQueries`
   instead for correctness under concurrent mutations.

4. **TypeScript** — No `as` casts that narrow types unsafely. No `any` or `// @ts-ignore`
   without a comment explaining why. Discriminated unions must be exhaustive — missing
   a variant in a switch/if-else chain is a bug. API response types must be validated
   at runtime (Zod / io-ts) before being trusted as the TypeScript type.

5. **Accessibility** — Every `<img>` must have a meaningful `alt` attribute (empty
   string for decorative images). Form controls must have associated `<label>` elements
   or `aria-label`. Interactive elements must be focusable and operable via keyboard.
   Color must not be the sole indicator of state. Modal dialogs must trap focus and
   restore it on close.

6. **Performance** — Heavy computations in render must be wrapped in `useMemo`. Large
   component trees should use `React.memo` when props are reference-stable. Images
   must use `next/image` with explicit `width`/`height` to prevent layout shift.
   Dynamic imports (`next/dynamic` or `React.lazy`) for routes and heavy components.
   Avoid creating new object/array/function references in render that break memoization.

7. **Error boundaries** — Every route segment should have an `error.tsx` boundary.
   API calls without error handling must be wrapped in try/catch with user-visible
   fallback UI. Promise rejections must not go unhandled.

## Signal Patterns — Flag as Critical

- `dangerouslySetInnerHTML={{ __html: anythingUserControlled }}`
- `NEXT_PUBLIC_` prefix on secrets, tokens, or API keys
- `as` casting a `string` to a union type without runtime validation
- `useEffect` with empty/missing dependency array and no comment
- Server action (`'use server'`) that does not validate its arguments
- `useSearchParams()` wrapped in `<Suspense>` without a fallback
- Images without `alt` text or explicit dimensions
- API fetch without error handling or loading state

## Signal Patterns — Do NOT flag

- `console.log` or `console.error` statements (they're fine in dev, tree-shaken in prod)
- CSS-in-JS patterns or Tailwind class ordering — handled by Prettier / linter
- Named export vs default export preference
- Arrow function vs function declaration in components
- `useCallback` on event handlers (reasonable optimization, not required)

## Verdict Guidelines

- **POSITIVE** if all states are handled (loading, error, empty, success) and no
  security or accessibility defects are present.
- **NEGATIVE** if any Critical signal pattern is found, or there is a logic error
  that would cause incorrect rendering or a runtime crash.

At the end of your response, include exactly this metadata block:

[RS_GUARD_VERDICT_METADATA]
Verdict: POSITIVE or NEGATIVE
CriticalBugs: <count>
SecurityIssues: <count>
```

---

### Template: Rails Monolith (with Hotwire, Sidekiq, RSpec)

```markdown
# rs-guard Review Prompt — Rails Monolith

## Role
You are a senior Rails engineer reviewing a Pull Request for a production monolith.
The stack is Ruby on Rails with PostgreSQL, Sidekiq for background jobs, Hotwire
(Turbo + Stimulus) for interactivity, Pundit for authorization, and RSpec for testing.
You understand that every change can cascade across models, callbacks, jobs, and views.

## Focus Areas (in priority order)

1. **Database safety** — Migrations must never lock the table for writes on production.
   Adding a column with a default value on a large table is a blocking operation unless
   done in multiple steps. Index creation must use `algorithm: :concurrently` and
   `disable_ddl_transaction!`. Foreign keys must have corresponding indexes. Renaming
   or dropping columns requires a multi-release deprecation cycle. Always define
   `def down` for reversible migrations — irreversible migrations need a comment
   explaining why.

2. **Data integrity** — Every `save!` / `update!` / `create!` must be wrapped in a
   transaction when it touches multiple records. Race conditions between web requests
   and background jobs on the same record must use `with_lock` or optimistic locking
   (`lock_version`). `touch: true` on `belongs_to` should not cause unnecessary cache
   invalidations on every child update. Avoid `counter_cache` unless you audit every
   path that creates/destroys the child.

3. **N+1 queries** — Every controller action must eager-load associations with
   `includes`, `preload`, or `eager_load`. Views must never call `Model.find` or
   `Model.where` — all data must be loaded in the controller. Use `strict_loading`
   mode in development to catch lazily-loaded associations. Check for `pluck` misuse:
   `Model.where(...).pluck(:id).map { |id| Model.find(id) }` is explicitly an N+1.

4. **Authorization** — Every controller action must call `authorize` or verify a
   policy via Pundit. Scoping (`policy_scope`) must be used on index actions to
   prevent data leakage. GraphQL mutations must authorize at the field level, not just
   the query level. Admin-only actions must never be reachable from non-admin
   controller ancestors.

5. **Coupling and boundaries** — AR callbacks (`after_save`, `after_create`) must not
   modify records in other bounded contexts. Service objects must return a consistent
   result type (`{ success:, response: }` hash). Controllers must not contain business
   logic — delegate to service objects. Models with more than 10 callbacks, 15 scopes,
   or 300 lines need extraction.

6. **Background jobs** — Every job must be idempotent: running it twice produces the
   same result. Job arguments must be simple types (no AR objects — pass IDs).
   Sidekiq jobs need retry/discard strategies defined. Jobs that depend on transient
   state (current time, external API status) must handle failure gracefully.

7. **Testing** — Every new model, service, and policy must have a corresponding spec.
   Controller specs must test authorization (both granted and denied). System specs
   must cover the happy path for new user-facing features. Avoid `allow_any_instance_of`
   and `any_instance` stubs. Prefer `let` over `let!` to avoid unnecessary database
   writes. Shared examples must be documented with what they expect from the calling
   context.

8. **Hotwire correctness** — Turbo Streams must broadcast to the correct channel and
   target the correct DOM ID. Stimulus controllers must disconnect cleanly (remove
   event listeners in `disconnect()`). Avoid rendering HTML partials as Turbo Stream
   responses without the correct MIME type.

## Signal Patterns — Flag as Critical

- Migration that adds a column with `default:` on an existing large table
- `Model.find(params[:id])` without ownership scoping or authorization
- `after_save` callback on a model that modifies a different bounded context
- Database query inside a loop or inside a view/partial
- Job argument that is an ActiveRecord object instead of an ID
- `save!` outside a transaction when multiple records are involved
- Controller that contains more than one service object call or model query
- Missing `def down` in migration without a comment

## Signal Patterns — Do NOT flag

- Rubocop violations — handled by linter in CI
- `TODO` or `FIXME` comments unless they describe a known bug being shipped
- Minor refactoring (extract method, rename variable) that doesn't change behavior
- RSpec `let` ordering — handled by test linter

## Verdict Guidelines

- **POSITIVE** if migrations are safe, data integrity is preserved, authorization is
  correct, and the diff doesn't introduce coupling.
- **NEGATIVE** if any Critical signal pattern is found, the migration cannot be rolled
  back, or data loss would occur in production.

At the end of your response, include exactly this metadata block:

[RS_GUARD_VERDICT_METADATA]
Verdict: POSITIVE or NEGATIVE
CriticalBugs: <count>
SecurityIssues: <count>
```

### When to Use Which Template

| Template | For projects with |
|---|---|
| **Rust Backend** | tokio-based services, sqlx/diesel databases, actix-web/axum APIs |
| **React + TypeScript** | Next.js App Router, Vite SPAs, React Query, React Server Components |
| **Rails Monolith** | ActiveRecord + Sidekiq + Hotwire + RSpec stacks |

If your project uses a different stack, use the closest template as a starting point:
- **Go backends** — adapt the Rust template, replacing ownership patterns with goroutine leak / channel deadlock / nil pointer checks
- **Vue/Svelte frontends** — adapt the React template, replacing hooks with their `watch`/`reactive` equivalents
- **Django/Laravel** — adapt the Rails template, replacing ActiveRecord callbacks with their ORM equivalents
- **Mixed polyglot repos** — pick the template for the language that dominates the diff

The prompts are designed to be combined — mix focus areas from multiple templates if your
project spans domains.

---

## Installation and Setup

rs-guard can be installed in three ways: download a pre-built binary (recommended for CI),
install via cargo, or build from source.

### Quick Start — GitHub Actions (Copy-Paste)

Create `.github/workflows/ai-review.yml`:

```yaml
name: AI Code Review
on:
  pull_request:
    types: [opened, synchronize]

permissions:
  pull-requests: write
  contents: read

jobs:
  review:
    runs-on: ubuntu-latest
    if: ${{ !github.event.pull_request.head.repo.fork }}
    steps:
      - uses: actions/checkout@v4

      - name: Download rs-guard
        run: |
          curl -L -o rs-guard \
            https://github.com/nebulaideas/rs-guard/releases/latest/download/rs-guard
          chmod +x rs-guard

      - name: AI Code Review
        run: ./rs-guard
        env:
          DEEPSEEK_API_KEY: ${{ secrets.DEEPSEEK_API_KEY }}
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          PR_NUMBER: ${{ github.event.pull_request.number }}
          REPO_FULL_NAME: ${{ github.repository }}
```

Then add your API key in **Settings → Secrets and variables → Actions → `DEEPSEEK_API_KEY`**.

### Local Setup (Pre-commit Hook)

Install the binary:

```bash
# Option A: Pre-built binary
curl -L -o /usr/local/bin/rs-guard \
  https://github.com/nebulaideas/rs-guard/releases/latest/download/rs-guard
chmod +x /usr/local/bin/rs-guard

# Option B: cargo install
cargo install rs-guard
```

Create `.git/hooks/pre-commit`:

```bash
#!/bin/sh
# Save this as .git/hooks/pre-commit and make it executable: chmod +x .git/hooks/pre-commit
set -e

export DEEPSEEK_API_KEY="${DEEPSEEK_API_KEY:-}"
if [ -z "$DEEPSEEK_API_KEY" ]; then
  echo "⚠️  DEEPSEEK_API_KEY not set — skipping AI review"
  exit 0
fi

# Skip if nothing is staged
if git diff --cached --quiet; then
  exit 0
fi

rs-guard --prompt-file .github/review-prompt.md
if [ $? -eq 2 ]; then
  echo ""
  echo "🚫 Commit blocked: review returned REQUEST_CHANGES"
  echo "   Skip with: git commit --no-verify"
  exit 1
fi
exit 0
```

Create `.github/review-prompt.md` by copying the template for your stack from above.

To bypass the hook on a single commit:

```bash
git commit -m "skip review" --no-verify
```

### Verifying the Setup

```bash
# Check the binary is installed
rs-guard --version

# Run a test review on a local diff file
rs-guard --diff-file /path/to/test.diff --no-cache
```

---

## Troubleshooting

### `GITHUB_TOKEN is required in CI mode`

Check that your workflow step includes `GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}` in `env`.

### `API key not found. Set DEEPSEEK_API_KEY for provider 'deepseek'`

Set the env var for your provider. Check [docs/PROVIDERS.md](PROVIDERS.md) for how to obtain keys.

### `Unknown provider: 'xxx'`

Supported providers: `deepseek`, `kimi`, `qwen`, `openrouter`, `openai`.

### `Provider base URL is not a recognized LLM provider endpoint`

In CI mode, URLs are checked against a SSRF allowlist. Use a known provider URL.

### `Diff too large`

The diff exceeds 100 KB / 1500 lines. In CI: an explanatory `COMMENT` is posted. In local/file mode: exits `0`.

### `Diff chunked: omitted {} middle lines`

The diff was truncated (400 head + 400 tail preserved). Expected for large PRs.

### `Review body exceeds GitHub's character limit`

GitHub has a 65536 character limit for review bodies. If your review exceeds this:

- Use a shorter prompt (e.g., remove detailed instructions)
- The diff will be chunked automatically for large PRs
- Consider using `--max-tokens` to limit LLM output length

### `Cache hit — using cached LLM response`

The same diff+prompt+provider+model+temperature combination was cached within the 24-hour TTL. Pass `--no-cache` for a fresh call.

### Review posted as `COMMENT` instead of `APPROVE`/`REQUEST_CHANGES`

The token may lack `pull-requests: write` scope. See [Permission Fallback](#permission-fallback).

### Local mode produces no output

There may be no staged changes. Run `git add .` first.

### `Failed to read config file '.reviewer.toml'`

Check file permissions and TOML syntax. See `rs-guard --help` for the expected config path.

---

## See Also

- [docs/ARCHITECTURE.md](ARCHITECTURE.md) — System design
- [docs/API.md](API.md) — Library module API reference
- [docs/PROVIDERS.md](PROVIDERS.md) — Per-provider setup
- [docs/CONFIGURATION.md](CONFIGURATION.md) — `.reviewer.toml` reference
- [docs/LOCAL_MODE.md](LOCAL_MODE.md) — Pre-commit hook setup
- [examples/github-actions-workflow/ai-review.yml](../examples/github-actions-workflow/ai-review.yml) — Complete CI workflow
