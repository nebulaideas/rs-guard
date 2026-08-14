//! Verdict parsing and review state determination.
//!
//! Parses structured metadata from LLM responses to determine the appropriate
//! GitHub review state (`APPROVE`, `REQUEST_CHANGES`, or `COMMENT`).
//!
//! The parser first attempts to extract a `[RS_GUARD_VERDICT_METADATA]` block
//! via substring scanning. If no metadata block is found, it falls back to
//! counting `[Critical]`, `[Security]`, `[Important]`, and `[Suggestion]` tags
//! in the response text.

use crate::error::RsGuardError;
use regex::Regex;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;

/// Maximum bytes to scan after the metadata marker for fields.
/// Increased to 4096 to handle large LLM responses where the metadata block
/// may appear near the end. This prevents silent fallback to tag counting
/// which can produce incorrect verdicts.
const METADATA_SCAN_WINDOW: usize = 4096;

/// Marker string that identifies the verdict metadata block.
const METADATA_MARKER: &str = "[RS_GUARD_VERDICT_METADATA]";

/// Marker string that identifies the structured findings JSON block.
///
/// When present at the end of an LLM response, the JSON array following this
/// marker is parsed into [`Finding`] values with file-level precision.
const FINDINGS_MARKER: &str = "[RS_GUARD_VERDICT_FINDINGS]";

/// Ensures the CriticalBugs deprecation warning is emitted at most once per process.
static CRITICAL_BUGS_WARNED: AtomicBool = AtomicBool::new(false);

/// Compiled regex for counting critical bug tags.
static CRITICAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[Critical Bug\]|\[Critical\]").expect("critical regex is valid")
});

/// Compiled regex for counting security issue tags.
static SECURITY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[Security\]|\[Security Issue\]").expect("security regex is valid")
});

/// Compiled regex for counting important issue tags.
/// Matches `[Important]` and `[Important Issue]` for consistency with critical/security variants.
static IMPORTANT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[Important\]|\[Important Issue\]").expect("important regex is valid")
});

/// Compiled regex for counting suggestion tags.
/// Matches `[Suggestion]` and `[Suggestion Issue]` for consistency with critical/security variants.
static SUGGESTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[Suggestion\]|\[Suggestion Issue\]").expect("suggestion regex is valid")
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

/// Severity level for a structured finding.
///
/// Maps to the existing tag names used in the metadata block and tag-counting
/// fallback. Variants are ordered from most to least severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum FindingSeverity {
    /// Critical issue — blocks merge unconditionally.
    Critical,
    /// Security issue — blocks merge unconditionally.
    Security,
    /// Important issue — blocks merge when count ≥ threshold.
    Important,
    /// Suggestion — advisory only, never blocks merge.
    Suggestion,
}

impl FindingSeverity {
    /// Returns the string representation used in display and serialization.
    pub fn as_str(&self) -> &'static str {
        match self {
            FindingSeverity::Critical => "Critical",
            FindingSeverity::Security => "Security",
            FindingSeverity::Important => "Important",
            FindingSeverity::Suggestion => "Suggestion",
        }
    }
}

impl std::fmt::Display for FindingSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A structured finding from an LLM review response.
///
/// Each finding pinpoints a specific issue in the reviewed code with file path,
/// line number, severity, and an actionable message. Findings are parsed from
/// the `[RS_GUARD_VERDICT_FINDINGS]` JSON block at the end of an LLM response.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Finding {
    /// File path relative to the repository root (e.g. `"src/main.rs"`).
    pub path: String,
    /// 1-based line number in the file where the issue occurs.
    pub line: u32,
    /// Severity classification of this finding.
    pub severity: FindingSeverity,
    /// Human-readable description of the issue.
    pub message: String,
    /// Optional actionable suggestion for fixing the issue.
    #[serde(default)]
    pub suggestion: Option<String>,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {}:{} — {}",
            self.severity, self.path, self.line, self.message
        )?;
        if let Some(ref s) = self.suggestion {
            write!(f, " (suggestion: {})", s)?;
        }
        Ok(())
    }
}

/// Parsed verdict metadata from an LLM response.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "Verdict should be used to determine a ReviewState"]
pub struct Verdict {
    /// The verdict string: `"POSITIVE"` or `"NEGATIVE"`.
    pub verdict: String,
    /// Count of `[Critical]` issues identified. Blocks merge unconditionally.
    pub critical_issues: u32,
    /// Count of `[Security]` issues identified. Blocks merge unconditionally.
    pub security_issues: u32,
    /// Count of `[Important]` issues identified. Blocks merge when ≥ 3.
    pub important_issues: u32,
    /// Count of `[Suggestion]` items. Advisory only — never blocks merge.
    pub suggestions: u32,
    /// Structured findings parsed from `[RS_GUARD_VERDICT_FINDINGS]`.
    ///
    /// When non-empty, the counts above are derived from these findings.
    /// When empty, counts come from the metadata block or tag counting fallback.
    pub findings: Vec<Finding>,
}

impl Verdict {
    /// Creates a `Verdict` with counts derived from the given findings.
    ///
    /// The `fallback_verdict` string is preserved when no blocking findings are
    /// present (i.e. no `Critical` or `Security`). When any blocking findings
    /// are present, the verdict string is forced to `"NEGATIVE"` to guarantee
    /// internal consistency with the derived counts and prevent callers from
    /// observing a `"POSITIVE"` verdict alongside blocking findings.
    pub fn from_findings(fallback_verdict: &str, findings: Vec<Finding>) -> Self {
        let mut critical_issues = 0u32;
        let mut security_issues = 0u32;
        let mut important_issues = 0u32;
        let mut suggestions = 0u32;
        for finding in &findings {
            match finding.severity {
                FindingSeverity::Critical => critical_issues += 1,
                FindingSeverity::Security => security_issues += 1,
                FindingSeverity::Important => important_issues += 1,
                FindingSeverity::Suggestion => suggestions += 1,
            }
        }

        let verdict = if critical_issues > 0 || security_issues > 0 {
            "NEGATIVE".to_string()
        } else {
            fallback_verdict.to_string()
        };

        Verdict {
            verdict,
            critical_issues,
            security_issues,
            important_issues,
            suggestions,
            findings,
        }
    }

    /// Returns `true` when structured findings are available.
    pub fn has_findings(&self) -> bool {
        !self.findings.is_empty()
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Verdict: {}, CriticalIssues: {}, SecurityIssues: {}, ImportantIssues: {}, Suggestions: {}",
            self.verdict,
            self.critical_issues,
            self.security_issues,
            self.important_issues,
            self.suggestions
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
/// Returns `None` if the metadata block is not present or the `Verdict:` field is missing.
/// All numeric fields (`CriticalIssues`, `SecurityIssues`, `ImportantIssues`, `Suggestions`)
/// default to `0` when absent, allowing partial blocks from older prompt formats to parse
/// successfully. This relaxed policy is intentional — a missing count is treated as zero
/// rather than a parse failure.
pub fn parse_metadata_block(response: &str) -> Option<Verdict> {
    let marker_pos = response.find(METADATA_MARKER)?;
    let section_start = marker_pos + METADATA_MARKER.len();
    let section = &response[section_start..];
    // Only scan a limited window after the marker — the metadata block is small
    let scan_window = &section[..METADATA_SCAN_WINDOW.min(section.len())];

    let verdict = extract_field(scan_window, "Verdict:")?.to_string();
    // Accept both "CriticalIssues:" (current) and "CriticalBugs:" (legacy alias).
    // Prefer CriticalIssues when both are present.
    let critical_from_issues = extract_field(scan_window, "CriticalIssues:");
    let critical_from_bugs = extract_field(scan_window, "CriticalBugs:");
    if critical_from_bugs.is_some() {
        // Log at most once per process to avoid spam if many responses are parsed.
        if !CRITICAL_BUGS_WARNED.swap(true, Ordering::Relaxed) {
            if critical_from_issues.is_some() {
                log::warn!(
                    "Deprecated metadata field 'CriticalBugs:' detected alongside                      'CriticalIssues:'; 'CriticalBugs:' is ignored. Prefer                      'CriticalIssues:' only. CriticalBugs will be removed in a                      future major release."
                );
            } else {
                log::warn!(
                    "Deprecated metadata field 'CriticalBugs:' detected; use                      'CriticalIssues:' instead. CriticalBugs will be removed in                      a future major release."
                );
            }
        }
    }
    let critical_issues: u32 = critical_from_issues
        .or(critical_from_bugs)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let security_issues: u32 = extract_field(scan_window, "SecurityIssues:")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let important_issues: u32 = extract_field(scan_window, "ImportantIssues:")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let suggestions: u32 = extract_field(scan_window, "Suggestions:")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    Some(Verdict {
        verdict,
        critical_issues,
        security_issues,
        important_issues,
        suggestions,
        findings: Vec::new(),
    })
}

/// Fallback verdict derivation by counting severity tags in the response text.
///
/// Used when the LLM response does not contain a structured metadata block.
/// Counts `[Critical]` / `[Critical Bug]`, `[Security]` / `[Security Issue]`,
/// `[Important]` / `[Important Issue]`, and `[Suggestion]` / `[Suggestion Issue]` tags.
pub fn evaluate_by_tags(response: &str) -> Verdict {
    let critical_issues = CRITICAL_RE.find_iter(response).count() as u32;
    let security_issues = SECURITY_RE.find_iter(response).count() as u32;
    let important_issues = IMPORTANT_RE.find_iter(response).count() as u32;
    let suggestions = SUGGESTION_RE.find_iter(response).count() as u32;

    Verdict {
        verdict: if critical_issues > 0 || security_issues > 0 {
            "NEGATIVE".to_string()
        } else {
            "POSITIVE".to_string()
        },
        critical_issues,
        security_issues,
        important_issues,
        suggestions,
        findings: Vec::new(),
    }
}

/// Determines the GitHub review state from a parsed verdict.
///
/// Uses an asymmetric safety model:
/// - `NEGATIVE` verdict, any `[Security]` issues, or any `[Critical]` issues → `REQUEST_CHANGES`.
/// - `[Important]` issues ≥ `important_threshold` → `REQUEST_CHANGES`.
/// - `[Important]` issues 1..`important_threshold` → `COMMENT` (human review recommended).
/// - All counts zero and verdict `POSITIVE` → `APPROVE`.
pub fn determine_review_state(verdict: &Verdict, important_threshold: u32) -> ReviewState {
    if verdict.verdict == "NEGATIVE"
        || verdict.security_issues > 0
        || verdict.critical_issues > 0
        || (important_threshold > 0 && verdict.important_issues >= important_threshold)
    {
        ReviewState::RequestChanges
    } else if verdict.important_issues > 0 {
        ReviewState::Comment
    } else {
        ReviewState::Approve
    }
}

/// Parses an LLM response into a verdict and corresponding review state.
///
/// First validates the response is not empty or whitespace-only, then:
/// 1. Extracts the metadata block to get the verdict string and counts.
///    Falls back to tag counting if no metadata block is present.
/// 2. If a `[RS_GUARD_VERDICT_FINDINGS]` marker is present, parses the
///    findings JSON and overrides the counts derived from the findings.
///    A malformed findings block is treated as a `VerdictParse` error so
///    silently dropping attempted findings is not possible.
/// 3. Validates the verdict value and computes the review state.
///
/// When structured findings are present, issue counts are derived from them
/// (overriding any metadata block counts). This ensures the counts always
/// match the detailed findings.
///
/// # Errors
///
/// Returns [`RsGuardError::VerdictParse`] if:
/// - The response is empty or whitespace-only
/// - The verdict value is neither `"POSITIVE"` nor `"NEGATIVE"`
/// - A `[RS_GUARD_VERDICT_FINDINGS]` marker is present but the JSON is malformed
pub fn parse_verdict(
    response: &str,
    important_threshold: u32,
) -> Result<(Verdict, ReviewState), RsGuardError> {
    // Validate response is not empty or whitespace-only
    if response.trim().is_empty() {
        return Err(RsGuardError::VerdictParse(
            "LLM response is empty or whitespace-only. Cannot determine verdict.".to_string(),
        ));
    }

    // Step 1: extract the metadata block to get the verdict string and counts.
    let mut verdict = parse_metadata_block(response).unwrap_or_else(|| evaluate_by_tags(response));

    // Step 2: if a findings block is present, override counts from findings.
    // Marker absent → Ok(None) → no override. Marker present but malformed → Err.
    if let Some(findings) = parse_findings(response)? {
        if !findings.is_empty() {
            verdict = Verdict::from_findings(&verdict.verdict, findings);
        }
    }

    if verdict.verdict != "POSITIVE" && verdict.verdict != "NEGATIVE" {
        return Err(RsGuardError::VerdictParse(format!(
            "Invalid verdict value: {}. Expected POSITIVE or NEGATIVE.",
            verdict.verdict
        )));
    }

    let state = determine_review_state(&verdict, important_threshold);
    Ok((verdict, state))
}

/// Parses structured findings from a `[RS_GUARD_VERDICT_FINDINGS]` JSON block.
///
/// Looks for the `[RS_GUARD_VERDICT_FINDINGS]` marker at the end of the LLM
/// response. If found, extracts and deserializes the JSON array that follows it.
///
/// Returns `Ok(None)` if the marker is absent. Returns `Err(VerdictParse)`
/// if the marker is present but the JSON is malformed, since silently dropping
/// explicitly-attempted findings is a security-sensitive failure mode.
///
/// # Arguments
///
/// * `response` — The full LLM response text.
pub fn parse_findings(response: &str) -> Result<Option<Vec<Finding>>, RsGuardError> {
    let Some(marker_pos) = response.find(FINDINGS_MARKER) else {
        return Ok(None);
    };
    let json_start = marker_pos + FINDINGS_MARKER.len();
    let json_text = response[json_start..].trim();

    // The JSON must start with '[' to be a valid findings array.
    if !json_text.starts_with('[') {
        return Err(RsGuardError::VerdictParse(format!(
            "Found '{}' marker but the following content is not a JSON array. \
             Expected a JSON array of Finding objects.",
            FINDINGS_MARKER
        )));
    }

    serde_json::from_str::<Vec<Finding>>(json_text)
        .map(Some)
        .map_err(|e| {
            RsGuardError::VerdictParse(format!(
                "Found '{}' marker but the JSON is malformed: {}. \
             A malformed findings block is treated as an error to prevent \
             silent approval when the model attempted to report findings.",
                FINDINGS_MARKER, e
            ))
        })
}

/// Strips the `[RS_GUARD_VERDICT_FINDINGS]` JSON block from the response.
///
/// Returns the response text with the findings marker and its trailing JSON
/// removed. This is used to produce a clean prose body for the GitHub review
/// comment. If the marker is not present, the original response is returned
/// unchanged.
///
/// # Arguments
///
/// * `response` — The full LLM response text.
pub fn strip_findings_json(response: &str) -> &str {
    if let Some(marker_pos) = response.find(FINDINGS_MARKER) {
        response[..marker_pos].trim_end()
    } else {
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_positive() {
        let response = "Some review text\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(verdict.verdict, "POSITIVE");
        assert_eq!(verdict.critical_issues, 0);
        assert_eq!(verdict.security_issues, 0);
        assert_eq!(verdict.important_issues, 0);
        assert_eq!(verdict.suggestions, 0);
        assert_eq!(determine_review_state(&verdict, 3), ReviewState::Approve);
    }

    #[test]
    fn test_parse_negative() {
        let response = "Some review text\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: NEGATIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(
            determine_review_state(&verdict, 3),
            ReviewState::RequestChanges
        );
    }

    #[test]
    fn test_parse_critical_gt_0() {
        let response =
            "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 1\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(
            determine_review_state(&verdict, 3),
            ReviewState::RequestChanges
        );
    }

    #[test]
    fn test_parse_security_gt_0() {
        let response =
            "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 1\nImportantIssues: 0\nSuggestions: 0";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(
            determine_review_state(&verdict, 3),
            ReviewState::RequestChanges
        );
    }

    #[test]
    fn test_missing_metadata_fallback_to_tags() {
        let response = "Review found some issues.\n[Critical Bug] Race condition in handler\n[Security] SQL injection risk";
        let verdict = evaluate_by_tags(response);
        assert_eq!(verdict.critical_issues, 1);
        assert_eq!(verdict.security_issues, 1);
        assert_eq!(
            determine_review_state(&verdict, 3),
            ReviewState::RequestChanges
        );
    }

    #[test]
    fn test_clean_tag_fallback() {
        let response = "Everything looks good. No issues found.";
        let verdict = evaluate_by_tags(response);
        assert_eq!(verdict.critical_issues, 0);
        assert_eq!(verdict.security_issues, 0);
        assert_eq!(verdict.important_issues, 0);
        assert_eq!(verdict.suggestions, 0);
        assert_eq!(determine_review_state(&verdict, 3), ReviewState::Approve);
    }

    #[test]
    fn test_positive_with_important_issues_comment() {
        let response =
            "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 1\nSuggestions: 0";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(determine_review_state(&verdict, 3), ReviewState::Comment);
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
        let padding = "x".repeat(3000);
        let response = format!(
            "{}\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0",
            padding
        );
        let verdict = parse_metadata_block(&response).unwrap();
        assert_eq!(verdict.verdict, "POSITIVE");
        assert_eq!(verdict.critical_issues, 0);
        assert_eq!(verdict.security_issues, 0);
    }

    #[test]
    fn test_metadata_block_near_boundary() {
        let padding = "x".repeat(3500);
        let response = format!(
            "{}\n[RS_GUARD_VERDICT_METADATA]\nVerdict: NEGATIVE\nCriticalIssues: 1\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0",
            padding
        );
        let verdict = parse_metadata_block(&response).unwrap();
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(verdict.critical_issues, 1);
        assert_eq!(verdict.security_issues, 0);
    }

    #[test]
    fn test_metadata_block_beyond_window_fallback_to_tags() {
        let padding = "x".repeat(5000);
        let response = format!(
            "[RS_GUARD_VERDICT_METADATA]\n{}\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0",
            padding
        );
        let verdict = parse_metadata_block(&response);
        assert!(verdict.is_none());
    }

    #[test]
    fn test_empty_response_returns_error() {
        let response = "";
        let result = parse_verdict(response, 3);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("empty or whitespace-only"));
    }

    #[test]
    fn test_whitespace_only_response_returns_error() {
        let response = "   \n\t  \n  ";
        let result = parse_verdict(response, 3);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("empty or whitespace-only"));
    }

    #[test]
    fn test_valid_response_parses_successfully() {
        let response = "Some review text\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";
        let result = parse_verdict(response, 3);
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
            "[RS_GUARD_VERDICT_METADATA]\nSuggestions: 1\nImportantIssues: 0\nSecurityIssues: 0\nCriticalIssues: 1\nVerdict: NEGATIVE";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(verdict.critical_issues, 1);
        assert_eq!(verdict.security_issues, 0);
        assert_eq!(verdict.suggestions, 1);
    }

    #[test]
    fn test_metadata_block_fields_with_content_between() {
        // Content between fields should not affect parsing
        let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nSome extra text here\nCriticalIssues: 0\nMore text\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(verdict.verdict, "POSITIVE");
        assert_eq!(verdict.critical_issues, 0);
        assert_eq!(verdict.security_issues, 0);
    }

    #[test]
    fn test_metadata_block_random_field_order() {
        // Random field order should work
        let response =
            "[RS_GUARD_VERDICT_METADATA]\nImportantIssues: 2\nCriticalIssues: 1\nVerdict: NEGATIVE\nSecurityIssues: 0\nSuggestions: 3";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(verdict.critical_issues, 1);
        assert_eq!(verdict.security_issues, 0);
        assert_eq!(verdict.important_issues, 2);
        assert_eq!(verdict.suggestions, 3);
    }

    #[test]
    fn test_legacy_critical_issues_field_still_parses() {
        // Legacy CriticalBugs: alias still parses (deprecated; prefer CriticalIssues:)
        let response =
            "[RS_GUARD_VERDICT_METADATA]\nVerdict: NEGATIVE\nCriticalBugs: 2\nSecurityIssues: 1";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(verdict.critical_issues, 2);
        assert_eq!(verdict.security_issues, 1);
        assert_eq!(verdict.important_issues, 0);
        assert_eq!(verdict.suggestions, 0);
    }

    #[test]
    fn test_invalid_verdict_value_in_metadata_block() {
        let response =
            "[RS_GUARD_VERDICT_METADATA]\nVerdict: MAYBE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";
        let result = parse_verdict(response, 3);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid verdict value"),
            "expected invalid verdict error"
        );
    }

    #[test]
    fn test_parse_three_important_issues_request_changes() {
        // Three important issues is the exact threshold for REQUEST_CHANGES.
        let response =
            "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 3\nSuggestions: 0";
        let (verdict, state) = parse_verdict(response, 3).unwrap();
        assert_eq!(verdict.important_issues, 3);
        assert_eq!(state, ReviewState::RequestChanges);
    }

    #[test]
    fn test_evaluate_by_tags_counts_important_issue_variant() {
        let response =
            "Review complete.\n[Important Issue] Missing test\n[Important Issue] Poor naming";
        let verdict = evaluate_by_tags(response);
        assert_eq!(verdict.important_issues, 2);
        assert_eq!(verdict.verdict, "POSITIVE");
        assert_eq!(determine_review_state(&verdict, 3), ReviewState::Comment);
    }

    #[test]
    fn test_evaluate_by_tags_counts_suggestion_issue_variant() {
        let response =
            "Review complete.\n[Suggestion Issue] Use a constant\n[Suggestion] Add doc comment";
        let verdict = evaluate_by_tags(response);
        assert_eq!(verdict.suggestions, 2);
        assert_eq!(verdict.verdict, "POSITIVE");
        assert_eq!(determine_review_state(&verdict, 3), ReviewState::Approve);
    }

    // ── Finding schema tests ──────────────────────────────────────────────

    #[test]
    fn test_parse_findings_valid_single() {
        let response = r#"Review complete.

[RS_GUARD_VERDICT_FINDINGS]
[{"path":"src/main.rs","line":42,"severity":"Critical","message":"Null pointer dereference"}]"#;
        let findings = parse_findings(response)
            .expect("should parse")
            .expect("should be Some");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].path, "src/main.rs");
        assert_eq!(findings[0].line, 42);
        assert_eq!(findings[0].severity, FindingSeverity::Critical);
        assert_eq!(findings[0].message, "Null pointer dereference");
        assert_eq!(findings[0].suggestion, None);
    }

    #[test]
    fn test_parse_findings_multiple_with_suggestion() {
        let response = r#"Some prose here.

[RS_GUARD_VERDICT_FINDINGS]
[
  {"path":"src/lib.rs","line":10,"severity":"Security","message":"SQL injection risk","suggestion":"Use parameterized queries"},
  {"path":"src/handler.rs","line":55,"severity":"Important","message":"Missing error handling"},
  {"path":"src/util.rs","line":3,"severity":"Suggestion","message":"Consider using a constant","suggestion":"Extract to a const"}
]"#;
        let findings = parse_findings(response)
            .expect("should parse")
            .expect("should be Some");
        assert_eq!(findings.len(), 3);
        assert_eq!(findings[0].severity, FindingSeverity::Security);
        assert_eq!(
            findings[0].suggestion.as_deref(),
            Some("Use parameterized queries")
        );
        assert_eq!(findings[1].severity, FindingSeverity::Important);
        assert_eq!(findings[1].suggestion, None);
        assert_eq!(findings[2].severity, FindingSeverity::Suggestion);
        assert_eq!(
            findings[2].suggestion.as_deref(),
            Some("Extract to a const")
        );
    }

    #[test]
    fn test_parse_findings_no_marker_returns_none() {
        let response = "Review complete. No issues found.";
        assert!(parse_findings(response).unwrap().is_none());
    }

    #[test]
    fn test_parse_findings_invalid_json_returns_err() {
        // Marker is present and content starts with '[' but is malformed JSON
        // → must be an error, not a silent drop (security-sensitive).
        let response = "[RS_GUARD_VERDICT_FINDINGS]\n[{\"path\":\"a.rs\"}]";
        let result = parse_findings(response);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("malformed"),
            "error should explain JSON is malformed: {msg}"
        );
    }

    #[test]
    fn test_parse_findings_non_array_after_marker_returns_err() {
        // Marker is present but content is not a JSON array → must error.
        let response = "[RS_GUARD_VERDICT_FINDINGS]\n{\"foo\":\"bar\"}";
        let result = parse_findings(response);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not a JSON array"),
            "error should explain non-array content: {msg}"
        );
    }

    #[test]
    fn test_parse_findings_empty_array() {
        let response = "[RS_GUARD_VERDICT_FINDINGS]\n[]";
        let findings = parse_findings(response)
            .expect("should parse empty array")
            .expect("should be Some");
        assert!(findings.is_empty());
    }

    #[test]
    fn test_parse_findings_missing_required_field_returns_err() {
        let response = r#"[RS_GUARD_VERDICT_FINDINGS]
[{"path":"src/main.rs","severity":"Critical","message":"Missing line field"}]"#;
        let result = parse_findings(response);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_findings_unknown_severity_returns_err() {
        let response = r#"[RS_GUARD_VERDICT_FINDINGS]
[{"path":"src/main.rs","line":1,"severity":"Unknown","message":"bad severity"}]"#;
        let result = parse_findings(response);
        assert!(result.is_err());
    }

    #[test]
    fn test_strip_findings_json_removes_block() {
        let response = "Here is the review prose.\n\nMore details.\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{\"path\":\"a.rs\",\"line\":1,\"severity\":\"Critical\",\"message\":\"test\"}]";
        let stripped = strip_findings_json(response);
        assert!(!stripped.contains("[RS_GUARD_VERDICT_FINDINGS]"));
        assert!(!stripped.contains("\"path\""));
        assert!(stripped.contains("Here is the review prose."));
    }

    #[test]
    fn test_strip_findings_json_no_marker_returns_original() {
        let response = "Just a normal review response.";
        assert_eq!(strip_findings_json(response), response);
    }

    #[test]
    fn test_finding_display_without_suggestion() {
        let finding = Finding {
            path: "src/main.rs".to_string(),
            line: 42,
            severity: FindingSeverity::Critical,
            message: "Null pointer".to_string(),
            suggestion: None,
        };
        assert_eq!(
            format!("{finding}"),
            "[Critical] src/main.rs:42 — Null pointer"
        );
    }

    #[test]
    fn test_finding_display_with_suggestion() {
        let finding = Finding {
            path: "src/lib.rs".to_string(),
            line: 10,
            severity: FindingSeverity::Suggestion,
            message: "Use a constant".to_string(),
            suggestion: Some("Extract to MAGIC_NUMBER".to_string()),
        };
        assert_eq!(
            format!("{finding}"),
            "[Suggestion] src/lib.rs:10 — Use a constant (suggestion: Extract to MAGIC_NUMBER)"
        );
    }

    #[test]
    fn test_finding_severity_display() {
        assert_eq!(format!("{}", FindingSeverity::Critical), "Critical");
        assert_eq!(format!("{}", FindingSeverity::Security), "Security");
        assert_eq!(format!("{}", FindingSeverity::Important), "Important");
        assert_eq!(format!("{}", FindingSeverity::Suggestion), "Suggestion");
    }

    #[test]
    fn test_finding_severity_as_str() {
        assert_eq!(FindingSeverity::Critical.as_str(), "Critical");
        assert_eq!(FindingSeverity::Security.as_str(), "Security");
        assert_eq!(FindingSeverity::Important.as_str(), "Important");
        assert_eq!(FindingSeverity::Suggestion.as_str(), "Suggestion");
    }

    #[test]
    fn test_parse_findings_marker_with_leading_whitespace_in_json() {
        let response = "[RS_GUARD_VERDICT_FINDINGS]\n  \n  [{\"path\":\"a.rs\",\"line\":1,\"severity\":\"Suggestion\",\"message\":\"ok\"}]";
        let findings = parse_findings(response)
            .expect("should parse with whitespace")
            .expect("should be Some");
        assert_eq!(findings.len(), 1);
    }

    // ── Verdict findings integration tests ────────────────────────────────

    #[test]
    fn test_verdict_from_findings_derives_counts() {
        let findings = vec![
            Finding {
                path: "a.rs".into(),
                line: 1,
                severity: FindingSeverity::Critical,
                message: "crit".into(),
                suggestion: None,
            },
            Finding {
                path: "b.rs".into(),
                line: 2,
                severity: FindingSeverity::Security,
                message: "sec".into(),
                suggestion: None,
            },
            Finding {
                path: "c.rs".into(),
                line: 3,
                severity: FindingSeverity::Important,
                message: "imp".into(),
                suggestion: None,
            },
            Finding {
                path: "d.rs".into(),
                line: 4,
                severity: FindingSeverity::Suggestion,
                message: "sug".into(),
                suggestion: None,
            },
        ];
        // Caller passes POSITIVE, but blocking findings must force NEGATIVE.
        let verdict = Verdict::from_findings("POSITIVE", findings);
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(verdict.critical_issues, 1);
        assert_eq!(verdict.security_issues, 1);
        assert_eq!(verdict.important_issues, 1);
        assert_eq!(verdict.suggestions, 1);
        assert!(verdict.has_findings());
    }

    #[test]
    fn test_verdict_from_findings_empty() {
        // Empty findings + fallback verdict is preserved unchanged.
        let verdict = Verdict::from_findings("POSITIVE", vec![]);
        assert_eq!(verdict.verdict, "POSITIVE");
        assert_eq!(verdict.critical_issues, 0);
        assert!(!verdict.has_findings());
    }

    #[test]
    fn test_verdict_from_findings_preserves_fallback_when_no_blocking() {
        // No Critical/Security findings → fallback verdict string is preserved
        // even if the caller passed NEGATIVE.
        let findings = vec![
            Finding {
                path: "a.rs".into(),
                line: 1,
                severity: FindingSeverity::Important,
                message: "imp".into(),
                suggestion: None,
            },
            Finding {
                path: "b.rs".into(),
                line: 2,
                severity: FindingSeverity::Suggestion,
                message: "sug".into(),
                suggestion: None,
            },
        ];
        let verdict = Verdict::from_findings("NEGATIVE", findings);
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(verdict.important_issues, 1);
        assert_eq!(verdict.suggestions, 1);
        assert_eq!(verdict.critical_issues, 0);
        assert_eq!(verdict.security_issues, 0);
    }

    #[test]
    fn test_verdict_from_findings_forces_negative_on_critical() {
        // Even with fallback "POSITIVE", a single Critical finding must
        // produce a NEGATIVE verdict string.
        let findings = vec![Finding {
            path: "a.rs".into(),
            line: 1,
            severity: FindingSeverity::Critical,
            message: "crash".into(),
            suggestion: None,
        }];
        let verdict = Verdict::from_findings("POSITIVE", findings);
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(verdict.critical_issues, 1);
    }

    #[test]
    fn test_verdict_from_findings_forces_negative_on_security() {
        // Even with fallback "POSITIVE", a single Security finding must
        // produce a NEGATIVE verdict string.
        let findings = vec![Finding {
            path: "a.rs".into(),
            line: 1,
            severity: FindingSeverity::Security,
            message: "sqli".into(),
            suggestion: None,
        }];
        let verdict = Verdict::from_findings("POSITIVE", findings);
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(verdict.security_issues, 1);
    }

    #[test]
    fn test_parse_verdict_with_findings_overrides_metadata_counts() {
        // Metadata says 0 critical, but findings say 2 critical
        let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{\"path\":\"a.rs\",\"line\":1,\"severity\":\"Critical\",\"message\":\"c1\"},{\"path\":\"b.rs\",\"line\":2,\"severity\":\"Critical\",\"message\":\"c2\"}]";
        let (verdict, state) = parse_verdict(response, 3).unwrap();
        assert_eq!(verdict.critical_issues, 2);
        assert_eq!(verdict.findings.len(), 2);
        // Verdict string must also flip to NEGATIVE for internal consistency.
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(state, ReviewState::RequestChanges);
    }

    #[test]
    fn test_parse_verdict_with_findings_positive_to_request_changes() {
        // Metadata says POSITIVE with 0 issues, but findings have Security
        let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{\"path\":\"a.rs\",\"line\":10,\"severity\":\"Security\",\"message\":\"SQL injection\",\"suggestion\":\"Use parameterized queries\"}]";
        let (verdict, state) = parse_verdict(response, 3).unwrap();
        assert_eq!(verdict.security_issues, 1);
        assert_eq!(verdict.findings.len(), 1);
        assert_eq!(
            verdict.findings[0].suggestion.as_deref(),
            Some("Use parameterized queries")
        );
        // Verdict string must flip to NEGATIVE for internal consistency.
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(state, ReviewState::RequestChanges);
    }

    /// Symmetry: metadata says NEGATIVE but findings contain only
    /// `Important` / `Suggestion` items. The fallback verdict should be
    /// preserved (no blocking findings), giving a non-blocking review.
    #[test]
    fn test_parse_verdict_negative_metadata_with_non_blocking_findings_preserved() {
        let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: NEGATIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 1\nSuggestions: 0\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{\"path\":\"a.rs\",\"line\":1,\"severity\":\"Important\",\"message\":\"Missing test\"}]";
        let (verdict, _) = parse_verdict(response, 3).unwrap();
        // No blocking findings → verdict string kept as the metadata's NEGATIVE.
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(verdict.critical_issues, 0);
        assert_eq!(verdict.security_issues, 0);
        assert_eq!(verdict.important_issues, 1);
        assert_eq!(verdict.findings.len(), 1);
    }

    #[test]
    fn test_parse_verdict_malformed_findings_returns_error() {
        // Marker present but JSON invalid → must error, not silently approve.
        let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{\"path\":\"a.rs\"}]";
        let result = parse_verdict(response, 3);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("malformed"),
            "error should explain JSON is malformed"
        );
    }

    #[test]
    fn test_parse_verdict_without_findings_uses_metadata() {
        // No findings marker — should use metadata counts as before
        let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";
        let (verdict, state) = parse_verdict(response, 3).unwrap();
        assert!(verdict.findings.is_empty());
        assert!(!verdict.has_findings());
        assert_eq!(state, ReviewState::Approve);
    }

    #[test]
    fn test_parse_verdict_empty_findings_array_uses_metadata() {
        // Empty findings array should not override metadata
        let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 1\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0\n\n[RS_GUARD_VERDICT_FINDINGS]\n[]";
        let (verdict, _) = parse_verdict(response, 3).unwrap();
        // Empty findings array means we keep metadata counts
        assert_eq!(verdict.critical_issues, 1);
        assert!(verdict.findings.is_empty());
    }
}
