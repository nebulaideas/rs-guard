//! Verdict parsing and review state determination.
//!
//! Parses structured metadata from LLM responses to determine the appropriate
//! GitHub review state (`APPROVE`, `REQUEST_CHANGES`, or `COMMENT`).
//!
//! The parser first attempts to extract a `[RS_GUARD_VERDICT_METADATA]` block
//! via substring scanning. If no metadata block is found, it falls back to
//! counting `[Critical Bug]` and `[Security]` tags in the response text.

use crate::error::RsGuardError;
use regex::Regex;
use std::sync::LazyLock;

/// Maximum bytes to scan after the metadata marker for fields.
/// Increased to 4096 to handle large LLM responses where the metadata block
/// may appear near the end. This prevents silent fallback to tag counting
/// which can produce incorrect verdicts.
const METADATA_SCAN_WINDOW: usize = 4096;

/// Marker string that identifies the verdict metadata block.
const METADATA_MARKER: &str = "[RS_GUARD_VERDICT_METADATA]";

/// Compiled regex for counting critical bug tags.
static CRITICAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[Critical Bug\]|\[Critical\]").expect("critical regex is valid")
});

/// Compiled regex for counting security issue tags.
static SECURITY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[Security\]|\[Security Issue\]").expect("security regex is valid")
});

/// GitHub Pull Request review states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewState {
    /// Approve the PR — code is ready to merge.
    Approve,
    /// Request changes — issues must be addressed before merging.
    RequestChanges,
    /// Leave a comment without approving or blocking.
    Comment,
}

impl std::fmt::Display for ReviewState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReviewState::Approve => write!(f, "APPROVE"),
            ReviewState::RequestChanges => write!(f, "REQUEST_CHANGES"),
            ReviewState::Comment => write!(f, "COMMENT"),
        }
    }
}

impl ReviewState {
    /// Returns the GitHub REST API `event` value for creating a pull request review.
    ///
    /// The GitHub REST API has a well-known asymmetry between the input and
    /// output enum names for review events:
    ///
    /// | State                    | `event` (request body) | `state` (response body) |
    /// |--------------------------|------------------------|-------------------------|
    /// | [`ReviewState::Approve`] | `"APPROVE"`            | `"APPROVED"`            |
    /// | [`ReviewState::RequestChanges`] | `"REQUEST_CHANGES"` | `"CHANGES_REQUESTED"`   |
    /// | [`ReviewState::Comment`] | `"COMMENT"`            | `"COMMENTED"`           |
    ///
    /// This function returns the **request-body** form. Use the read-side
    /// string `"CHANGES_REQUESTED"` directly when comparing against the
    /// `state` field of an existing review (e.g. in
    /// [`crate::github::dismiss_previous_reviews`]).
    ///
    /// Sending `"CHANGES_REQUESTED"` as the `event` value causes GitHub to
    /// respond with HTTP 422 and the error
    /// `Variable $event of type PullRequestReviewEvent was provided invalid value`.
    pub fn as_github_state(&self) -> &'static str {
        match self {
            ReviewState::Approve => "APPROVE",
            ReviewState::RequestChanges => "REQUEST_CHANGES",
            ReviewState::Comment => "COMMENT",
        }
    }
}

/// Parsed verdict metadata from an LLM response.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "Verdict should be used to determine a ReviewState"]
pub struct Verdict {
    /// The verdict string: `"POSITIVE"` or `"NEGATIVE"`.
    pub verdict: String,
    /// Count of critical bugs identified.
    pub critical_bugs: u32,
    /// Count of security issues identified.
    pub security_issues: u32,
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Verdict: {}, CriticalBugs: {}, SecurityIssues: {}",
            self.verdict, self.critical_bugs, self.security_issues
        )
    }
}

/// Extracts a named field value from the metadata section.
///
/// Searches for `label:` in `section`, extracts the value until end-of-line,
/// and returns the trimmed result. Fields may appear in any order.
fn extract_field<'a>(section: &'a str, label: &str) -> Option<&'a str> {
    let pos = section.find(label)?;
    let value = section[pos + label.len()..].trim_start();
    let end = value.find(['\n', '\r']).unwrap_or(value.len());
    let result = value[..end].trim();
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Attempts to extract a `[RS_GUARD_VERDICT_METADATA]` block from the response
/// using fast substring scanning instead of regex.
///
/// Returns `None` if the metadata block is not present or any field cannot be parsed.
pub fn parse_metadata_block(response: &str) -> Option<Verdict> {
    let marker_pos = response.find(METADATA_MARKER)?;
    let section_start = marker_pos + METADATA_MARKER.len();
    let section = &response[section_start..];
    // Only scan a limited window after the marker — the metadata block is small
    let scan_window = &section[..METADATA_SCAN_WINDOW.min(section.len())];

    let verdict = extract_field(scan_window, "Verdict:")?;
    // Accept both "CriticalIssues:" (new format) and "CriticalBugs:" (legacy format)
    // so that user-supplied prompt files using the old field name continue to work.
    let critical_bugs: u32 = extract_field(scan_window, "CriticalIssues:")
        .or_else(|| extract_field(scan_window, "CriticalBugs:"))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let security_issues: u32 = extract_field(scan_window, "SecurityIssues:")?
        .parse()
        .unwrap_or(0);

    Some(Verdict {
        verdict: verdict.to_string(),
        critical_bugs,
        security_issues,
    })
}

/// Fallback verdict derivation by counting `[Critical Bug]` and `[Security]` tags.
///
/// Used when the LLM response does not contain a structured metadata block.
pub fn evaluate_by_tags(response: &str) -> Verdict {
    let critical_bugs = CRITICAL_RE.find_iter(response).count() as u32;
    let security_issues = SECURITY_RE.find_iter(response).count() as u32;

    Verdict {
        verdict: if critical_bugs > 0 || security_issues > 0 {
            "NEGATIVE".to_string()
        } else {
            "POSITIVE".to_string()
        },
        critical_bugs,
        security_issues,
    }
}

/// Determines the GitHub review state from a parsed verdict.
///
/// Uses an asymmetric safety model:
/// - Pessimistic signals (`NEGATIVE`, any security issues, or >2 critical bugs)
///   always produce `REQUEST_CHANGES`.
/// - A `POSITIVE` verdict with zero issues produces `APPROVE`.
/// - A `POSITIVE` verdict with 1–2 critical bugs and no security issues
///   produces `COMMENT` (human review recommended).
pub fn determine_review_state(verdict: &Verdict) -> ReviewState {
    if verdict.verdict == "NEGATIVE" || verdict.security_issues > 0 || verdict.critical_bugs > 2 {
        ReviewState::RequestChanges
    } else if verdict.critical_bugs == 0 && verdict.security_issues == 0 {
        ReviewState::Approve
    } else {
        ReviewState::Comment
    }
}

/// Parses an LLM response into a verdict and corresponding review state.
///
/// First validates the response is not empty or whitespace-only, then attempts
/// structured metadata extraction, falls back to tag counting, validates the
/// verdict value, and computes the review state.
///
/// # Errors
///
/// Returns [`RsGuardError::VerdictParse`] if:
/// - The response is empty or whitespace-only
/// - The verdict value is neither `"POSITIVE"` nor `"NEGATIVE"`
pub fn parse_verdict(response: &str) -> Result<(Verdict, ReviewState), RsGuardError> {
    // Validate response is not empty or whitespace-only
    if response.trim().is_empty() {
        return Err(RsGuardError::VerdictParse(
            "LLM response is empty or whitespace-only. Cannot determine verdict.".to_string(),
        ));
    }

    let verdict = parse_metadata_block(response).unwrap_or_else(|| evaluate_by_tags(response));

    if verdict.verdict != "POSITIVE" && verdict.verdict != "NEGATIVE" {
        return Err(RsGuardError::VerdictParse(format!(
            "Invalid verdict value: {}. Expected POSITIVE or NEGATIVE.",
            verdict.verdict
        )));
    }

    let state = determine_review_state(&verdict);
    Ok((verdict, state))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_positive() {
        let response = "Some review text\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 0\nSecurityIssues: 0";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(verdict.verdict, "POSITIVE");
        assert_eq!(verdict.critical_bugs, 0);
        assert_eq!(verdict.security_issues, 0);
        assert_eq!(determine_review_state(&verdict), ReviewState::Approve);
    }

    #[test]
    fn test_parse_negative() {
        let response = "Some review text\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: NEGATIVE\nCriticalBugs: 0\nSecurityIssues: 0";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(
            determine_review_state(&verdict),
            ReviewState::RequestChanges
        );
    }

    #[test]
    fn test_parse_critical_gt_2() {
        let response =
            "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 5\nSecurityIssues: 0";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(
            determine_review_state(&verdict),
            ReviewState::RequestChanges
        );
    }

    #[test]
    fn test_parse_security_gt_0() {
        let response =
            "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 0\nSecurityIssues: 1";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(
            determine_review_state(&verdict),
            ReviewState::RequestChanges
        );
    }

    #[test]
    fn test_missing_metadata_fallback_to_tags() {
        let response = "Review found some issues.\n[Critical Bug] Race condition in handler\n[Security] SQL injection risk";
        let verdict = evaluate_by_tags(response);
        assert_eq!(verdict.critical_bugs, 1);
        assert_eq!(verdict.security_issues, 1);
        assert_eq!(
            determine_review_state(&verdict),
            ReviewState::RequestChanges
        );
    }

    #[test]
    fn test_clean_tag_fallback() {
        let response = "Everything looks good. No issues found.";
        let verdict = evaluate_by_tags(response);
        assert_eq!(verdict.critical_bugs, 0);
        assert_eq!(verdict.security_issues, 0);
        assert_eq!(determine_review_state(&verdict), ReviewState::Approve);
    }

    #[test]
    fn test_positive_with_minor_bugs() {
        let response =
            "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 1\nSecurityIssues: 0";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(determine_review_state(&verdict), ReviewState::Comment);
    }

    /// Regression test for the GitHub REST API `event` field values.
    ///
    /// GitHub's REST API has a request/response asymmetry for review event
    /// names: the **input** field `event` expects `REQUEST_CHANGES`, but the
    /// **output** field `state` returns `CHANGES_REQUESTED`. This test pins
    /// the request-side strings so a future refactor cannot regress to
    /// sending `CHANGES_REQUESTED` (which causes a 422 with the error
    /// `Variable $event of type PullRequestReviewEvent was provided invalid value`).
    #[test]
    fn test_as_github_state_request_body_values() {
        assert_eq!(ReviewState::Approve.as_github_state(), "APPROVE");
        assert_eq!(
            ReviewState::RequestChanges.as_github_state(),
            "REQUEST_CHANGES"
        );
        assert_eq!(ReviewState::Comment.as_github_state(), "COMMENT");
    }

    #[test]
    fn test_metadata_block_at_end_of_large_response() {
        // Create a large response with metadata at the end
        let padding = "x".repeat(3000);
        let response = format!(
            "{}\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 0\nSecurityIssues: 0",
            padding
        );
        let verdict = parse_metadata_block(&response).unwrap();
        assert_eq!(verdict.verdict, "POSITIVE");
        assert_eq!(verdict.critical_bugs, 0);
        assert_eq!(verdict.security_issues, 0);
    }

    #[test]
    fn test_metadata_block_near_boundary() {
        // Create a response where metadata is near the 4096 byte window boundary
        let padding = "x".repeat(3500);
        let response = format!(
            "{}\n[RS_GUARD_VERDICT_METADATA]\nVerdict: NEGATIVE\nCriticalBugs: 1\nSecurityIssues: 0",
            padding
        );
        let verdict = parse_metadata_block(&response).unwrap();
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(verdict.critical_bugs, 1);
        assert_eq!(verdict.security_issues, 0);
    }

    #[test]
    fn test_metadata_block_beyond_window_fallback_to_tags() {
        // Create a response where metadata fields are beyond the scan window
        // With 4096 window, we need more than 4096 chars between marker and fields
        let padding = "x".repeat(5000);
        let response = format!(
            "[RS_GUARD_VERDICT_METADATA]\n{}\nVerdict: POSITIVE\nCriticalBugs: 0\nSecurityIssues: 0",
            padding
        );
        let verdict = parse_metadata_block(&response);
        // Should return None since fields are beyond the window
        assert!(verdict.is_none());
    }

    #[test]
    fn test_empty_response_returns_error() {
        let response = "";
        let result = parse_verdict(response);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("empty or whitespace-only"));
    }

    #[test]
    fn test_whitespace_only_response_returns_error() {
        let response = "   \n\t  \n  ";
        let result = parse_verdict(response);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("empty or whitespace-only"));
    }

    #[test]
    fn test_valid_response_parses_successfully() {
        let response = "Some review text\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 0\nSecurityIssues: 0";
        let result = parse_verdict(response);
        assert!(result.is_ok());
        let (verdict, state) = result.unwrap();
        assert_eq!(verdict.verdict, "POSITIVE");
        assert_eq!(state, ReviewState::Approve);
    }

    // --- Issue #22: Metadata block with non-standard field order ---

    #[test]
    fn test_metadata_block_reversed_field_order() {
        // Fields in reverse order should still parse correctly
        let response =
            "[RS_GUARD_VERDICT_METADATA]\nSecurityIssues: 0\nCriticalBugs: 1\nVerdict: NEGATIVE";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(verdict.critical_bugs, 1);
        assert_eq!(verdict.security_issues, 0);
    }

    #[test]
    fn test_metadata_block_fields_with_content_between() {
        // Content between fields should not affect parsing
        let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nSome extra text here\nCriticalBugs: 0\nMore text\nSecurityIssues: 0";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(verdict.verdict, "POSITIVE");
        assert_eq!(verdict.critical_bugs, 0);
        assert_eq!(verdict.security_issues, 0);
    }

    #[test]
    fn test_metadata_block_random_field_order() {
        // Random field order should work
        let response =
            "[RS_GUARD_VERDICT_METADATA]\nCriticalBugs: 2\nVerdict: NEGATIVE\nSecurityIssues: 1";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(verdict.critical_bugs, 2);
        assert_eq!(verdict.security_issues, 1);
    }
}
