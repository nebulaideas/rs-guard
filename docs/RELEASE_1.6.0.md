# rs-guard v1.6.0 Release Notes

**Date:** 2026-07-21

## Highlights

1. **Safer LLM calls** — secrets in diffs are redacted before they leave the machine (#102 / PR #119).
2. **Larger, smarter diffs** — higher defaults, path include/exclude, filter-before-size (#103 / PR #124).
3. **JSON output** — `--format json` for CI and tooling (#104 / PR #123).
4. **Local branch review** — `--base origin/main` for PR-like local runs (#105 / PR #122).
5. **Docs + deprecations** — architecture/performance accuracy (#106 / PR #120); `CriticalBugs` warn (#107 / PR #121).

## Upgrade notes

- **Defaults:** hard reject is now **500 KB / 5000 lines** (was 100 KB / 1500).
- **Raw fetch ceiling:** unfiltered fetches up to **10 MB / 100k lines** before path filters + user limits.
- **Secrets:** expect `[REDACTED]` in outbound diffs when patterns match.
- **Prompts:** prefer `CriticalIssues:` over `CriticalBugs:` (alias still works with a warning).
- **CI:** this release includes `crossbeam-epoch` ≥ 0.9.20.

## Publish checklist

1. Merge PR #122 if still open, then rebase this branch on `main`.
2. `cargo test && cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --all -- --check`
3. `cargo deny check && cargo audit`
4. Merge release PR → tag `v1.6.0` → push tags
5. Confirm GitHub Release workflow + `cargo publish` / crates.io + docs.rs

## Ticket → PR map

| Issue | Topic | PR |
|-------|--------|-----|
| #102 | Outbound secret redaction | #119 |
| #103 | Size limits + path filters | #124 |
| #104 | `--format json` | #123 |
| #105 | Local `--base` | #122 |
| #106 | Docs drift | #120 |
| #107 | CriticalBugs deprecation | #121 |

Milestone: [v1.6.0 - Review depth](https://github.com/nebulaideas/rs-guard/milestone/2)
