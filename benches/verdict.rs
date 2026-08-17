//! Benchmarks for verdict parsing performance.
//!
//! Measures throughput of metadata block parsing, tag-based fallback,
//! structured findings parsing, findings-to-verdict merging, and review
//! state determination — the hottest path in the pipeline.

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use rs_guard::verdict::{
    determine_review_state, evaluate_by_tags, parse_findings, parse_metadata_block, parse_verdict,
    strip_findings_json, Finding, FindingSeverity, ReviewState, Verdict,
};

fn bench_parse_metadata_block(c: &mut Criterion) {
    let response = "Some review text with reasonable length.\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";

    c.bench_function("parse_metadata_block", |b| {
        b.iter(|| black_box(parse_metadata_block(black_box(response))))
    });
}

fn bench_evaluate_by_tags(c: &mut Criterion) {
    let response = "Found [Critical Bug] null pointer and [Security] SQL injection. Also [Critical] race condition.";

    c.bench_function("evaluate_by_tags", |b| {
        b.iter(|| black_box(evaluate_by_tags(black_box(response))))
    });
}

fn bench_parse_no_metadata(c: &mut Criterion) {
    let response =
        "Everything looks good. No issues found in this PR. The code is clean and well-structured.";

    c.bench_function("parse_no_metadata_fallback", |b| {
        b.iter(|| {
            black_box(
                parse_metadata_block(black_box(response))
                    .unwrap_or_else(|| evaluate_by_tags(black_box(response))),
            )
        })
    });
}

fn bench_determine_review_state(c: &mut Criterion) {
    let verdict = Verdict {
        verdict: "POSITIVE".to_string(),
        critical_issues: 0,
        security_issues: 0,
        important_issues: 0,
        suggestions: 0,
        findings: Vec::new(),
    };

    c.bench_function("determine_review_state", |b| {
        b.iter(|| black_box(determine_review_state(black_box(&verdict), black_box(3))))
    });
}

fn bench_large_diff_parsing(c: &mut Criterion) {
    // Simulate a large LLM response (~10KB) without findings
    let body = "Detailed review paragraph explaining code quality, patterns, and architecture.\n"
        .repeat(130);
    let response = format!(
        "Review:\n{}\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0",
        body
    );

    c.bench_function("parse_large_response", |b| {
        b.iter(|| black_box(parse_metadata_block(black_box(&response))))
    });
}

fn bench_parse_verdict(c: &mut Criterion) {
    let response = "Review summary and findings.\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";

    c.bench_function("parse_verdict", |b| {
        b.iter(|| black_box(parse_verdict(black_box(response), black_box(3))))
    });
}

fn bench_parse_findings(c: &mut Criterion) {
    let findings: Vec<String> = (1..=50)
        .map(|i| {
            format!(
                r#"{{"path":"src/file_{}.rs","line":{},"severity":"Suggestion","message":"Potential improvement #{}"}}"#,
                i, i * 10, i
            )
        })
        .collect();
    let response = format!(
        "Review prose here.\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 50\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{}]",
        findings.join(",")
    );

    c.bench_function("parse_findings_50", |b| {
        b.iter(|| black_box(parse_findings(black_box(&response))))
    });
}

fn bench_strip_findings_json(c: &mut Criterion) {
    let body = "review paragraph line\n".repeat(200);
    let response = format!(
        "Review:\n{}\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{{\"path\":\"src/lib.rs\",\"line\":1,\"severity\":\"Suggestion\",\"message\":\"nit\"}}]",
        body
    );

    c.bench_function("strip_findings_json", |b| {
        b.iter(|| black_box(strip_findings_json(black_box(&response))))
    });
}

fn bench_merge_with_findings(c: &mut Criterion) {
    let base_verdict = parse_metadata_block(
        "[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 1\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0",
    )
    .expect("metadata block should parse");

    let findings: Vec<Finding> = (1..=20)
        .map(|i| Finding {
            path: format!("src/module_{}.rs", i),
            line: i * 5,
            severity: FindingSeverity::Suggestion,
            message: format!("Refactor recommendation #{i}"),
            suggestion: None,
        })
        .collect();

    // Sanity check: max-rule merge preserves preliminary critical issues outside timed loop
    let sample_merged = Verdict::merge_with_findings(&base_verdict, findings.clone(), 3);
    assert_eq!(sample_merged.critical_issues, 1);
    assert_eq!(sample_merged.verdict, "NEGATIVE");
    assert_eq!(
        determine_review_state(&sample_merged, 3),
        ReviewState::RequestChanges
    );

    c.bench_function("merge_with_findings_20", |b| {
        b.iter_batched(
            || findings.clone(),
            |f| {
                black_box(Verdict::merge_with_findings(
                    black_box(&base_verdict),
                    black_box(f),
                    black_box(3),
                ))
            },
            BatchSize::SmallInput,
        )
    });
}

fn bench_from_findings(c: &mut Criterion) {
    let findings: Vec<Finding> = (1..=50)
        .map(|i| {
            let severity = match i % 4 {
                0 => FindingSeverity::Critical,
                1 => FindingSeverity::Security,
                2 => FindingSeverity::Important,
                _ => FindingSeverity::Suggestion,
            };
            Finding {
                path: format!("src/file_{}.rs", i),
                line: i * 2,
                severity,
                message: format!("Issue description #{i}"),
                suggestion: Some("Recommended remediation".to_string()),
            }
        })
        .collect();

    // Sanity check: from_findings blocks when critical/security issues are present
    let sample = Verdict::from_findings("POSITIVE", findings.clone(), 3);
    assert_eq!(sample.verdict, "NEGATIVE");
    assert!(sample.critical_issues > 0);
    assert_eq!(sample.findings.len(), 50);

    c.bench_function("from_findings_50", |b| {
        b.iter_batched(
            || findings.clone(),
            |f| {
                black_box(Verdict::from_findings(
                    black_box("POSITIVE"),
                    black_box(f),
                    black_box(3),
                ))
            },
            BatchSize::SmallInput,
        )
    });
}

fn bench_parse_large_response_with_findings(c: &mut Criterion) {
    let body = "Detailed review paragraph explaining code quality, patterns, and architecture.\n"
        .repeat(130);
    let findings: Vec<String> = (1..=50)
        .map(|i| {
            format!(
                r#"{{"path":"src/component_{}.rs","line":{},"severity":"Important","message":"Boundary check missing #{}"}}"#,
                i, i * 4, i
            )
        })
        .collect();
    let response = format!(
        "Review:\n{}\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0\n\n[RS_GUARD_VERDICT_FINDINGS]\n[{}]",
        body,
        findings.join(",")
    );

    c.bench_function("parse_large_response_with_findings", |b| {
        b.iter(|| black_box(parse_verdict(black_box(&response), black_box(3))))
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(3))
        .measurement_time(std::time::Duration::from_secs(10));
    targets = bench_parse_metadata_block,
              bench_evaluate_by_tags,
              bench_parse_no_metadata,
              bench_determine_review_state,
              bench_large_diff_parsing,
              bench_parse_verdict,
              bench_parse_findings,
              bench_strip_findings_json,
              bench_merge_with_findings,
              bench_from_findings,
              bench_parse_large_response_with_findings,
}
criterion_main!(benches);
