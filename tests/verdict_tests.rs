use diffguard::verdict::{
    determine_review_state, evaluate_by_tags, parse_verdict, ReviewState, Verdict,
};

#[test]
fn test_parse_valid_positive_clean() {
    let response = "Review text\n\n[DIFFGUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 0\nSecurityIssues: 0";
    let (_verdict, state) = parse_verdict(response).unwrap();
    assert_eq!(state, ReviewState::Approve);
}

#[test]
fn test_parse_negative_verdict() {
    let response =
        "[DIFFGUARD_VERDICT_METADATA]\nVerdict: NEGATIVE\nCriticalBugs: 0\nSecurityIssues: 0";
    let (_verdict, state) = parse_verdict(response).unwrap();
    assert_eq!(state, ReviewState::RequestChanges);
}

#[test]
fn test_parse_critical_bugs_gt_2() {
    let response =
        "[DIFFGUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 5\nSecurityIssues: 0";
    let (_verdict, state) = parse_verdict(response).unwrap();
    assert_eq!(state, ReviewState::RequestChanges);
}

#[test]
fn test_parse_security_issues_gt_0() {
    let response =
        "[DIFFGUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 0\nSecurityIssues: 3";
    let (_verdict, state) = parse_verdict(response).unwrap();
    assert_eq!(state, ReviewState::RequestChanges);
}

#[test]
fn test_parse_positive_with_minor_bugs_yields_comment() {
    let response =
        "[DIFFGUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 2\nSecurityIssues: 0";
    let (_verdict, state) = parse_verdict(response).unwrap();
    assert_eq!(state, ReviewState::Comment);
}

#[test]
fn test_parse_positive_with_1_critical_bug_yields_comment() {
    let response =
        "[DIFFGUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 1\nSecurityIssues: 0";
    let (_verdict, state) = parse_verdict(response).unwrap();
    assert_eq!(state, ReviewState::Comment);
}

#[test]
fn test_missing_metadata_block_fallback_tags() {
    let response = "I found some issues.\n[Critical Bug] Null pointer dereference\n[Critical Bug] Race condition\n[Security] XSS vulnerability";
    let (verdict, state) = parse_verdict(response).unwrap();
    assert_eq!(verdict.critical_bugs, 2);
    assert_eq!(verdict.security_issues, 1);
    assert_eq!(state, ReviewState::RequestChanges);
}

#[test]
fn test_clean_response_no_tags_yields_approve() {
    let response = "Everything looks good. No issues found in this PR.";
    let (verdict, state) = parse_verdict(response).unwrap();
    assert_eq!(verdict.critical_bugs, 0);
    assert_eq!(verdict.security_issues, 0);
    assert_eq!(state, ReviewState::Approve);
}

#[test]
fn test_invalid_verdict_value() {
    let response =
        "[DIFFGUARD_VERDICT_METADATA]\nVerdict: MAYBE\nCriticalBugs: 0\nSecurityIssues: 0";
    let result = parse_verdict(response);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid verdict value"));
}

#[test]
fn test_evaluate_by_tags_critical_only() {
    let response = "Found a [Critical Bug] memory leak. Otherwise looks fine.";
    let verdict = evaluate_by_tags(response);
    assert_eq!(verdict.critical_bugs, 1);
    assert_eq!(verdict.security_issues, 0);
    assert_eq!(verdict.verdict, "NEGATIVE");
}

#[test]
fn test_evaluate_by_tags_security_only() {
    let response = "Found a [Security] issue with hardcoded password.";
    let verdict = evaluate_by_tags(response);
    assert_eq!(verdict.critical_bugs, 0);
    assert_eq!(verdict.security_issues, 1);
    assert_eq!(verdict.verdict, "NEGATIVE");
}

#[test]
fn test_evaluate_by_tags_alternate_formats() {
    let response = "[Critical] Buffer overflow\n[Security Issue] SQL injection";
    let verdict = evaluate_by_tags(response);
    assert_eq!(verdict.critical_bugs, 1);
    assert_eq!(verdict.security_issues, 1);
}

#[test]
fn test_determine_review_state_negative_always_requests_changes() {
    let verdict = Verdict {
        verdict: "NEGATIVE".to_string(),
        critical_bugs: 0,
        security_issues: 0,
    };
    assert_eq!(
        determine_review_state(&verdict),
        ReviewState::RequestChanges
    );
}

#[test]
fn test_determine_review_state_positive_with_zero_counts_approves() {
    let verdict = Verdict {
        verdict: "POSITIVE".to_string(),
        critical_bugs: 0,
        security_issues: 0,
    };
    assert_eq!(determine_review_state(&verdict), ReviewState::Approve);
}

#[test]
fn test_determine_review_state_asymmetric_safety() {
    // Positive verdict but 3 critical bugs -> REQUEST_CHANGES (bugs override)
    let verdict = Verdict {
        verdict: "POSITIVE".to_string(),
        critical_bugs: 3,
        security_issues: 0,
    };
    assert_eq!(
        determine_review_state(&verdict),
        ReviewState::RequestChanges
    );

    // Negative verdict but 0 bugs -> REQUEST_CHANGES (verdict overrides)
    let verdict = Verdict {
        verdict: "NEGATIVE".to_string(),
        critical_bugs: 0,
        security_issues: 0,
    };
    assert_eq!(
        determine_review_state(&verdict),
        ReviewState::RequestChanges
    );
}
