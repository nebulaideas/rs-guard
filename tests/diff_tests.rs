use rs_guard::diff::{fetch_pr_diff, DiffLimits};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const VALID_DIFF: &str = "diff --git a/file.rs b/file.rs\n--- a/file.rs\n+++ b/file.rs\n@@ -1,2 +1,3 @@\n+line1\n+line2\n line3";

#[tokio::test]
async fn test_fetch_diff_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/pulls/42"))
        .and(header("Accept", "application/vnd.github.v3.diff"))
        .and(header("Authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_string(VALID_DIFF))
        .mount(&mock_server)
        .await;

    let result = fetch_pr_diff(
        &mock_server.uri(),
        "test-owner",
        "test-repo",
        42,
        "test-token",
        DiffLimits::default(),
    )
    .await;
    assert!(result.is_ok());

    let diff = result.unwrap();
    assert_eq!(diff.line_count, 7);
    assert!(diff.content.contains("diff --git"));
}

#[tokio::test]
async fn test_fetch_diff_rate_limited_then_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/pulls/42"))
        .respond_with(ResponseTemplate::new(429).set_body_string("Rate limited"))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/pulls/42"))
        .respond_with(ResponseTemplate::new(200).set_body_string(VALID_DIFF))
        .mount(&mock_server)
        .await;

    let result = fetch_pr_diff(
        &mock_server.uri(),
        "test-owner",
        "test-repo",
        42,
        "test-token",
        DiffLimits::default(),
    )
    .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_fetch_diff_not_found() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/pulls/999"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .mount(&mock_server)
        .await;

    let result = fetch_pr_diff(
        &mock_server.uri(),
        "test-owner",
        "test-repo",
        999,
        "test-token",
        DiffLimits::default(),
    )
    .await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("404"));
}

#[tokio::test]
async fn test_fetch_diff_empty() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/pulls/42"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .mount(&mock_server)
        .await;

    let result = fetch_pr_diff(
        &mock_server.uri(),
        "test-owner",
        "test-repo",
        42,
        "test-token",
        DiffLimits::default(),
    )
    .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("No diff content"));
}

#[tokio::test]
async fn test_fetch_diff_rejects_json_body() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/pulls/42"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(r#"{"message": "Internal Server Error"}"#),
        )
        .mount(&mock_server)
        .await;

    let result = fetch_pr_diff(
        &mock_server.uri(),
        "test-owner",
        "test-repo",
        42,
        "test-token",
        DiffLimits::default(),
    )
    .await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("not appear to be a diff"));
}

// --- .rs-guardignore integration tests ---

#[test]
fn test_filter_diff_by_paths_with_ignore_patterns() {
    use rs_guard::diff::filter_diff_by_paths_with_ignore;
    let content = "diff --git a/Cargo.lock b/Cargo.lock\n--- a/Cargo.lock\n+++ b/Cargo.lock\n@@ -1 +1,2 @@\n+foo\ndiff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1,2 @@\n+bar\n";
    let ignore = vec!["Cargo.lock".to_string()];
    let filtered = filter_diff_by_paths_with_ignore(content, &[], &[], &ignore);
    assert!(!filtered.contains("Cargo.lock"));
    assert!(filtered.contains("src/main.rs"));
}

#[test]
fn test_filter_diff_by_paths_with_ignore_directory_pattern() {
    use rs_guard::diff::filter_diff_by_paths_with_ignore;
    let content = "diff --git a/vendor/lib.rs b/vendor/lib.rs\n--- a/vendor/lib.rs\n+++ b/vendor/lib.rs\n@@ -1 +1,2 @@\n+x\ndiff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1,2 @@\n+y\n";
    let ignore = vec!["vendor/".to_string()];
    let filtered = filter_diff_by_paths_with_ignore(content, &[], &[], &ignore);
    assert!(!filtered.contains("vendor/lib.rs"));
    assert!(filtered.contains("src/main.rs"));
}

#[test]
fn test_filter_diff_by_paths_with_ignore_negation() {
    use rs_guard::diff::filter_diff_by_paths_with_ignore;
    let content = "diff --git a/vendor/lib.rs b/vendor/lib.rs\n--- a/vendor/lib.rs\n+++ b/vendor/lib.rs\n@@ -1 +1,2 @@\n+x\ndiff --git a/vendor/keep.rs b/vendor/keep.rs\n--- a/vendor/keep.rs\n+++ b/vendor/keep.rs\n@@ -1 +1,2 @@\n+y\n";
    // Ignore vendor/ but un-ignore vendor/keep.rs
    let ignore = vec!["vendor/".to_string(), "!vendor/keep.rs".to_string()];
    let filtered = filter_diff_by_paths_with_ignore(content, &[], &[], &ignore);
    assert!(!filtered.contains("vendor/lib.rs"));
    assert!(filtered.contains("vendor/keep.rs"));
}

#[test]
fn test_filter_diff_by_paths_ignore_empty_is_noop() {
    use rs_guard::diff::filter_diff_by_paths_with_ignore;
    let content = "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1,2 @@\n+x\n";
    let filtered = filter_diff_by_paths_with_ignore(content, &[], &[], &[]);
    assert_eq!(filtered, content);
}

#[test]
fn test_parse_rs_guard_ignore_integration() {
    use rs_guard::diff::parse_rs_guard_ignore;
    let content = "# This is a comment\n\nCargo.lock\nnode_modules/\n!keep.me\n  \n";
    let patterns = parse_rs_guard_ignore(content);
    assert_eq!(patterns.len(), 3);
    assert_eq!(patterns[0], "Cargo.lock");
    assert_eq!(patterns[1], "node_modules/");
    assert_eq!(patterns[2], "!keep.me");
}
