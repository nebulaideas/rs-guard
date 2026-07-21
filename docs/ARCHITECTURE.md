# rs-guard — Architecture

This document describes the system design, key decisions, and extension points of rs-guard.

---

## Overview

rs-guard is a **single-binary, single-pass review pipeline**. It fetches a diff, calls an LLM once, parses the structured response, and either submits a GitHub review (CI mode) or prints a colored terminal summary (local mode). No intermediate state is persisted, no comments are posted during analysis, and no database or server is required.

---

## Pipeline Flow

```mermaid
flowchart TD
    A(["rs-guard invoked"]) --> B["Parse CLI Args\n(clap)"]
    B --> C["Resolve Config\nCLI → env → TOML → defaults"]
    C --> D{"Diff Source?"}

    D -->|"--diff-file"| E["Read Diff File\nfetch_file_diff()"]
    D -->|"GITHUB_ACTIONS=true"| F["Fetch PR Diff\nfetch_pr_diff() via GitHub REST API"]
    D -->|"local"| G["Fetch Staged Diff\nfetch_local_diff()\ngit diff --cached"]

    E & F & G --> H["Validate Diff\nsize + content markers"]
    H --> I["Chunk Diff\nchunk_diff()\n400 head + 400 tail lines"]

    I --> J["Check Cache\nDiffCache::get()"]
    J -->|"Hit"| M
    J -->|"Miss"| K["Call LLM\nprovider.chat_completion()\nwith_retry + circuit breaker"]
    K --> L["Store in Cache\nDiffCache::set()"]
    L --> M["Parse Verdict\nparse_verdict()\n[RS_GUARD_VERDICT_METADATA] block"]

    M --> N["Write Artifacts\nreview-result.txt\nrs-guard-metrics.json"]

    N --> O{"Mode?"}
    O -->|"CI"| P["Submit GitHub Review\nsubmit_review()\nAPPROVE / REQUEST_CHANGES / COMMENT"]
    O -->|"Local"| Q["Print Colored Summary\nprint_colored_summary()\nExit 2 if REQUEST_CHANGES"]

    P --> R(["Exit 0"])
    Q --> S(["Exit 0 or 2"])
```

---

## Module Structure

```mermaid
graph LR
    main["main.rs"] --> pipeline
    main --> cli

    pipeline["pipeline.rs\nOrchestration"] --> cache
    pipeline --> config
    pipeline --> diff
    pipeline --> github
    pipeline --> llm
    pipeline --> output
    pipeline --> redact
    pipeline --> verdict

    diff["diff.rs"] --> http
    diff --> retry
    github["github.rs"] --> http
    github --> retry

    llm["llm/"] --> factory
    llm --> providers
    factory --> generic_client : "GenericOpenAiCompatibleClient"
    generic_client --> providers : "ProviderMeta lookup"

    config["config.rs"] --> llm
    config --> http

    cache["cache.rs"] --> error
    retry["retry.rs"] --> error
    output["output.rs"] --> verdict
    verdict["verdict.rs"] --> error
    error["error.rs"]
    redact["redact.rs"]
    http["http.rs"] --> error
```

---

## Key Modules

### Module inventory

| Module | Role |
|--------|------|
| `cli` / `main` | Clap args + process exit mapping |
| `config` | CLI → env → TOML → defaults |
| `diff` | PR / local / file diffs + chunking |
| `pipeline` | End-to-end orchestration |
| `llm/` | `LlmProvider` trait, factory, `ProviderMeta`, generic client |
| `verdict` | Metadata parse + review state |
| `github` | Review submit + dismiss previous blockers |
| `cache` | SHA-256 keyed LLM response cache |
| `retry` | Backoff + optional circuit breaker |
| `redact` | Secret redaction for logs/artifacts (and outbound diffs when enabled) |
| `rules` | Project rules file detection/loading (`AGENTS.md`, etc.) |
| `scaffold` | `init` / `generate-prompt` / `generate-workflow` / `validate-config` |
| `repo` | Git working-tree root helpers |
| `output` | Artifacts, metrics, colored summary |
| `http` | Shared clients + SSRF URL validation |
| `error` | `RsGuardError` |

### `pipeline.rs` — Orchestration

The single entry point for all review logic. `run_pipeline()` accepts a `Config` and optional diff file path, drives the full workflow, and returns a `PipelineResult` enum instead of calling `process::exit()`. This keeps the library testable without subprocess spawning.

```rust
pub enum PipelineResult {
    Success,       // exit 0
    ReviewBlocked, // exit 2 — local mode REQUEST_CHANGES
}
```

### `llm/` — Provider Abstraction

All providers implement the `LlmProvider` async trait:

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &'static str;

    async fn chat_completion(
        &self,
        system_prompt: &str,
        user_content: &str,
        temperature: f32,
    ) -> Result<String, RsGuardError>;
}
```

The factory maps a provider name string to a `Box<dyn LlmProvider>` via a single
data-driven path: [`GenericOpenAiCompatibleClient`](../src/llm/generic_client.rs)
parameterized by [`ProviderMeta`](../src/llm/providers.rs).

Adding a new OpenAI-compatible provider requires:

1. A `ProviderMeta` entry in `src/llm/providers.rs` (name, env var, default model, base URL, optional `result_format` / headers / variants)
2. SSRF allowlist update in `http.rs` if the host is new
3. Docs (`PROVIDERS.md`) and tests (`known_provider_names`, factory smoke tests)

No new client module is needed for standard OpenAI-compatible APIs. See [docs/API.md](API.md) for the full guide.

### `cache.rs` — Response Caching

Cache entries are keyed by a SHA-256 hash of all LLM call parameters that affect
the outgoing request (see also [PERFORMANCE.md](PERFORMANCE.md)):

```
key = SHA-256(
  diff_content | prompt | project_rules |
  provider | model | variant | temperature |
  base_url | max_tokens | result_format
)
```

Each `.cache` file stores:

- **Line 1:** Unix timestamp (seconds since epoch) — stored in content, not mtime, for reliability
- **Line 2+:** The raw LLM response

Writes are atomic: content is written to a `.tmp` file in the same directory, then renamed into place. This prevents partial reads from concurrent rs-guard processes.

The cache enforces a configurable maximum size (default: 100 MB) using LRU cleanup: entries are sorted by stored timestamp and the oldest are removed until the total falls below the limit.

### `retry.rs` — Retry + Circuit Breaker

**Retry policy:** Up to 3 retries with exponential backoff (1s, 2s, 4s base) and ±25% jitter. Only retries on retryable errors (429, 5xx, timeouts). Non-retryable errors (404, 401, 403) are returned immediately.

**Circuit breaker:** Simple two-state (Closed/Open). Opens after N consecutive failures; auto-resets to Closed after a configurable cooldown. No half-open state (keeps complexity low for v1). Thread-safe via `Arc<Mutex<>>`. Opt-in, disabled by default.

```mermaid
stateDiagram-v2
    [*] --> Closed
    Closed --> Open : N consecutive failures
    Open --> Closed : Cooldown elapsed + request allowed
    Closed --> Closed : Success (reset failure count)
```

### `diff.rs` — Diff Fetching + Chunking

Three diff sources with different behavior:

| Source                      | Function             | On `DiffTooLarge`                     |
| --------------------------- | -------------------- | ------------------------------------- |
| GitHub API (`--ci`)         | `fetch_pr_diff()`    | Posts an explanatory `COMMENT` review |
| File (`--diff-file`)        | `fetch_file_diff()`  | Prints to stderr, exits 0             |
| Local (`git diff --cached`) | `fetch_local_diff()` | Prints to stderr, exits 0             |

After fetching, `chunk_diff()` trims large diffs to the first 400 + last 400 lines (configurable via `chunk_head_lines` / `chunk_tail_lines` in `.reviewer.toml`). Returns `Cow<str>` — borrowed when no truncation is needed (zero allocation in the common case).

### `verdict.rs` — Verdict Parsing

The LLM is instructed to append a structured block at the end of its response:

```
[RS_GUARD_VERDICT_METADATA]
Verdict: POSITIVE
CriticalIssues: 0
SecurityIssues: 0
ImportantIssues: 0
Suggestions: 0
```

The parser extracts this block via substring scanning (with tag-counting fallback if the
block is missing) and applies the review state logic in `determine_review_state`:

```
NEGATIVE
  or security_issues > 0
  or critical_issues > 0
  or (important_threshold > 0 and important_issues >= important_threshold)
    →  REQUEST_CHANGES

important_issues > 0 (but below threshold)
    →  COMMENT

otherwise (POSITIVE and all blocking counts zero)
    →  APPROVE
```

`important_threshold` defaults to `3` and is configurable via CLI/env/TOML.
The legacy metadata field `CriticalBugs:` is still accepted as an alias for
`CriticalIssues:` for older prompt files.

### `github.rs` — Review Submission

Submits reviews via the GitHub REST API (`POST /repos/{owner}/{repo}/pulls/{pr}/reviews`). Includes `<!-- rs-guard-bot -->` as an HTML comment signature in the review body for identification.

When the new state is non-blocking (`APPROVE` or `COMMENT`), any previous rs-guard `CHANGES_REQUESTED` reviews are dismissed to clean up the PR review list.

Fallback: if `APPROVE` or `REQUEST_CHANGES` fails with HTTP 403 (insufficient token permissions), the state is downgraded to `COMMENT` and resubmitted.

---

## CI vs Local Mode Detection

```mermaid
flowchart LR
    A["Start"] --> B{"GITHUB_ACTIONS\nenv var set?"}
    B -->|"Yes"| C["CI Mode\nFetch PR diff via API\nSubmit GitHub review"]
    B -->|"No"| D{"--diff-file\nflag set?"}
    D -->|"Yes"| E["File Mode\nRead diff from path\nPrint colored summary"]
    D -->|"No"| F["Local Mode\ngit diff --cached\nPrint colored summary\nExit 2 if blocked"]
```

---

## Security Model

### SSRF Protection

All provider base URLs are validated against a per-provider allowlist before any HTTP request is made. This ensures that `Authorization` headers containing API keys are never sent to an attacker-controlled host.

- **CI mode (GitHub API):** URL must match `api.github.com` or the configured GitHub Enterprise base URL.
- **Provider APIs:** URL must match the known canonical base URL for the provider.
- **Local/test mode:** Loopback addresses (`127.0.0.1`, `localhost`, `[::1]`) are allowed — enables wiremock-based testing.

### Secret Handling

- **Outbound diffs** are scrubbed with `redact_secrets_with_count` before cache keying
  and the LLM call. Matches are replaced with `[REDACTED]`; the count is stored in
  metrics and shown in local mode.
- API keys are read from environment variables, never from the diff content or command-line arguments.
- The `redact.rs` module strips known secret patterns from LLM responses before writing artifacts or submitting reviews.
- `log_redacted()` truncates sensitive content in debug log output.
- Secrets are never written to the response cache.

### Token Permissions

rs-guard requests the minimum GitHub token scope needed: `pull-requests: write`. If the token only has `read` permission, the submission is downgraded to `COMMENT` (which requires only `read` for public repos, or a token that can post comments).

---

## Performance Characteristics

| Metric                          | Typical Value         |
| ------------------------------- | --------------------- |
| Binary size (release, stripped) | ~5 MB                 |
| Cold startup to first API call  | < 100ms               |
| End-to-end latency (cache miss) | 3–15s (LLM-dominated) |
| End-to-end latency (cache hit)  | < 200ms               |
| Memory footprint                | < 50 MB               |
| Diff size limit                 | 100 KB / 1500 lines   |

The Criterion benchmarks in `benches/verdict.rs` cover verdict parsing (the only CPU-intensive step). Run with:

```bash
cargo bench --bench verdict
```

---

## Extending the Codebase

### Adding a New LLM Provider

Add a `ProviderMeta` entry in `src/llm/providers.rs` (and allowlist/docs/tests).
Do **not** add a new per-provider client module — all OpenAI-compatible providers
share `GenericOpenAiCompatibleClient`. See [docs/API.md](API.md#adding-a-new-provider).

### Adding a New Diff Source

1. Add a new function in `diff.rs` returning `Result<DiffResult, RsGuardError>`
2. Add the corresponding CLI flag or env var detection in `cli.rs` / `config.rs`
3. Add a new branch in the diff-source `if/else` block in `pipeline.rs`
4. Handle the `DiffTooLarge` case consistently with the existing sources

### Modifying the Verdict Format

The metadata block format is defined in the default prompt (`config.rs::DEFAULT_PROMPT`) and parsed in `verdict.rs::parse_verdict()`. Both must be kept in sync. Add tests to `verdict_tests.rs` for any new fields.
