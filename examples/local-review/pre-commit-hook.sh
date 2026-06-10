#!/bin/sh
# rs-guard pre-commit hook
#
# Install:
#   cp examples/local-review/pre-commit-hook.sh .git/hooks/pre-commit
#   chmod +x .git/hooks/pre-commit
#
# This hook analyzes staged changes with rs-guard and aborts the commit
# if the review returns REQUEST_CHANGES.
#
# Tips:
#   - Test the hook without blocking: ./rs-guard --dry-run
#   - Force a fresh review: ./rs-guard --no-cache
#   - Bypass the hook: git commit --no-verify
#
# Security:
#   - NEVER hardcode API keys in this script or commit them to git
#   - Use environment variables or a config file at ~/.config/rs-guard/env
#   - Add ~/.config/rs-guard/env to your .gitignore if it contains secrets

# Load optional config file (sourced if it exists)
if [ -f ~/.config/rs-guard/env ]; then
    . ~/.config/rs-guard/env
fi

# Verify API key is set (check for common providers)
if [ -z "$DEEPSEEK_API_KEY" ] && [ -z "$KIMI_API_KEY" ] && [ -z "$DASHSCOPE_API_KEY" ] && [ -z "$OPENROUTER_API_KEY" ] && [ -z "$OPENAI_API_KEY" ]; then
    echo "rs-guard: No API key found in environment."
    echo "Set one of the following environment variables:"
    echo "  - DEEPSEEK_API_KEY (for DeepSeek)"
    echo "  - KIMI_API_KEY (for Moonshot AI)"
    echo "  - DASHSCOPE_API_KEY (for Qwen/Alibaba Cloud)"
    echo "  - OPENROUTER_API_KEY (for OpenRouter)"
    echo "  - OPENAI_API_KEY (for OpenAI)"
    echo ""
    echo "Or create ~/.config/rs-guard/env with your API key:"
    echo "  mkdir -p ~/.config/rs-guard"
    echo "  echo 'export DEEPSEEK_API_KEY=\"your-api-key\"' > ~/.config/rs-guard/env"
    echo ""
    echo "Skipping AI review (commit will proceed)."
    exit 0
fi

if ! command -v rs-guard >/dev/null 2>&1; then
    echo "rs-guard: not found in PATH. Skipping AI review."
    echo "Install from: https://github.com/nebulaideas/rs-guard/releases"
    exit 0
fi

# Skip if nothing is staged
if git diff --cached --quiet; then
    exit 0
fi

echo "Running rs-guard pre-commit review..."

rs-guard
EXIT_CODE=$?

if [ "$EXIT_CODE" -eq 0 ]; then
    echo "rs-guard: Review passed."
    exit 0
elif [ "$EXIT_CODE" -eq 2 ]; then
    echo "rs-guard: Review returned REQUEST_CHANGES. Commit aborted."
    echo "Address the issues above or bypass with: git commit --no-verify"
    exit 1
else
    echo "rs-guard: Error occurred (exit code $EXIT_CODE)."
    exit 1
fi
