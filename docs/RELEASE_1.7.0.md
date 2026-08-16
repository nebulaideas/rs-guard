# rs-guard v1.7.0 Release Notes

**Date:** 2026-08-15

## Highlights

1. **Structured findings** — `--findings` mode asks the LLM for a structured JSON array of findings with file path, line number, and severity. Counts use a max-rule merge so findings can add evidence but never suppress a blocking verdict (#110).
2. **Inline review comments** — `--inline-comments` posts each finding as an inline comment on the exact diff line. Unmappable findings are appended to the review body (#108).
3. **GitHub Check Runs** — `--check-run` creates a Check Run in addition to the PR review, enabling branch protection without `APPROVE`/`REQUEST_CHANGES` permissions. Conclusion mapping: `APPROVE`→`success`, `REQUEST_CHANGES`→`failure`, `COMMENT`→`neutral` (#109).
4. **Review body truncation** — review bodies exceeding GitHub's 65,536 character limit are now truncated with a visible notice instead of failing the review. The findings JSON block is stripped before truncation so only prose is cut (#111).
5. **API token usage** — when the LLM provider returns `usage` data, rs-guard uses real token counts for metrics and cost estimation instead of character heuristics. A `token_source` field (`"api"`, `"mixed"`, or `"estimate"`) indicates the source (#115).
6. **Ollama and Gemini providers** — `--provider ollama` for local inference (no API key required, loopback-only) and `--provider gemini` for Google's OpenAI-compatible endpoint with `flash`/`pro` variants (#116).
7. **MSRV raised to 1.88** — removed `rust-toolchain.toml` pin; CI uses `stable` via `dtolnay/rust-toolchain` (#135).

## Breaking changes

- **`LlmProvider::chat_completion` return type** changed from `Result<String, RsGuardError>` to `Result<ChatCompletionResult, RsGuardError>`. External trait implementors must update.
- **`ProviderMeta` gains required `api_key_required` field.** Anyone constructing `ProviderMeta` literals must add the field.
- **MSRV raised from 1.82 to 1.88** (Rust 1.88 is over a year old).

## Upgrade notes

- **Ollama** is local-mode only — its loopback URL is rejected in CI mode by `validate_provider_base_url`. Use in local mode (unset `GITHUB_ACTIONS`).
- **Gemini** requires `GEMINI_API_KEY` from Google AI Studio. Default model is `gemini-2.5-flash`; `--variant pro` selects `gemini-2.5-pro`.
- **Token metrics** now show `"Tokens"` instead of `"Est. Tokens"` when API usage data is available. The `--format json` output includes a `token_source` field.
- **Check Runs** require `checks: write` permission on the GitHub token.

## Publish checklist

1. `cargo test && cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --all -- --check`
2. `cargo deny check && cargo audit`
3. Tag `v1.7.0` → push tags
4. Confirm GitHub Release workflow + `cargo publish` / crates.io + docs.rs

## Ticket → PR map

| Issue | Topic | PR |
|-------|--------|-----|
| #108 | Inline review comments | #126–#130 |
| #109 | GitHub Check Runs | #131 |
| #110 | Structured findings mode | #126–#130 |
| #111 | Review body truncation | #136 |
| #112 | Documentation updates | #132–#133 |
| #115 | API token usage | #138 |
| #116 | Ollama and Gemini presets | #139 |
| #135 | MSRV 1.88, remove toolchain pin | #137 |

Milestone: [v1.7.0 - GitHub-native UX](https://github.com/nebulaideas/rs-guard/milestone/3)

## Related docs

- [v1.6.0 — Review depth](v1.6.md)
- [v1.8.0 — Scale](v1.8.md)
- [PROVIDERS](PROVIDERS.md) · [USAGE](USAGE.md) · [ARCHITECTURE](ARCHITECTURE.md) · [CONFIGURATION](CONFIGURATION.md)
