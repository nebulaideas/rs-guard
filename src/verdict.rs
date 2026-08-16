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

/// Counts findings by severity, returning a tuple
/// `(critical, security, important, suggestion)`.
///
/// Single-pass tally shared by [`Verdict::from_findings`] and
/// [`Verdict::merge_with_findings`] so the per-severity counting logic
/// lives in exactly one place.
fn count_findings(findings: &[Finding]) -> (u32, u32, u32, u32) {
    let mut critical_issues = 0u32;
    let mut security_issues = 0u32;
    let mut important_issues = 0u32;
    let mut suggestions = 0u32;
    for finding in findings {
        match finding.severity {
            FindingSeverity::Critical => critical_issues += 1,
            FindingSeverity::Security => security_issues += 1,
            FindingSeverity::Important => important_issues += 1,
            FindingSeverity::Suggestion => suggestions += 1,
        }
    }
    (
        critical_issues,
        security_issues,
        important_issues,
        suggestions,
    )
}

impl Verdict {
    /// Creates a `Verdict` from findings alone, without a preliminary
    /// verdict. Prefer [`Verdict::merge_with_findings`] when a preliminary
    /// verdict exists; this constructor is for tests and for the rare case
    /// where no metadata block was available.
    ///
    /// The verdict string is `"NEGATIVE"` if the findings would produce
    /// a `RequestChanges` under the given threshold (any `Critical`/
    /// `Security` findings, or `important_issues >= important_threshold`).
    /// Otherwise `fallback_verdict` is preserved.
    pub fn from_findings(
        fallback_verdict: &str,
        findings: Vec<Finding>,
        important_threshold: u32,
    ) -> Self {
        let (critical_issues, security_issues, important_issues, suggestions) =
            count_findings(&findings);

        let would_block = critical_issues > 0
            || security_issues > 0
            || (important_threshold > 0 && important_issues >= important_threshold);
        let verdict = if would_block {
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

    /// Merges a structured-findings block into a preliminary `Verdict`
    /// derived from the metadata block or tag counting, applying the
    /// **max-rule** for each severity count. This is the fail-safe merge:
    /// findings can add new evidence but cannot suppress a blocking
    /// preliminary verdict or down-count a blocking severity.
    ///
    /// Specifically, for each severity (`critical_issues`, `security_issues`,
    /// `important_issues`, `suggestions`):
    ///
    /// - `final_count = max(preliminary_count, count_in_findings)`
    ///
    /// The verdict string is forced to `"NEGATIVE"` if either the
    /// preliminary verdict was already blocking, or the merged counts
    /// produce a blocking review under the given `important_threshold`.
    ///
    /// The findings are stored on the returned `Verdict` regardless of
    /// whether they changed the counts.
    pub fn merge_with_findings(
        preliminary: &Verdict,
        findings: Vec<Finding>,
        important_threshold: u32,
    ) -> Self {
        let (findings_critical, findings_security, findings_important, findings_suggestions) =
            count_findings(&findings);

        let critical_issues = preliminary.critical_issues.max(findings_critical);
        let security_issues = preliminary.security_issues.max(findings_security);
        let important_issues = preliminary.important_issues.max(findings_important);
        let suggestions = preliminary.suggestions.max(findings_suggestions);

        // Preliminary blocks if it was NEGATIVE or any of its counts
        // would have triggered RequestChanges on their own.
        let preliminary_blocks = preliminary.verdict == "NEGATIVE"
            || preliminary.critical_issues > 0
            || preliminary.security_issues > 0
            || (important_threshold > 0 && preliminary.important_issues >= important_threshold);

        // Merged counts block if any of them would trigger RequestChanges.
        let merged_blocks = critical_issues > 0
            || security_issues > 0
            || (important_threshold > 0 && important_issues >= important_threshold);

        let verdict = if preliminary_blocks || merged_blocks {
            "NEGATIVE".to_string()
        } else {
            preliminary.verdict.clone()
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
                    "Deprecated metadata field 'CriticalBugs:' detected alongside \
                     'CriticalIssues:'; 'CriticalBugs:' is ignored. Prefer \
                     'CriticalIssues:' only. CriticalBugs will be removed in a \
                     future major release."
                );
            } else {
                log::warn!(
                    "Deprecated metadata field 'CriticalBugs:' detected; use \
                     'CriticalIssues:' instead. CriticalBugs will be removed in \
                     a future major release."
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
///    A malformed findings block is treated as a `VerdictParse` error
///    **only when the preliminary verdict is not already blocking** —
///    otherwise the malformed JSON is logged as a warning and the
///    fallback blocking verdict is preserved (so we never suppress a
///    valid `REQUEST_CHANGES` review because of bad findings JSON).
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
/// - A `[RS_GUARD_VERDICT_FINDINGS]` marker is present but the JSON is
///   malformed **and** the preliminary verdict would not otherwise block
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

    // Step 1: extract the metadata block to get the preliminary verdict
    // string and counts. Fall back to tag counting if no metadata block.
    let preliminary = parse_metadata_block(response).unwrap_or_else(|| evaluate_by_tags(response));

    // Step 2: merge structured findings using the max-rule. This is the
    // fail-safe direction: findings can add new evidence but cannot
    // suppress a blocking preliminary verdict or down-count a blocking
    // severity. See Verdict::merge_with_findings.
    let verdict = match parse_findings(response) {
        Ok(Some(findings)) if !findings.is_empty() => {
            Verdict::merge_with_findings(&preliminary, findings, important_threshold)
        }
        Ok(_) => preliminary,
        Err(e) => {
            // Malformed findings: only propagate the error when the
            // preliminary verdict is not already blocking. If the
            // preliminary verdict is already blocking, log a warning and
            // keep it (a malformed findings block must never suppress a
            // valid REQUEST_CHANGES review). The verdict string is
            // normalized to "NEGATIVE" when the preliminary counts would
            // have produced a blocking review, so callers cannot observe
            // a "POSITIVE" verdict alongside a blocking review state.
            let preliminary_blocks = preliminary.verdict == "NEGATIVE"
                || preliminary.critical_issues > 0
                || preliminary.security_issues > 0
                || (important_threshold > 0 && preliminary.important_issues >= important_threshold);
            if preliminary_blocks {
                log::warn!(
                    "Ignoring malformed [RS_GUARD_VERDICT_FINDINGS] block: {}. \
                     Preliminary verdict is already blocking; keeping it.",
                    e
                );
                let mut v = preliminary;
                if v.verdict == "POSITIVE" {
                    v.verdict = "NEGATIVE".to_string();
                }
                v
            } else {
                return Err(e);
            }
        }
    };

    if verdict.verdict != "POSITIVE" && verdict.verdict != "NEGATIVE" {
        return Err(RsGuardError::VerdictParse(format!(
            "Invalid verdict value: {}. Expected POSITIVE or NEGATIVE.",
            verdict.verdict
        )));
    }

    let state = determine_review_state(&verdict, important_threshold);
    Ok((verdict, state))
}

/// Maximum bytes allowed in the findings JSON block after the marker.
///
/// Caps allocations from adversarial or malformed LLM responses. The
/// default is intentionally generous — a single finding with a long
/// message can already be ~200 bytes.
const MAX_FINDINGS_JSON_BYTES: usize = 64 * 1024;

/// Maximum number of findings allowed in a single response.
///
/// Caps allocations from adversarial or malformed LLM responses. Most real
/// reviews report << 100 findings; 1000 leaves headroom for verbose
/// reviews without giving the model a megabyte of attacks.
const MAX_FINDINGS_COUNT: usize = 1000;

/// Parses structured findings from a `[RS_GUARD_VERDICT_FINDINGS]` JSON
/// block at the **end** of the LLM response.
///
/// Returns `Ok(None)` if no marker is present. Returns `Err(VerdictParse)`
/// if the marker is present but the content is malformed, since silently
/// dropping explicitly-attempted findings is a security-sensitive failure
/// mode.
///
/// # Marker placement
///
/// The parser uses `rfind` to locate the **last** occurrence of the
/// marker. This is intentional: the LLM may quote code from the PR (which
/// could itself contain the marker text). When multiple markers exist,
/// the response is rejected as ambiguous.
///
/// # Code-fence tolerance
///
/// A single surrounding ```` ```json ... ``` ```` fence is stripped before
/// parsing if present. This allows the LLM to wrap the JSON even when the
/// prompt asks for raw output, without accidentally accepting arbitrary
/// fences that could hide non-JSON content.
///
/// # Size limits
///
/// The findings block is capped at 64 KiB (65,536 bytes) and the resulting
/// array at 1,000 entries. Both limits return `Err(VerdictParse)` so that
/// over-sized responses fail loudly instead of silently truncating.
pub fn parse_findings(response: &str) -> Result<Option<Vec<Finding>>, RsGuardError> {
    // Use rfind so an attacker-influenced marker in the middle of the
    // response (e.g. in quoted diff content) cannot be picked up.
    let Some(marker_pos) = response.rfind(FINDINGS_MARKER) else {
        return Ok(None);
    };
    // Reject multiple markers — the LLM is expected to emit exactly one.
    // We count occurrences in the full response, not just after the
    // last marker, to detect the case where an earlier fake marker is
    // embedded in quoted diff content.
    if response.matches(FINDINGS_MARKER).count() > 1 {
        return Err(RsGuardError::VerdictParse(format!(
            "Multiple '{}' markers found; the findings block must appear exactly once.",
            FINDINGS_MARKER
        )));
    }
    let json_start = marker_pos + FINDINGS_MARKER.len();
    let raw = response[json_start..].trim();

    let json_text = strip_code_fence(raw);

    // The JSON must start with '[' to be a valid findings array.
    if !json_text.starts_with('[') {
        return Err(RsGuardError::VerdictParse(format!(
            "Found '{}' marker but the following content is not a JSON array. \
             Expected a JSON array of Finding objects.",
            FINDINGS_MARKER
        )));
    }

    // Cap byte size before deserializing to bound allocations.
    if json_text.len() > MAX_FINDINGS_JSON_BYTES {
        return Err(RsGuardError::VerdictParse(format!(
            "Findings block is {} bytes, exceeding the {} byte limit.",
            json_text.len(),
            MAX_FINDINGS_JSON_BYTES
        )));
    }

    let findings: Vec<Finding> = serde_json::from_str(json_text).map_err(|e| {
        RsGuardError::VerdictParse(format!(
            "Found '{}' marker but the JSON is malformed: {}. \
                 A malformed findings block is treated as an error to prevent \
                 silent approval when the model attempted to report findings.",
            FINDINGS_MARKER, e
        ))
    })?;

    if findings.len() > MAX_FINDINGS_COUNT {
        return Err(RsGuardError::VerdictParse(format!(
            "Findings array has {} entries, exceeding the {} entry limit.",
            findings.len(),
            MAX_FINDINGS_COUNT
        )));
    }

    Ok(Some(findings))
}

/// Strips a single surrounding code fence from `text`, if present.
/// If no fence is detected, `text` is returned unchanged.
fn strip_code_fence(text: &str) -> &str {
    if let Some(after_open) = text.strip_prefix("```json") {
        if let Some(after_close) = after_open.trim_start().strip_suffix("```") {
            return after_close.trim();
        }
    } else if let Some(after_open) = text.strip_prefix("```") {
        if let Some(after_close) = after_open.trim_start().strip_suffix("```") {
            return after_close.trim();
        }
    }
    text
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
    if let Some(marker_pos) = response.rfind(FINDINGS_MARKER) {
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
        let verdict = Verdict::from_findings("POSITIVE", findings, 3);
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
        let verdict = Verdict::from_findings("POSITIVE", vec![], 3);
        assert_eq!(verdict.verdict, "POSITIVE");
        assert_eq!(verdict.critical_issues, 0);
        assert!(!verdict.has_findings());
    }

    #[test]
    fn test_verdict_from_findings_preserves_fallback_when_no_blocking() {
        // No Critical/Security findings and important_issues < threshold →
        // fallback verdict string is preserved even if the caller passed NEGATIVE.
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
        let verdict = Verdict::from_findings("NEGATIVE", findings, 3);
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
        let verdict = Verdict::from_findings("POSITIVE", findings, 3);
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
        let verdict = Verdict::from_findings("POSITIVE", findings, 3);
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(verdict.security_issues, 1);
    }

    /// Regression: when `important_issues >= important_threshold`, the verdict
    /// string must be `"NEGATIVE"` to match the `RequestChanges` review state.
    /// Previously this produced a `"POSITIVE"` verdict string with a blocking
    /// review state.
    #[test]
    fn test_verdict_from_findings_forces_negative_when_important_meets_threshold() {
        let findings = vec![
            Finding {
                path: "a.rs".into(),
                line: 1,
                severity: FindingSeverity::Important,
                message: "imp1".into(),
                suggestion: None,
            },
            Finding {
                path: "b.rs".into(),
                line: 2,
                severity: FindingSeverity::Important,
                message: "imp2".into(),
                suggestion: None,
            },
            Finding {
                path: "c.rs".into(),
                line: 3,
                severity: FindingSeverity::Important,
                message: "imp3".into(),
                suggestion: None,
            },
        ];
        // 3 important findings, threshold 3 → must be NEGATIVE.
        let verdict = Verdict::from_findings("POSITIVE", findings, 3);
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(verdict.important_issues, 3);
    }

    /// `important_issues < threshold` and no critical/security → preserve
    /// the fallback verdict (will become `Comment` in `determine_review_state`).
    #[test]
    fn test_verdict_from_findings_below_threshold_preserves_fallback() {
        let findings = vec![
            Finding {
                path: "a.rs".into(),
                line: 1,
                severity: FindingSeverity::Important,
                message: "imp1".into(),
                suggestion: None,
            },
            Finding {
                path: "b.rs".into(),
                line: 2,
                severity: FindingSeverity::Important,
                message: "imp2".into(),
                suggestion: None,
            },
        ];
        // 2 important findings, threshold 3 → below threshold.
        let verdict = Verdict::from_findings("POSITIVE", findings, 3);
        assert_eq!(verdict.verdict, "POSITIVE");
        assert_eq!(verdict.important_issues, 2);
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
    fn test_parse_verdict_malformed_findings_with_non_blocking_preliminary_errors() {
        // Marker present but JSON invalid AND preliminary verdict is not
        // blocking → must error, not silently approve.
        let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{\"path\":\"a.rs\"}]";
        let result = parse_verdict(response, 3);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("malformed"),
            "error should explain JSON is malformed"
        );
    }

    /// Regression: when the preliminary verdict is already blocking (NEGATIVE
    /// metadata), a malformed findings block must NOT turn the review into
    /// an error — the valid REQUEST_CHANGES review should still go through.
    #[test]
    fn test_parse_verdict_malformed_findings_with_blocking_preliminary_keeps_block() {
        let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: NEGATIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{\"path\":\"a.rs\"}]";
        let (verdict, state) = parse_verdict(response, 3)
            .expect("malformed findings must not suppress a blocking preliminary verdict");
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(state, ReviewState::RequestChanges);
    }

    /// Regression: preliminary POSITIVE metadata with `CriticalIssues: 1`
    /// (already blocking) plus malformed findings → must keep the block.
    #[test]
    fn test_parse_verdict_malformed_findings_with_critical_in_preliminary_keeps_block() {
        let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 1\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{\"path\":\"a.rs\"}]";
        let (verdict, state) = parse_verdict(response, 3)
            .expect("malformed findings must not suppress a critical preliminary verdict");
        assert_eq!(verdict.critical_issues, 1);
        assert_eq!(state, ReviewState::RequestChanges);
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

    // ── Critical fail-open regression tests (third review) ──────────────

    /// Regression: metadata `CriticalIssues: 1` plus findings containing
    /// only `Suggestion` items MUST NOT silently approve. The merged counts
    /// take the max of the two sources, so the critical issue from the
    /// metadata block survives the merge.
    #[test]
    fn test_merge_findings_does_not_erase_blocking_metadata_counts() {
        let preliminary = Verdict {
            verdict: "POSITIVE".to_string(),
            critical_issues: 1,
            security_issues: 0,
            important_issues: 0,
            suggestions: 0,
            findings: Vec::new(),
        };
        let findings = vec![Finding {
            path: "a.rs".into(),
            line: 1,
            severity: FindingSeverity::Suggestion,
            message: "nit".into(),
            suggestion: None,
        }];
        let merged = Verdict::merge_with_findings(&preliminary, findings, 3);
        assert_eq!(
            merged.critical_issues, 1,
            "metadata's CriticalIssues: 1 must survive the merge"
        );
        assert_eq!(merged.verdict, "NEGATIVE");
    }

    /// Regression: same scenario through `parse_verdict`. A blocking
    /// metadata block (CriticalIssues: 1) must not be downgraded by a
    /// findings block that contains only Suggestion items.
    #[test]
    fn test_parse_verdict_findings_cannot_erase_blocking_metadata() {
        let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 1\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{\"path\":\"a.rs\",\"line\":1,\"severity\":\"Suggestion\",\"message\":\"nit\"}]";
        let (verdict, state) = parse_verdict(response, 3)
            .expect("blocking metadata must not be erased by a weaker findings block");
        assert_eq!(verdict.critical_issues, 1);
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(state, ReviewState::RequestChanges);
    }

    /// Regression: `ImportantIssues: 5` (>= threshold 3) plus findings
    /// containing only Suggestion items → still blocking.
    #[test]
    fn test_parse_verdict_findings_cannot_erase_blocking_important_count() {
        let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 5\nSuggestions: 0\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{\"path\":\"a.rs\",\"line\":1,\"severity\":\"Suggestion\",\"message\":\"nit\"}]";
        let (verdict, state) = parse_verdict(response, 3)
            .expect("metadata ImportantIssues >= threshold must not be erased");
        assert_eq!(verdict.important_issues, 5);
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(state, ReviewState::RequestChanges);
    }

    /// Regression: Security issue in metadata + Suggestion findings →
    /// Security count survives the merge.
    #[test]
    fn test_parse_verdict_findings_cannot_erase_blocking_security_count() {
        let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 2\nImportantIssues: 0\nSuggestions: 0\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{\"path\":\"a.rs\",\"line\":1,\"severity\":\"Suggestion\",\"message\":\"nit\"}]";
        let (verdict, state) =
            parse_verdict(response, 3).expect("metadata SecurityIssues must not be erased");
        assert_eq!(verdict.security_issues, 2);
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(state, ReviewState::RequestChanges);
    }

    /// Regression: when the LLM echoes diff content that contains the
    /// findings marker, the parser must use the *last* marker (rfind) for
    /// the JSON position, but multiple markers are rejected as ambiguous.
    /// (This test exercises the security position: an attacker cannot
    /// inject a fake marker that overrides the real one.)
    #[test]
    fn test_parse_findings_multiple_markers_in_response_errors() {
        let response = "Some prose.\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{\"path\":\"fake.rs\",\"line\":1,\"severity\":\"Suggestion\",\"message\":\"fake\"}]\n\nReal findings:\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{\"path\":\"real.rs\",\"line\":99,\"severity\":\"Critical\",\"message\":\"real bug\"}]";
        let result = parse_findings(response);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Multiple"));
    }

    /// Regression: rfind picks the last marker position when only one
    /// marker is present (no ambiguity).
    #[test]
    fn test_parse_findings_uses_last_marker_single() {
        let response = "Real findings:\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{\"path\":\"real.rs\",\"line\":99,\"severity\":\"Critical\",\"message\":\"real bug\"}]";
        let findings = parse_findings(response)
            .expect("should parse")
            .expect("should be Some");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].path, "real.rs");
        assert_eq!(findings[0].severity, FindingSeverity::Critical);
    }

    /// Regression: when the preliminary verdict is `POSITIVE` with
    /// `CriticalIssues: 1` and the findings are malformed, the returned
    /// verdict string must be `NEGATIVE` to match
    /// `determine_review_state`'s output of `RequestChanges`.
    #[test]
    fn test_malformed_findings_with_blocking_preliminary_normalizes_verdict() {
        let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 1\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{\"path\":\"a.rs\"}]";
        let (verdict, state) = parse_verdict(response, 3)
            .expect("malformed findings must not suppress the critical preliminary");
        assert_eq!(verdict.verdict, "NEGATIVE");
        assert_eq!(verdict.critical_issues, 1);
        assert_eq!(state, ReviewState::RequestChanges);
    }

    /// Regression: findings JSON exceeding the byte cap → error.
    #[test]
    fn test_parse_findings_byte_cap() {
        let mut entries = Vec::new();
        for i in 0..100 {
            entries.push(format!(
                r#"{{"path":"a.rs","line":{i},"severity":"Suggestion","message":"{}"}}"#,
                "x".repeat(700)
            ));
        }
        let response = format!("[RS_GUARD_VERDICT_FINDINGS]\n[{}]", entries.join(","));
        let result = parse_findings(&response);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("byte limit"));
    }

    /// Regression: findings count exceeding the cap → error.
    #[test]
    fn test_parse_findings_count_cap() {
        let entries: Vec<String> = (0..1001)
            .map(|i| {
                format!(r#"{{"path":"a.rs","line":{i},"severity":"Suggestion","message":"m"}}"#)
            })
            .collect();
        let response = format!("[RS_GUARD_VERDICT_FINDINGS]\n[{}]", entries.join(","));
        let result = parse_findings(&response);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("entry limit"));
    }

    /// Regression: a single surrounding json code fence is tolerated.
    #[test]
    fn test_parse_findings_tolerates_json_code_fence() {
        let response = "Review complete.\n\n[RS_GUARD_VERDICT_FINDINGS]\n```json\n[{\"path\":\"a.rs\",\"line\":1,\"severity\":\"Suggestion\",\"message\":\"m\"}]\n```";
        let findings = parse_findings(response)
            .expect("should parse with json fence")
            .expect("should be Some");
        assert_eq!(findings.len(), 1);
    }
}
