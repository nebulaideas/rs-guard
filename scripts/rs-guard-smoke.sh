#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ -z "${DEEPSEEK_API_KEY:-}" ]]; then
  echo "DEEPSEEK_API_KEY is required to run the rs-guard dry-run integration test." >&2
  exit 1
fi

cleanup() {
  rm -rf "${RS_GUARD_INSTALL_DIR:-}"
  rm -f "${OUTPUT_FILE:-}"
}

echo "==> Installing rs-guard from crates.io"
OUTPUT_FILE="$(mktemp)"
trap cleanup EXIT

cargo install rs-guard --locked --version "1.8.0"
DRY_RUN_BIN="rs-guard"

# Fallback for local development if rs-guard is not on PATH.
if ! command -v rs-guard &>/dev/null; then
  if [[ -x "$HOME/.cargo/bin/rs-guard" ]]; then
    DRY_RUN_BIN="$HOME/.cargo/bin/rs-guard"
  else
    echo "rs-guard not found. Install with: cargo install rs-guard --locked" >&2
    exit 1
  fi
fi

echo "==> Verifying rs-guard config files"
test -f "$REPO_ROOT/.reviewer.toml"
test -f "$REPO_ROOT/.github/review-prompt.md"

echo "==> Running dry-run integration test against fixture diff"
FIXTURE_DIFF="$SCRIPT_DIR/fixtures/rs-guard-sample.diff"
test -f "$FIXTURE_DIFF"

# Unset CI env vars so rs-guard runs in file/local mode (not PR submission mode).
# Required when smoke runs inside GitHub Actions, where GITHUB_ACTIONS is auto-set.
env -u GITHUB_ACTIONS -u GITHUB_TOKEN -u PR_NUMBER -u REPO_FULL_NAME \
  "$DRY_RUN_BIN" \
  --diff-file "$FIXTURE_DIFF" \
  --prompt-file "$REPO_ROOT/.github/review-prompt.md" \
  --dry-run | tee "$OUTPUT_FILE"

grep -q "DRY RUN" "$OUTPUT_FILE" || {
  echo "Expected dry-run marker missing from rs-guard output." >&2
  exit 1
}

grep -q "Verdict:" "$OUTPUT_FILE" || {
  echo "Expected verdict summary missing from rs-guard output." >&2
  exit 1
}

grep -qE "RS_GUARD_VERDICT_METADATA|State:" "$OUTPUT_FILE" || {
  echo "Expected review metadata missing from rs-guard output." >&2
  exit 1
}

echo "Dry-run integration test passed."
echo "rs-guard smoke test passed."
