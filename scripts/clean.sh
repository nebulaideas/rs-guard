#!/usr/bin/env bash
# Clean build artifacts from this repo's target/ directory.
#
# Usage:
#   ./scripts/clean.sh                 # show size, prompt, then cargo clean (full)
#   ./scripts/clean.sh --debug         # only clean the debug profile (frees the most space)
#   ./scripts/clean.sh --yes           # skip the confirmation prompt
#   ./scripts/clean.sh --release-build # clean, then build --release (clean-for-release flow)
#   ./scripts/clean.sh --help          # show usage
#
# Why: target/ accumulates gigabytes of build artifacts (deps + incremental
# cache). This is purely local — GitHub Actions CI uses ephemeral runners and
# does not need cleaning. See docs/ for details.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

CLEAN_DEBUG=false
ASSUME_YES=false
BUILD_RELEASE=false

usage() {
  sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
}

while (($# > 0)); do
  case "$1" in
    --debug) CLEAN_DEBUG=true ;;
    --yes) ASSUME_YES=true ;;
    --release-build) BUILD_RELEASE=true ;;
    --help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage >&2; exit 1 ;;
  esac
  shift
done

# Resolve the target directory, honoring a custom CARGO_TARGET_DIR if set.
if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  TARGET="$CARGO_TARGET_DIR"
else
  TARGET="$REPO_ROOT/target"
fi

if [[ ! -d "$TARGET" ]]; then
  echo "No target directory found at: $TARGET"
  echo "Nothing to clean."
  exit 0
fi

# Du the target directory (e.g. "47G", "2.0G").
TARGET_SIZE="$(du -sh "$TARGET" | awk '{print $1}')"
echo "Current build artifact size: $TARGET_SIZE ($TARGET)"

if [[ "$CLEAN_DEBUG" == true ]]; then
  echo "Plan: clean the debug profile only (frees the most space; keeps release)."
  CLEAN_ARGS=(--profile dev)
else
  echo "Plan: full clean of all build artifacts (frees everything)."
  CLEAN_ARGS=()
fi

if [[ "$BUILD_RELEASE" == true ]]; then
  echo "Plan: rebuild release binary after cleaning (clean-for-release flow)."
fi

if [[ "$ASSUME_YES" != true ]]; then
  read -r -p "Proceed? [y/N] " answer
  if [[ "${answer,,}" != "y" && "${answer,,}" != "yes" ]]; then
    echo "Aborted."
    exit 1
  fi
fi

echo "==> Cleaning..."
(cd "$REPO_ROOT" && cargo clean "${CLEAN_ARGS[@]}")

if [[ "$BUILD_RELEASE" == true ]]; then
  echo "==> Building release binary..."
  (cd "$REPO_ROOT" && cargo build --release)
fi

echo "Done. Freed $TARGET_SIZE — quick 'du -sh .' to confirm the new total."