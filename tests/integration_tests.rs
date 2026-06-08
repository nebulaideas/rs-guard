//! Full pipeline integration tests with mock GitHub and mock LLM servers.
//!
//! Tests [`run_pipeline`] end-to-end, verifying that the orchestration
//! correctly sequences diff fetching, LLM calling, verdict parsing, and
//! review submission.

use diffguard::config::Config;
use diffguard::pipeline::{run_pipeline, PipelineResult};
use serde_json::json;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Builds a minimal Config for CI-mode integration tests.
fn ci_config(pr_number: u64, provider: &str, api_key: &str) -> Config {
    let mut c = Config::empty();
    c.is_ci = true;
    c.pr_number = Some(pr_number);
    c.repo_owner = Some("test-owner".into());
    c.repo_name = Some("test-repo".into());
    c.github_token = Some(api_key.into());
    c.provider = provider.into();
    c.model = "test-model".into();
    c.temperature = 0.1;
    c.prompt = "You are a code reviewer.".into();
    c.api_key = "test-llm-key".into();
    c
}

/// Builds a minimal Config for local-mode integration tests.
fn local_config() -> Config {
    let mut c = Config::empty();
    c.is_ci = false;
    c.provider = "deepseek".into();
    c.model = "test-model".into();
    c.temperature = 0.1;
    c.prompt = "You are a code reviewer.".into();
    c.api_key = "test-llm-key".into();
    c
}

const VALID_DIFF: &str =
    "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1 +1,2 @@\n+line1\n line0";

const POSITIVE_RESPONSE: &str = "Looks good.\n\n[DIFFGUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 0\nSecurityIssues: 0";

const NEGATIVE_RESPONSE: &str = "Found issues.\n\n[DIFFGUARD_VERDICT_METADATA]\nVerdict: NEGATIVE\nCriticalBugs: 2\nSecurityIssues: 1";

#[tokio::test]
async fn test_full_pipeline_ci_approve() {
    let github = MockServer::start().await;
    let llm = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+"))
        .respond_with(ResponseTemplate::new(200).set_body_string(VALID_DIFF))
        .mount(&github)
        .await;

    Mock::given(method("POST"))
        .and(path_regex(r"/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": POSITIVE_RESPONSE}}]
        })))
        .mount(&llm)
        .await;

    Mock::given(method("POST"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+/reviews"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&github)
        .await;

    let mut config = ci_config(42, "deepseek", "test-token");
    config.github_base_url = github.uri();
    config.provider_config.base_url = Some(llm.uri());
    config.no_cache = true; // Disable cache to avoid conflicts

    let result = run_pipeline(config, None).await;
    assert!(matches!(result, Ok(PipelineResult::Success)));
}

#[tokio::test]
async fn test_full_pipeline_ci_request_changes() {
    let github = MockServer::start().await;
    let llm = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+"))
        .respond_with(ResponseTemplate::new(200).set_body_string(VALID_DIFF))
        .mount(&github)
        .await;

    Mock::given(method("POST"))
        .and(path_regex(r"/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": NEGATIVE_RESPONSE}}]
        })))
        .mount(&llm)
        .await;

    // REQUEST_CHANGES submission, but no dismissal (state is blocking)
    Mock::given(method("POST"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+/reviews"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&github)
        .await;

    let mut config = ci_config(42, "deepseek", "test-token");
    config.github_base_url = github.uri();
    config.provider_config.base_url = Some(llm.uri());
    config.no_cache = true; // Disable cache to avoid conflicts

    let result = run_pipeline(config, None).await;
    assert!(matches!(result, Ok(PipelineResult::Success)));
}

#[tokio::test]
async fn test_full_pipeline_ci_dismisses_previous_reviews() {
    let github = MockServer::start().await;
    let llm = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+"))
        .respond_with(ResponseTemplate::new(200).set_body_string(VALID_DIFF))
        .mount(&github)
        .await;

    Mock::given(method("POST"))
        .and(path_regex(r"/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": POSITIVE_RESPONSE}}]
        })))
        .mount(&llm)
        .await;

    // APPROVE submission succeeds
    Mock::given(method("POST"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+/reviews"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&github)
        .await;

    // Dismissal query returns a bot review
    Mock::given(method("GET"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+/reviews"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": 1,
            "state": "CHANGES_REQUESTED",
            "body": "Previous review\n\n<!-- diffguard-bot -->"
        }])))
        .mount(&github)
        .await;

    // Dismissal succeeds
    Mock::given(method("PUT"))
        .and(path_regex(
            r"/repos/test-owner/test-repo/pulls/\d+/reviews/\d+/dismissals",
        ))
        .respond_with(ResponseTemplate::new(200))
        .mount(&github)
        .await;

    let mut config = ci_config(42, "deepseek", "test-token");
    config.github_base_url = github.uri();
    config.provider_config.base_url = Some(llm.uri());
    config.no_cache = true; // Disable cache to avoid conflicts

    let result = run_pipeline(config, None).await;
    assert!(matches!(result, Ok(PipelineResult::Success)));
}

#[tokio::test]
async fn test_full_pipeline_local_approve() {
    let llm = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path_regex(r"/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": POSITIVE_RESPONSE}}]
        })))
        .mount(&llm)
        .await;

    let mut config = local_config();
    config.provider_config.base_url = Some(llm.uri());
    config.no_cache = true; // Disable cache to avoid conflicts

    let dir = tempfile::tempdir().unwrap();
    let diff_path = dir.path().join("test.diff");
    std::fs::write(&diff_path, VALID_DIFF).unwrap();

    let result = run_pipeline(config, Some(diff_path.to_str().unwrap())).await;
    assert!(matches!(result, Ok(PipelineResult::Success)));
}

#[tokio::test]
async fn test_full_pipeline_empty_diff() {
    let github = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .mount(&github)
        .await;

    let mut config = ci_config(42, "deepseek", "test-token");
    config.github_base_url = github.uri();
    config.no_cache = true; // Disable cache to avoid conflicts

    let result = run_pipeline(config, None).await;
    assert!(matches!(result, Ok(PipelineResult::Success)));
}

#[tokio::test]
#[serial_test::serial]
async fn test_full_pipeline_cache_hit() {
    // Clear cache before this test to ensure clean state
    let cache_dir = std::path::Path::new(".diffguard/cache");
    if cache_dir.exists() {
        let _ = std::fs::remove_dir_all(cache_dir);
    }

    let github = MockServer::start().await;
    let llm = MockServer::start().await;

    // Use unique diff content to avoid cache collisions with other tests
    let unique_diff = format!(
        "diff --git a/unique{}.rs b/unique{}.rs\n+line{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        42
    );

    Mock::given(method("GET"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&unique_diff))
        .mount(&github)
        .await;

    Mock::given(method("POST"))
        .and(path_regex(r"/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": POSITIVE_RESPONSE}}]
        })))
        .expect(1) // Should only be called once (first run)
        .mount(&llm)
        .await;

    Mock::given(method("POST"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+/reviews"))
        .respond_with(ResponseTemplate::new(200))
        .expect(2) // Should be called twice (two runs)
        .mount(&github)
        .await;

    let mut config1 = ci_config(42, "deepseek", "test-token");
    config1.github_base_url = github.uri();
    config1.provider_config.base_url = Some(llm.uri());

    // First run - should call LLM
    let result1 = run_pipeline(config1, None).await;
    assert!(matches!(result1, Ok(PipelineResult::Success)));

    // Second run - should use cache
    let mut config2 = ci_config(42, "deepseek", "test-token");
    config2.github_base_url = github.uri();
    config2.provider_config.base_url = Some(llm.uri());

    let result2 = run_pipeline(config2, None).await;
    assert!(matches!(result2, Ok(PipelineResult::Success)));

    // Verify LLM was only called once (cache hit on second run)
    // The mock's expect(1) will fail if called more than once
}

#[tokio::test]
async fn test_full_pipeline_chunked_diff() {
    let github = MockServer::start().await;
    let llm = MockServer::start().await;

    // Generate a large diff (20 valid unified diff blocks)
    let large_diff: String = (0..20)
        .map(|i| {
            format!(
                "diff --git a/file{}.rs b/file{}.rs\n--- a/file{}.rs\n+++ b/file{}.rs\n@@ -1,1 +1,1 @@\n-old line {}\n+new line {}\n",
                i, i, i, i, i, i
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    Mock::given(method("GET"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+"))
        .respond_with(ResponseTemplate::new(200).set_body_string(large_diff))
        .mount(&github)
        .await;

    Mock::given(method("POST"))
        .and(path_regex(r"/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": POSITIVE_RESPONSE}}]
        })))
        .mount(&llm)
        .await;

    Mock::given(method("POST"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+/reviews"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&github)
        .await;

    let mut config = ci_config(42, "deepseek", "test-token");
    config.github_base_url = github.uri();
    config.provider_config.base_url = Some(llm.uri());
    config.no_cache = true; // Disable cache to avoid conflicts

    let result = run_pipeline(config, None).await;
    assert!(matches!(result, Ok(PipelineResult::Success)));

    // The pipeline should have chunked the diff and added a warning
    // We can't easily verify the warning was added to the review body
    // without inspecting the mock server's request history
}

#[tokio::test]
#[serial_test::serial]
async fn test_full_pipeline_metrics_file_created() {
    let github = MockServer::start().await;
    let llm = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+"))
        .respond_with(ResponseTemplate::new(200).set_body_string(VALID_DIFF))
        .mount(&github)
        .await;

    Mock::given(method("POST"))
        .and(path_regex(r"/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": POSITIVE_RESPONSE}}]
        })))
        .mount(&llm)
        .await;

    Mock::given(method("POST"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+/reviews"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&github)
        .await;

    let mut config = ci_config(42, "deepseek", "test-token");
    config.github_base_url = github.uri();
    config.provider_config.base_url = Some(llm.uri());
    config.no_cache = true; // Disable cache to avoid conflicts

    // Use temp file for metrics to ensure cleanup
    let metrics_file = tempfile::NamedTempFile::new().unwrap();
    let metrics_path = metrics_file.path();
    std::env::set_var("DIFFGUARD_METRICS_PATH", metrics_path);

    let result = run_pipeline(config, None).await;
    assert!(matches!(result, Ok(PipelineResult::Success)));

    // Verify metrics file contains expected fields
    let content = std::fs::read_to_string(metrics_path).unwrap();
    assert!(content.contains("provider"));
    assert!(content.contains("tokens_in"));
    assert!(content.contains("tokens_out"));
    assert!(content.contains("latency_secs"));
    assert!(content.contains("estimated_cost_cents"));

    // Temp file is automatically cleaned up on drop
    std::env::remove_var("DIFFGUARD_METRICS_PATH");
}

#[tokio::test]
async fn test_full_pipeline_local_blocked() {
    let llm = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path_regex(r"/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": NEGATIVE_RESPONSE}}]
        })))
        .mount(&llm)
        .await;

    let mut config = local_config();
    config.provider_config.base_url = Some(llm.uri());
    config.no_cache = true; // Disable cache to avoid conflicts

    let dir = tempfile::tempdir().unwrap();
    let diff_path = dir.path().join("test.diff");
    std::fs::write(&diff_path, VALID_DIFF).unwrap();

    let result = run_pipeline(config, Some(diff_path.to_str().unwrap())).await;
    assert!(matches!(result, Ok(PipelineResult::ReviewBlocked)));
}

#[tokio::test]
async fn test_full_pipeline_llm_retries_exhausted() {
    let github = MockServer::start().await;
    let llm = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+"))
        .respond_with(ResponseTemplate::new(200).set_body_string(VALID_DIFF))
        .mount(&github)
        .await;

    // LLM server returns 500 errors - this will trigger retries with exponential backoff and eventually fail
    Mock::given(method("POST"))
        .and(path_regex(r"/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&llm)
        .await;

    let mut config = ci_config(42, "deepseek", "test-token");
    config.github_base_url = github.uri();
    config.provider_config.base_url = Some(llm.uri());
    config.no_cache = true; // Disable cache to avoid conflicts

    // The call should fail after retries due to repeated 500 errors
    let result = run_pipeline(config, None).await;
    assert!(result.is_err());
}
