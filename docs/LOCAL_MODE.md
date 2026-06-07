# Local Mode Guide

diffguard-rs supports local pre-commit execution, analyzing `git diff --cached` output before you commit changes.

---

## Detection

Local mode is automatically detected when the `GITHUB_ACTIONS` environment variable is **absent**.

```bash
# Local mode (no GITHUB_ACTIONS)
diffguard

# CI mode (GITHUB_ACTIONS is set by GitHub)
GITHUB_ACTIONS=true diffguard
```

---

## Behavior

In local mode, diffguard-rs:

1. Runs `git diff --cached` to fetch staged changes
2. Sends the diff to the configured LLM provider
3. Prints a colored summary to the terminal
4. Exits with code `2` if the review returns `REQUEST_CHANGES`

### Exit Codes

| Code | Meaning |
|---|---|
| `0` | Review completed successfully (`APPROVE` or `COMMENT`) |
| `1` | Error occurred (API failure, config error, etc.) |
| `2` | Review returned `REQUEST_CHANGES` — blocks commit |

---

## Pre-Commit Hook Setup

### Option 1: Copy the Example Hook

```bash
cp examples/local-review/pre-commit-hook.sh .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

### Option 2: Manual Hook

Create `.git/hooks/pre-commit`:

```bash
#!/bin/sh

# Required: set your API key
export DEEPSEEK_API_KEY="your-api-key"

# Optional: override provider/model
export DIFFGUARD_PROVIDER="deepseek"

echo "Running diffguard pre-commit review..."

diffguard
EXIT_CODE=$?

if [ "$EXIT_CODE" -eq 0 ]; then
    exit 0
elif [ "$EXIT_CODE" -eq 2 ]; then
    echo "diffguard: Review returned REQUEST_CHANGES. Commit aborted."
    echo "Bypass with: git commit --no-verify"
    exit 1
else
    echo "diffguard: Error occurred (exit code $EXIT_CODE)."
    exit 1
fi
```

Make it executable:

```bash
chmod +x .git/hooks/pre-commit
```

### Bypassing the Hook

If you need to commit despite the review:

```bash
git commit --no-verify  # or -n
```

---

## Running Locally Without a Hook

You can also run diffguard manually before committing:

```bash
# Using default provider (deepseek)
export DEEPSEEK_API_KEY="your-api-key"
diffguard

# Using a different provider
export KIMI_API_KEY="your-api-key"
diffguard --provider kimi

# With custom config
diffguard --config ./my-review-config.toml
```

---

## Terminal Output

Local mode prints a color-coded summary:

```
diffguard-rs Review

✓ State: APPROVE

Verdict:         POSITIVE
Critical Bugs:   0
Security Issues: 0

The code looks good. No issues found.

--- Metadata ---
Provider:    deepseek
Model:       deepseek-v4-flash
Temperature: 0.1
Diff Lines:  42
```

States are color-coded:
- **Green (`APPROVE`)** — Code is ready to merge
- **Red (`REQUEST_CHANGES`)** — Issues must be addressed
- **Yellow (`COMMENT`)** — Minor concerns, human review recommended

---

## Tips

- **No staged changes?** diffguard-rs exits cleanly with "No staged changes to review."
- **Diff too large?** Local mode warns and exits `0` (does not block).
- **Want a custom prompt?** Use `--prompt-file` or create `.github/review-prompt.md`.
- **Need faster reviews?** Use a smaller/faster model like `deepseek-v4-flash`.
