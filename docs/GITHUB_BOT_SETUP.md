# GitHub Bot / Machine User Setup

rs-guard submits reviews to GitHub on your behalf. To do this cleanly and
securely — without tying reviews to a personal account or sharing a personal
access token (PAT) across your team — set up a **dedicated GitHub identity**.
This document covers the two recommended approaches.

> **Why not a personal PAT?** A personal PAT attributes every review to a
> human (clutters the author's notifications and activity), ties the workflow
> to that person's employment/permissions, and broadens the blast radius if
> the secret leaks. A dedicated identity is auditable, revocable, and isolated.

---

## Table of Contents

- [Option A — Dedicated Machine-User Account (simplest)](#option-a--dedicated-machine-user-account-simplest)
- [Option B — GitHub App (recommended for orgs)](#option-b--github-app-recommended-for-orgs)
- [Token permissions](#token-permissions)
- [Storing the secret](#storing-the-secret)
- [Attribution (OpenRouter)](#attribution-openrouter)

---

## Option A — Dedicated Machine-User Account (simplest)

Create a normal GitHub user account that exists only to run rs-guard. This is
the lightest-weight option and works for any repo (public, private, personal,
or org) with no app-configuration overhead.

1. Create a new GitHub account, e.g. `yourorg-review-bot`. Use a shared team
   email or a distribution list (not a personal inbox) so the account is not
   tied to one employee.
2. **Add the bot as a collaborator** to each repository it should review, with
   the minimal role. For PR review it needs **Write** access (it must be able
   to post review events). If you only want it to comment (not block merges),
   **Triage** may suffice — but note that submitting `REQUEST_CHANGES` review
   state requires Write.
3. Create a **fine-grained personal access token** (recommended) or a classic
   PAT with the `repo` scope. Fine-grained is preferred:
   - GitHub → Settings → Developer settings → Personal access tokens →
     **Fine-grained tokens** → Generate new token
   - **Repository access**: select only the repos the bot reviews.
   - **Permissions** → Repository permissions:
     - `Pull requests`: **Read and write** (to submit reviews)
     - `Contents`: **Read-only** (to fetch PR diff metadata)
   - Set a reasonable expiration (90 days) and rotate.
4. Store the token as a repository/organization secret named `RS_GUARD_GITHUB_TOKEN`
   (see [Storing the secret](#storing-the-secret)).

**Pros:** trivial setup, works everywhere, the bot appears as a normal user in
the PR timeline.

**Cons:** the token is a PAT (scopable via fine-grained, but still a user
token); collaborator access must be granted per-repo manually.

---

## Option B — GitHub App (recommended for orgs)

A GitHub App is the most robust identity for automation. It is first-class on
GitHub, scopes permissions precisely, and installs across many repos at once.

1. Create the App:
   - Org → Settings → Developer settings → GitHub Apps → **New GitHub App**
   - **App name**: e.g. `rs-guard`
   - **Repository permissions** (read-only unless noted):
     - `Pull requests`: **Read and write** (submit reviews)
     - `Contents`: **Read-only** (fetch diff)
   - **Subscribe to events**: `Pull request` (only if you trigger on webhook;
     for GHA-driven runs this is optional)
   - Note the **App ID** and generate a **private key** (`.pem`).
2. **Install** the App into your org/account, selecting the target repos.
3. At runtime, exchange the App ID + private key for a short-lived installation
   token. In GitHub Actions this is automated by
   [`tibdex/github-app-token`](https://github.com/tibdex/github-app-token) or
   [`actions/create-github-app-token`](https://github.com/actions/create-github-app-token):

   ```yaml
   jobs:
     review:
       steps:
         - id: app-token
           uses: actions/create-github-app-token@v1
           with:
             app-id: ${{ vars.RS_GUARD_APP_ID }}
             private-key: ${{ secrets.RS_GUARD_APP_PRIVATE_KEY }}
         - uses: actions/checkout@v4
         - run: cargo install rs-guard --locked
         - env:
             RS_GUARD_GITHUB_TOKEN: ${{ steps.app-token.outputs.token }}
             DEEPSEEK_API_KEY: ${{ secrets.DEEPSEEK_API_KEY }}
           run: rs-guard --pr ${{ github.event.pull_request.number }}
   ```

   Store `RS_GUARD_APP_ID` as a **variable** and `RS_GUARD_APP_PRIVATE_KEY` as
   a **secret**.

**Pros:** no long-lived PAT, granular per-repo install, auditable as a bot
actor, scales across an org.

**Cons:** slightly more setup than Option A.

---

## Token permissions

rs-guard needs exactly these capabilities on the target repository:

| Capability | Classic scope | Fine-grained permission |
| List/read PR metadata + diff | `repo` | `Contents: Read` |
| Submit a review (APPROVE / REQUEST_CHANGES / COMMENT) | `repo` | `Pull requests: Read and write` |

No other scopes are required. Never grant `delete_repo`, `admin:*`, or
workflow-modifying scopes to the review identity.

---

## Storing the secret

- **GitHub Actions:** Settings → Secrets and variables → Actions →
  `RS_GUARD_GITHUB_TOKEN` (secret). Reference as
  `${{ secrets.RS_GUARD_GITHUB_TOKEN }}`.
- **Other CI:** export it in the runner environment as
  `RS_GUARD_GITHUB_TOKEN`.
- **Local/pre-commit:** keep it out of `.reviewer.toml`; load it from your
  shell environment or a secrets manager.

rs-guard reads the token from `RS_GUARD_GITHUB_TOKEN` (or `GITHUB_TOKEN` as a
fallback). See [docs/CONFIGURATION.md](CONFIGURATION.md) for the full variable
list.

---

## Attribution (OpenRouter)

If you use OpenRouter as the LLM gateway, rs-guard sends the `HTTP-Referer`
header (default `https://github.com/nebulaideas/rs-guard`) for rate-limit
attribution. Point it at your bot's own repo for accurate accounting:

```toml
[providers.openrouter]
http_referer = "https://github.com/yourorg/your-repo"
```
