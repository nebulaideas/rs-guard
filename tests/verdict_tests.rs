use rs_guard::verdict::{
    determine_review_state, evaluate_by_tags, parse_metadata_block, parse_verdict, ReviewState,
    Verdict,
};

#[test]
fn test_parse_valid_positive_clean() {
    let response = "Review text\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";
    let (_verdict, state) = parse_verdict(response).unwrap();
    assert_eq!(state, ReviewState::Approve);
}

#[test]
fn test_parse_negative_verdict() {
    let response =
        "[RS_GUARD_VERDICT_METADATA]\nVerdict: NEGATIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";
    let (_verdict, state) = parse_verdict(response).unwrap();
    assert_eq!(state, ReviewState::RequestChanges);
}

#[test]
fn test_parse_critical_issues_gt_0_blocks() {
    // Any [Critical] issue blocks merge regardless of verdict
    let response =
        "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 5\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";
    let (_verdict, state) = parse_verdict(response).unwrap();
    assert_eq!(state, ReviewState::RequestChanges);
}

#[test]
fn test_parse_security_issues_gt_0() {
    let response =
        "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 3\nImportantIssues: 0\nSuggestions: 0";
    let (_verdict, state) = parse_verdict(response).unwrap();
    assert_eq!(state, ReviewState::RequestChanges);
}

#[test]
fn test_parse_positive_with_2_important_issues_yields_comment() {
    // [Important] 1-2 → COMMENT (human review recommended, not blocked)
    let response =
        "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 2\nSuggestions: 0";
    let (_verdict, state) = parse_verdict(response).unwrap();
    assert_eq!(state, ReviewState::Comment);
}

#[test]
fn test_parse_positive_with_1_critical_issue_yields_request_changes() {
    // [Critical] always blocks, even with positive verdict
    let response =
        "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 1\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";
    let (_verdict, state) = parse_verdict(response).unwrap();
    assert_eq!(state, ReviewState::RequestChanges);
}

#[test]
fn test_missing_metadata_block_fallback_tags() {
    let response = "I found some issues.\n[Critical Bug] Null pointer dereference\n[Critical Bug] Race condition\n[Security] XSS vulnerability";
    let (verdict, state) = parse_verdict(response).unwrap();
    assert_eq!(verdict.critical_issues, 2);
    assert_eq!(verdict.security_issues, 1);
    assert_eq!(state, ReviewState::RequestChanges);
}

#[test]
fn test_clean_response_no_tags_yields_approve() {
    let response = "Everything looks good. No issues found in this PR.";
    let (verdict, state) = parse_verdict(response).unwrap();
    assert_eq!(verdict.critical_issues, 0);
    assert_eq!(verdict.security_issues, 0);
    assert_eq!(state, ReviewState::Approve);
}

#[test]
fn test_invalid_verdict_value() {
    let response =
        "[RS_GUARD_VERDICT_METADATA]\nVerdict: MAYBE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";
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
    assert_eq!(verdict.critical_issues, 1);
    assert_eq!(verdict.security_issues, 0);
    assert_eq!(verdict.verdict, "NEGATIVE");
}

#[test]
fn test_evaluate_by_tags_security_only() {
    let response = "Found a [Security] issue with hardcoded password.";
    let verdict = evaluate_by_tags(response);
    assert_eq!(verdict.critical_issues, 0);
    assert_eq!(verdict.security_issues, 1);
    assert_eq!(verdict.verdict, "NEGATIVE");
}

#[test]
fn test_evaluate_by_tags_alternate_formats() {
    let response = "[Critical] Buffer overflow\n[Security Issue] SQL injection";
    let verdict = evaluate_by_tags(response);
    assert_eq!(verdict.critical_issues, 1);
    assert_eq!(verdict.security_issues, 1);
}

#[test]
fn test_determine_review_state_negative_always_requests_changes() {
    let verdict = Verdict {
        verdict: "NEGATIVE".to_string(),
        critical_issues: 0,
        security_issues: 0,
        important_issues: 0,
        suggestions: 0,
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
        critical_issues: 0,
        security_issues: 0,
        important_issues: 0,
        suggestions: 0,
    };
    assert_eq!(determine_review_state(&verdict), ReviewState::Approve);
}

#[test]
fn test_determine_review_state_asymmetric_safety() {
    // Positive verdict but 3 critical bugs -> REQUEST_CHANGES (bugs override)
    let verdict = Verdict {
        verdict: "POSITIVE".to_string(),
        critical_issues: 3,
        security_issues: 0,
        important_issues: 0,
        suggestions: 0,
    };
    assert_eq!(
        determine_review_state(&verdict),
        ReviewState::RequestChanges
    );

    // Negative verdict but 0 bugs -> REQUEST_CHANGES (verdict overrides)
    let verdict = Verdict {
        verdict: "NEGATIVE".to_string(),
        critical_issues: 0,
        security_issues: 0,
        important_issues: 0,
        suggestions: 0,
    };
    assert_eq!(
        determine_review_state(&verdict),
        ReviewState::RequestChanges
    );
}

// ---------------------------------------------------------------------------
// Regression: evaluate_by_tags must count variant tag forms
// ---------------------------------------------------------------------------

#[test]
fn test_evaluate_by_tags_counts_important_issue_variant() {
    // [Important Issue] is an alternate form the LLM may emit
    let response = "[Important Issue] Missing null check\n[Important] No coverage";
    let verdict = evaluate_by_tags(response);
    assert_eq!(verdict.important_issues, 2);
}

#[test]
fn test_evaluate_by_tags_counts_suggestion_issue_variant() {
    // [Suggestion Issue] is an alternate form the LLM may emit
    let response = "[Suggestion Issue] Rename variable\n[Suggestion] Extract method";
    let verdict = evaluate_by_tags(response);
    assert_eq!(verdict.suggestions, 2);
}

// ---------------------------------------------------------------------------
// Boundary: metadata scan window off-by-one
// ---------------------------------------------------------------------------

#[test]
fn test_metadata_scan_window_exact_boundary() {
    // Metadata fields start exactly at byte 4096 after the marker — must still parse
    // METADATA_SCAN_WINDOW is 4096, so content of length 4095 fills the window and
    // the fields land just inside it.
    let fields = "\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";
    let padding_len = 4096usize.saturating_sub(fields.len());
    let padding = "x".repeat(padding_len);
    let response = format!("[RS_GUARD_VERDICT_METADATA]{}{}", padding, fields);
    // Fields land right at the boundary — should parse successfully
    let verdict = parse_metadata_block(&response);
    assert!(
        verdict.is_some(),
        "fields at scan window boundary should parse"
    );
    let v = verdict.unwrap();
    assert_eq!(v.verdict, "POSITIVE");
}

// ---------------------------------------------------------------------------
// TDD tests for Step 2: new Verdict fields + updated determine_review_state
// ---------------------------------------------------------------------------

#[test]
fn test_verdict_struct_has_important_issues_field() {
    // Arrange / Act: construct Verdict with all four count fields
    let verdict = Verdict {
        verdict: "POSITIVE".to_string(),
        critical_issues: 0,
        security_issues: 0,
        important_issues: 2,
        suggestions: 3,
    };
    // Assert: new fields are accessible and hold correct values
    assert_eq!(verdict.important_issues, 2);
    assert_eq!(verdict.suggestions, 3);
}

#[test]
fn test_parse_metadata_block_reads_important_issues_and_suggestions() {
    // Arrange: new four-field metadata block format from DEFAULT_PROMPT
    let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 3\nSuggestions: 2";
    // Act
    let verdict = parse_metadata_block(response).unwrap();
    // Assert
    assert_eq!(verdict.important_issues, 3);
    assert_eq!(verdict.suggestions, 2);
}

#[test]
fn test_parse_metadata_block_defaults_missing_new_fields_to_zero() {
    // Arrange: legacy two-field block — new fields absent, should default to 0
    let response =
        "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 0\nSecurityIssues: 0";
    // Act
    let verdict = parse_metadata_block(response).unwrap();
    // Assert: new fields default gracefully
    assert_eq!(verdict.important_issues, 0);
    assert_eq!(verdict.suggestions, 0);
}

#[test]
fn test_determine_review_state_important_issues_lt_3_yields_comment() {
    // Arrange: 2 important issues, no critical or security — should yield COMMENT
    let verdict = Verdict {
        verdict: "POSITIVE".to_string(),
        critical_issues: 0,
        security_issues: 0,
        important_issues: 2,
        suggestions: 0,
    };
    // Act / Assert
    assert_eq!(determine_review_state(&verdict), ReviewState::Comment);
}

#[test]
fn test_determine_review_state_important_issues_eq_3_yields_request_changes() {
    // Arrange: exactly 3 important issues triggers REQUEST_CHANGES threshold
    let verdict = Verdict {
        verdict: "POSITIVE".to_string(),
        critical_issues: 0,
        security_issues: 0,
        important_issues: 3,
        suggestions: 0,
    };
    // Act / Assert
    assert_eq!(
        determine_review_state(&verdict),
        ReviewState::RequestChanges
    );
}

#[test]
fn test_determine_review_state_important_issues_gt_3_yields_request_changes() {
    // Arrange: more than 3 important issues — still REQUEST_CHANGES
    let verdict = Verdict {
        verdict: "POSITIVE".to_string(),
        critical_issues: 0,
        security_issues: 0,
        important_issues: 5,
        suggestions: 10,
    };
    // Act / Assert
    assert_eq!(
        determine_review_state(&verdict),
        ReviewState::RequestChanges
    );
}

#[test]
fn test_determine_review_state_suggestions_alone_do_not_block() {
    // Arrange: suggestions only — must never block merge
    let verdict = Verdict {
        verdict: "POSITIVE".to_string(),
        critical_issues: 0,
        security_issues: 0,
        important_issues: 0,
        suggestions: 99,
    };
    // Act / Assert
    assert_eq!(determine_review_state(&verdict), ReviewState::Approve);
}

#[test]
fn test_evaluate_by_tags_counts_important_and_suggestion_tags() {
    // Arrange: response with new severity tags from the five-axis prompt
    let response =
        "[Important] Missing error handling\n[Important] No test coverage\n[Suggestion] Rename variable";
    // Act
    let verdict = evaluate_by_tags(response);
    // Assert
    assert_eq!(verdict.important_issues, 2);
    assert_eq!(verdict.suggestions, 1);
    // Important alone (< 3) should not drive NEGATIVE verdict
    assert_eq!(verdict.verdict, "POSITIVE");
}

#[test]
fn test_evaluate_by_tags_three_important_issues_drives_negative() {
    // Arrange: 3 [Important] tags — threshold for REQUEST_CHANGES
    let response =
        "[Important] Missing test\n[Important] Wrong abstraction\n[Important] Poor error handling";
    // Act
    let verdict = evaluate_by_tags(response);
    // Assert: evaluate_by_tags sets verdict based on critical/security only;
    // determine_review_state applies the important threshold
    assert_eq!(verdict.important_issues, 3);
    assert_eq!(
        determine_review_state(&verdict),
        ReviewState::RequestChanges
    );
}

// ---------------------------------------------------------------------------
// New tests: additional branch coverage for Step 5
// ---------------------------------------------------------------------------

#[test]
fn test_parse_verdict_full_four_field_block_round_trip() {
    // Arrange: canonical four-field block as emitted by the updated DEFAULT_PROMPT
    let response = "Good review.\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 2";
    // Act
    let (verdict, state) = parse_verdict(response).unwrap();
    // Assert: all fields parsed, suggestions alone never block
    assert_eq!(verdict.verdict, "POSITIVE");
    assert_eq!(verdict.critical_issues, 0);
    assert_eq!(verdict.security_issues, 0);
    assert_eq!(verdict.important_issues, 0);
    assert_eq!(verdict.suggestions, 2);
    assert_eq!(state, ReviewState::Approve);
}

#[test]
fn test_parse_verdict_important_issues_threshold_table() {
    // Table-driven: (important_issues count, expected ReviewState).
    // Threshold constant is 3 — below yields Comment, at/above yields RequestChanges.
    let cases: &[(u32, ReviewState)] = &[
        (1, ReviewState::Comment),        // below threshold: COMMENT
        (2, ReviewState::Comment),        // below threshold: COMMENT
        (3, ReviewState::RequestChanges), // at threshold: REQUEST_CHANGES
        (4, ReviewState::RequestChanges), // above threshold: REQUEST_CHANGES
    ];
    for (count, expected_state) in cases {
        // Arrange
        let response = format!(
            "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: {count}\nSuggestions: 0"
        );
        // Act
        let (_verdict, state) = parse_verdict(&response).unwrap();
        // Assert
        assert_eq!(
            state, *expected_state,
            "ImportantIssues: {count} should yield {expected_state:?}"
        );
    }
}

#[test]
fn test_parse_metadata_block_empty_field_value_defaults_to_zero() {
    // Arrange: ImportantIssues field present but has no numeric value — should default to 0,
    // not return None (a missing count is treated as zero, not a parse failure).
    let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues:\nSuggestions: 0";
    // Act
    let verdict = parse_metadata_block(response).unwrap();
    // Assert: graceful default to 0
    assert_eq!(verdict.important_issues, 0);
    assert_eq!(verdict.verdict, "POSITIVE");
}

#[test]
fn test_parse_metadata_block_whitespace_field_value_defaults_to_zero() {
    // Arrange: ImportantIssues field has only whitespace after the colon.
    // The parser must trim before parsing, so "   " should not cause a parse error.
    let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues:   \nSuggestions: 0";
    // Act
    let verdict = parse_metadata_block(response).unwrap();
    // Assert: whitespace-only value treated identically to empty — defaults to 0
    assert_eq!(verdict.important_issues, 0);
    assert_eq!(verdict.verdict, "POSITIVE");
}

#[test]
fn test_review_state_display() {
    // Arrange / Act / Assert: Display impl matches GitHub API event string
    assert_eq!(ReviewState::Approve.to_string(), "APPROVE");
    assert_eq!(ReviewState::RequestChanges.to_string(), "REQUEST_CHANGES");
    assert_eq!(ReviewState::Comment.to_string(), "COMMENT");
}

#[test]
fn test_parse_metadata_block_missing_important_issues_field_defaults_to_zero() {
    // Arrange: ImportantIssues field is completely absent (not just empty).
    // The relaxed parse policy must treat a missing count field as 0.
    let response = "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nSuggestions: 1";
    // Act
    let verdict = parse_metadata_block(response).unwrap();
    // Assert: absent field defaults gracefully to 0
    assert_eq!(verdict.important_issues, 0);
    assert_eq!(verdict.suggestions, 1);
    assert_eq!(verdict.verdict, "POSITIVE");
}

#[test]
fn test_verdict_display_includes_all_four_fields() {
    // Arrange
    let verdict = Verdict {
        verdict: "POSITIVE".to_string(),
        critical_issues: 1,
        security_issues: 0,
        important_issues: 2,
        suggestions: 3,
    };
    // Act
    let display = verdict.to_string();
    // Assert: all four counts appear in Display output with their labels
    assert!(
        display.contains("CriticalIssues: 1"),
        "critical count missing or malformed"
    );
    assert!(
        display.contains("ImportantIssues: 2"),
        "important count missing or malformed"
    );
    assert!(
        display.contains("Suggestions: 3"),
        "suggestions count missing or malformed"
    );
    assert!(
        display.contains("SecurityIssues: 0"),
        "security count missing or malformed"
    );
}
