# Local Mode Guide

rs-guard supports local pre-commit execution, analyzing `git diff --cached` output before you commit changes.

---

## Detection

Local mode is automatically detected when the `GITHUB_ACTIONS` environment variable is **absent**.

```bash
# Local mode (no GITHUB_ACTIONS)
rs-guard

# CI mode (GITHUB_ACTIONS is set by GitHub)
GITHUB_ACTIONS=true rs-guard
```

---

## Behavior

In local mode, rs-guard:

1. Runs `git diff --cached` to fetch staged changes
2. Sends the diff to the configured LLM provider
3. Prints a colored summary to the terminal
4. Exits with code `2` if the review returns `REQUEST_CHANGES`

### Exit Codes

| Code | Meaning                                                |
| ---- | ------------------------------------------------------ |
| `0`  | Review completed successfully (`APPROVE` or `COMMENT`) |
| `1`  | Error occurred (API failure, config error, etc.)       |
| `2`  | Review returned `REQUEST_CHANGES` — blocks commit      |

---

## Pre-Commit Hook Setup

### Option 1: Copy the Example Hook

```bash
cp examples/local-review/pre-commit-hook.sh .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

The example hook includes:

- Automatic API key detection (checks for all supported providers)
- Optional config file loading from `~/.config/rs-guard/env`
- Helpful error messages if no API key is found
- Security warnings about not hardcoding keys

### Option 2: Config File (Recommended for Security)

Create a config file at `~/.config/rs-guard/env`:

```bash
mkdir -p ~/.config/rs-guard
cat > ~/.config/rs-guard/env << 'EOF'
# rs-guard API key configuration
# Add this file to your .gitignore if it contains secrets

export DEEPSEEK_API_KEY="your-api-key"
# Or use another provider:
# export KIMI_API_KEY="your-api-key"
# export DASHSCOPE_API_KEY="your-api-key"
# export OPENROUTER_API_KEY="your-api-key"
# export OPENAI_API_KEY="your-api-key"
EOF
```

Add to your `.gitignore` (if the config file is in your repo):

```bash
echo "~/.config/rs-guard/env" >> .gitignore
```

### Option 3: Manual Hook

Create `.git/hooks/pre-commit`:

```bash
#!/bin/sh

# Load optional config file
if [ -f ~/.config/rs-guard/env ]; then
    . ~/.config/rs-guard/env
fi

# Required: set your API key (if not in config file)
# export DEEPSEEK_API_KEY="your-api-key"

# Optional: override provider/model
export RS_GUARD_PROVIDER="deepseek"

echo "Running rs-guard pre-commit review..."

rs-guard
EXIT_CODE=$?

if [ "$EXIT_CODE" -eq 0 ]; then
    exit 0
elif [ "$EXIT_CODE" -eq 2 ]; then
    echo "rs-guard: Review returned REQUEST_CHANGES. Commit aborted."
    echo "Bypass with: git commit --no-verify"
    exit 1
else
    echo "rs-guard: Error occurred (exit code $EXIT_CODE)."
    exit 1
fi
```

Make it executable:

```bash
chmod +x .git/hooks/pre-commit
```

### Security Best Practices

⚠️ **IMPORTANT**: Never hardcode API keys in your pre-commit hook or commit them to git.

- Use environment variables or the `~/.config/rs-guard/env` config file
- Add config files containing secrets to your `.gitignore`
- Rotate API keys if they're accidentally committed
- Use different API keys for different projects when possible

### Bypassing the Hook

If you need to commit despite the review:

```bash
git commit --no-verify  # or -n
```

---

## Running Locally Without a Hook

You can also run rs-guard manually before committing:

```bash
# Using default provider (deepseek)
export DEEPSEEK_API_KEY="your-api-key"
rs-guard

# Using a different provider
export KIMI_API_KEY="your-api-key"
rs-guard --provider kimi

# With custom config
rs-guard --config ./my-review-config.toml

# Test configuration without blocking the commit
rs-guard --dry-run

# Force a fresh review (bypass cache)
rs-guard --no-cache
```

---

## Terminal Output

Local mode prints a color-coded summary:

```text
rs-guard Review

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

- **No staged changes?** rs-guard exits cleanly with "No staged changes to review."
- **Diff too large?** Local mode warns and exits `0` (does not block).
- **Want a custom prompt?** Use `--prompt-file` or create `.github/review-prompt.md`.
- **Need faster reviews?** Use a smaller/faster model like `deepseek-v4-flash`.
- **Progress indicators** — In local mode, rs-guard prints
  `🤖 Calling {provider} ({model})...` before the LLM call and
  `✅ Response received (N chars)` after, so you know the tool is working.
