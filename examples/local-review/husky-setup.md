# Husky & Lefthook Integration for rs-guard

This guide explains how to integrate rs-guard with modern git hook managers for JavaScript/TypeScript projects.

## What are Husky and Lefthook?

- **Husky**: A popular git hook manager that uses `package.json` and shell scripts.
- **Lefthook**: A fast, language-agnostic git hook manager with YAML configuration.

Both tools allow you to run rs-guard before commits, ensuring code quality without manual intervention.

---

## Option 1: Husky Setup

### Prerequisites

- Node.js project with `package.json`
- Husky installed (`npm install husky --save-dev`)

### Installation

1. **Initialize Husky** (if not already done):

```bash
npx husky init
```

This creates `.husky/` directory and configures git hooks.

2. **Create the pre-commit hook**:

```bash
cat > .husky/pre-commit << 'EOF'
#!/bin/sh
. "$(dirname "$0")/_/husky.sh"

# Load optional config file
if [ -f ~/.config/rs-guard/env ]; then
    . ~/.config/rs-guard/env
fi

# Verify API key is set
if [ -z "$DEEPSEEK_API_KEY" ] && [ -z "$KIMI_API_KEY" ] && [ -z "$DASHSCOPE_API_KEY" ] && [ -z "$OPENROUTER_API_KEY" ] && [ -z "$OPENAI_API_KEY" ]; then
    echo "rs-guard: No API key found. Skipping AI review."
    exit 0
fi

# Check if rs-guard is installed
if ! command -v rs-guard >/dev/null 2>&1; then
    echo "rs-guard: not found in PATH. Install from https://github.com/nebulaideas/rs-guard/releases"
    exit 0
fi

# Run rs-guard on staged changes
rs-guard
EOF
chmod +x .husky/pre-commit
```

3. **Commit the hook**:

```bash
git add .husky/pre-commit
git commit -m "chore: add rs-guard pre-commit hook"
```

### Troubleshooting Husky

**Hook not running?**
- Verify `package.json` has `"prepare": "husky install"` in scripts
- Check that `.git/hooks/pre-commit` exists and is executable
- Run `npx husky install` to re-initialize

**rs-guard not found?**
- Install rs-guard globally: `cargo install rs-guard`
- Or use the binary directly in your PATH

**API key errors?**
- Set environment variables in your shell profile (`.zshrc`, `.bashrc`)
- Or use the config file at `~/.config/rs-guard/env`

---

## Option 2: Lefthook Setup

### Prerequisites

- Lefthook installed (`npm install lefthook --save-dev` or `go install github.com/evilmartians/lefthook@latest`)

### Installation

1. **Initialize Lefthook**:

```bash
npx lefthook install
# or if using Go version
lefthook install
```

2. **Create `.lefthook.yml`**:

```yaml
pre-commit:
  parallel: false
  commands:
    rs-guard:
      run: |
        # Load optional config file
        if [ -f ~/.config/rs-guard/env ]; then
          . ~/.config/rs-guard/env
        fi

        # Verify API key is set
        if [ -z "$DEEPSEEK_API_KEY" ] && [ -z "$KIMI_API_KEY" ] && [ -z "$DASHSCOPE_API_KEY" ] && [ -z "$OPENROUTER_API_KEY" ] && [ -z "$OPENAI_API_KEY" ]; then
          echo "rs-guard: No API key found. Skipping AI review."
          exit 0
        fi

        # Check if rs-guard is installed
        if ! command -v rs-guard >/dev/null 2>&1; then
          echo "rs-guard: not found in PATH. Install from https://github.com/nebulaideas/rs-guard/releases"
          exit 0
        fi

        # Run rs-guard
        rs-guard
```

3. **Commit the configuration**:

```bash
git add .lefthook.yml
git commit -m "chore: add rs-guard lefthook configuration"
```

### Troubleshooting Lefthook

**Hook not running?**
- Run `lefthook install` to ensure hooks are installed
- Check that `.git/hooks/pre-commit` exists

**Configuration errors?**
- Validate YAML syntax: `lefthook run pre-commit`
- Check indentation in `.lefthook.yml` (YAML is sensitive to spaces/tabs)

**rs-guard not found?**
- Install rs-guard globally: `cargo install rs-guard`
- Or use the full path to the binary

---

## Framework-Specific Prompts

For best results, use a prompt tailored to your framework:

- **React/Vite**: See [`examples/prompts/react-vite.md`](../prompts/react-vite.md)
- **Rails**: See [`examples/prompts/rails.md`](../prompts/rails.md)
- **General**: See [`examples/prompts/general-code-review.md`](../prompts/general-code-review.md)

To use a custom prompt:

```bash
# In your hook script
rs-guard --prompt-file .github/review-prompt.md
```

---

## Bypassing the Hook

If you need to commit despite the review:

```bash
# Husky
git commit --no-verify

# Lefthook
git commit --no-verify
```

---

## Security Best Practices

⚠️ **IMPORTANT**: Never hardcode API keys in your hook scripts or commit them to git.

- Use environment variables or the `~/.config/rs-guard/env` config file
- Add config files containing secrets to your `.gitignore`
- Rotate API keys if they're accidentally committed
- Use different API keys for different projects when possible

---

## Additional Resources

- [Husky Documentation](https://typicode.github.io/husky/)
- [Lefthook Documentation](https://github.com/evilmartians/lefthook)
- [rs-guard Documentation](https://github.com/nebulaideas/rs-guard)
- [Framework-Specific Prompts](../prompts/)
