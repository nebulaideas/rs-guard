//! Verdict parsing and review state determination.
//!
//! Parses structured metadata from LLM responses to determine the appropriate
//! GitHub review state (`APPROVE`, `REQUEST_CHANGES`, or `COMMENT`).
//!
//! The parser first attempts to extract a `[DIFFGUARD_VERDICT_METADATA]` block.
//! If no metadata block is found, it falls back to counting `[Critical Bug]`
//! and `[Security]` tags in the response text.

use crate::error::DiffguardError;
use regex::Regex;
use std::sync::LazyLock;

/// Compiled regex for extracting the verdict metadata block.
static METADATA_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\[DIFFGUARD_VERDICT_METADATA\][\s\S]*?Verdict:\s*(\w+)[\s\S]*?CriticalBugs:\s*(\d+)[\s\S]*?SecurityIssues:\s*(\d+)"
    ).expect("metadata regex is valid")
});

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
    /// Returns the GitHub REST API event string for this review state.
    pub fn as_github_state(&self) -> &'static str {
        match self {
            ReviewState::Approve => "APPROVE",
            ReviewState::RequestChanges => "CHANGES_REQUESTED",
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

/// Attempts to extract a `[DIFFGUARD_VERDICT_METADATA]` block from the response.
///
/// Returns `None` if the metadata block is not present or cannot be parsed.
pub fn parse_metadata_block(response: &str) -> Option<Verdict> {
    let caps = METADATA_RE.captures(response)?;
    Some(Verdict {
        verdict: caps.get(1)?.as_str().to_string(),
        critical_bugs: caps.get(2)?.as_str().parse().unwrap_or(0),
        security_issues: caps.get(3)?.as_str().parse().unwrap_or(0),
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
/// First attempts structured metadata extraction, falls back to tag counting,
/// then validates the verdict value and computes the review state.
///
/// # Errors
///
/// Returns [`DiffguardError::VerdictParse`] if the verdict value is neither
/// `"POSITIVE"` nor `"NEGATIVE"`.
pub fn parse_verdict(response: &str) -> Result<(Verdict, ReviewState), DiffguardError> {
    let verdict = parse_metadata_block(response).unwrap_or_else(|| evaluate_by_tags(response));

    if verdict.verdict != "POSITIVE" && verdict.verdict != "NEGATIVE" {
        return Err(DiffguardError::VerdictParse(format!(
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
        let response = "Some review text\n\n[DIFFGUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 0\nSecurityIssues: 0";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(verdict.verdict, "POSITIVE");
        assert_eq!(verdict.critical_bugs, 0);
        assert_eq!(verdict.security_issues, 0);
        assert_eq!(determine_review_state(&verdict), ReviewState::Approve);
    }

    #[test]
    fn test_parse_negative() {
        let response = "Some review text\n\n[DIFFGUARD_VERDICT_METADATA]\nVerdict: NEGATIVE\nCriticalBugs: 0\nSecurityIssues: 0";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(
            determine_review_state(&verdict),
            ReviewState::RequestChanges
        );
    }

    #[test]
    fn test_parse_critical_gt_2() {
        let response =
            "[DIFFGUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 5\nSecurityIssues: 0";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(
            determine_review_state(&verdict),
            ReviewState::RequestChanges
        );
    }

    #[test]
    fn test_parse_security_gt_0() {
        let response =
            "[DIFFGUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 0\nSecurityIssues: 1";
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
            "[DIFFGUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 1\nSecurityIssues: 0";
        let verdict = parse_metadata_block(response).unwrap();
        assert_eq!(determine_review_state(&verdict), ReviewState::Comment);
    }
}
