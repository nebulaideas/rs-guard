use diffguard::diff::fetch_pr_diff;
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
    )
    .await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("not appear to be a diff"));
}
