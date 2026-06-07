#!/bin/sh
# diffguard pre-commit hook
#
# Install:
#   cp examples/local-review/pre-commit-hook.sh .git/hooks/pre-commit
#   chmod +x .git/hooks/pre-commit
#
# This hook analyzes staged changes with diffguard and aborts the commit
# if the review returns REQUEST_CHANGES.

# Set your preferred provider and API key here, or rely on env vars.
# export DIFFGUARD_PROVIDER="deepseek"
# export DEEPSEEK_API_KEY="your-api-key"

echo "Running diffguard pre-commit review..."

diffguard
EXIT_CODE=$?

if [ "$EXIT_CODE" -eq 0 ]; then
    echo "diffguard: Review passed."
    exit 0
elif [ "$EXIT_CODE" -eq 2 ]; then
    echo "diffguard: Review returned REQUEST_CHANGES. Commit aborted."
    echo "Address the issues above or bypass with: git commit --no-verify"
    exit 1
else
    echo "diffguard: Error occurred (exit code $EXIT_CODE)."
    exit 1
fi
