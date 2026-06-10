This PR addresses all 13 open issues in the rs-guard project, prioritized by security, then bug, then test-coverage, and finally documentation.

## Security Issues

### #25: Remove hardcoded API key from pre-commit-hook.sh ✅
- Removed hardcoded API key from example script
- Added environment variable checks with helpful error messages
- Added optional config file support (~/.config/rs-guard/env)
- Updated LOCAL_MODE.md with secure setup instructions
- Added warning about not committing API keys

## Bug Fixes

### #19: Review body exceeding 65536-char limit ✅
- Added validation for review body length before submission in submit_review
- Added explicit error handling for 422 "body is too long" response
- Added tests for oversized review body, boundary at limit, and 422 error handling
- Updated USAGE.md to document the 65536 character limit and mitigation strategies

### #18: REPO_FULL_NAME with multiple slashes ✅
- Added validation that owner and repo parts do not contain slashes
- Added validation that owner and repo are non-empty
- Added test cases for owner/repo/subpath, /repo (empty owner), owner/ (empty repo)
- Updated error message to be clear

### #17: Metadata block near scan window boundary ✅
- Increased METADATA_SCAN_WINDOW from 1024 to 4096 bytes
- Added tests for metadata block at end of large response, near boundary, and beyond window
- Updated documentation to explain scan window behavior

### #16: Empty or whitespace-only LLM response ✅
- Added validation for empty/whitespace-only LLM responses before parsing
- Returns error with user-friendly message for such responses
- Added test cases for empty response and whitespace-only response

## Test Coverage Improvements

### #24: Add boundary test for fetch_pr_diff at exactly 100KB ✅
- Added test for exactly 100KB (should pass)
- Added test for 100KB + 1 byte (should fail)
- Added test for exactly 1500 lines (should pass)
- Added test for 1501 lines (should fail)
- Verified error messages are clear

### #23: Add test for estimate_cost_cents overflow ✅
- Added test for very large token counts (u64::MAX)
- Added test for realistically large token counts (1B tokens)
- Verified f64 handles large values gracefully (finite or infinity)

### #22: Add test for metadata block with non-standard field order ✅
- Added test for reversed field order
- Added test for fields with content in between
- Added test for random field order
- Verified all field combinations work correctly

### #21: Add test for non-UTF8 output in fetch_local_diff ✅
- Added test for non-UTF8 diff with valid markers
- Verified lossy conversion allows it to proceed
- Documented behavior in test comments

### #20: Add boundary test for chunk_diff at exactly 101 lines ✅
- Added test for 101 lines (should truncate with 50/50 params)
- Added test for 100 lines (should not truncate with 50/50 params)
- Verified truncation marker is present in truncated output
- Verified no truncation marker in non-truncated output

## Documentation Improvements

### #28: Add husky/lefthook integration example ✅
- Created examples/local-review/husky-setup.md with complete setup guide
- Included Husky setup instructions
- Included Lefthook setup instructions (alternative)
- Included troubleshooting section
- Included link to framework-specific prompts

### #27: Add framework-specific prompt templates ✅
- Created examples/prompts/react-vite.md for React/Vite projects
- Created examples/prompts/rails.md for Rails applications
- Created examples/prompts/general-code-review.md for general use
- Added README in examples/prompts/ explaining each prompt
- Updated main README.md to reference these prompts

### #26: Add framework-specific GitHub Actions workflow examples ✅
- Updated existing react-vite.yml to use standard .github/review-prompt.md
- Updated existing rails.yml to use standard .github/review-prompt.md
- Updated main README.md to reference framework-specific examples

## Verification

All changes have been verified with:
- cargo test (250 tests passed)
- cargo clippy --all-targets --all-features -- -D warnings (no warnings)
- cargo fmt --all (formatted)
- cargo deny check (all checks passed)

## Issue Status

All 13 issues have been labeled as ready-for-review:
- #25 (security) ✅
- #19 (bug) ✅
- #18 (bug) ✅
- #17 (bug) ✅
- #16 (bug) ✅
- #24 (test coverage) ✅
- #23 (test coverage) ✅
- #22 (test coverage) ✅
- #21 (test coverage) ✅
- #20 (test coverage) ✅
- #28 (documentation) ✅
- #27 (documentation) ✅
- #26 (documentation) ✅
