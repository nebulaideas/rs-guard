//! Benchmarks for verdict parsing performance.
//!
//! Measures throughput of metadata block parsing, tag-based fallback,
//! and review state determination — the hottest path in the pipeline.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rs_guard::verdict::{determine_review_state, evaluate_by_tags, parse_metadata_block, Verdict};

fn bench_parse_metadata_block(c: &mut Criterion) {
    let response = "Some review text with reasonable length.\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";

    c.bench_function("parse_metadata_block", |b| {
        b.iter(|| parse_metadata_block(black_box(response)))
    });
}

fn bench_evaluate_by_tags(c: &mut Criterion) {
    let response = "Found [Critical Bug] null pointer and [Security] SQL injection. Also [Critical] race condition.";

    c.bench_function("evaluate_by_tags", |b| {
        b.iter(|| evaluate_by_tags(black_box(response)))
    });
}

fn bench_parse_no_metadata(c: &mut Criterion) {
    let response =
        "Everything looks good. No issues found in this PR. The code is clean and well-structured.";

    c.bench_function("parse_no_metadata_fallback", |b| {
        b.iter(|| {
            parse_metadata_block(black_box(response))
                .unwrap_or_else(|| evaluate_by_tags(black_box(response)))
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
    };

    c.bench_function("determine_review_state", |b| {
        b.iter(|| determine_review_state(black_box(&verdict), black_box(3)))
    });
}

fn bench_large_diff_parsing(c: &mut Criterion) {
    // Simulate a large LLM response (~10KB)
    let body = "line\n".repeat(500);
    let response = format!(
        "Review:\n{}\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0",
        body
    );

    c.bench_function("parse_large_response", |b| {
        b.iter(|| parse_metadata_block(black_box(&response)))
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
}
criterion_main!(benches);
