//! Full pipeline integration tests with mock GitHub and mock LLM servers.
//!
//! Tests [`run_pipeline`] end-to-end, verifying that the orchestration
//! correctly sequences diff fetching, LLM calling, verdict parsing, and
//! review submission.

use rs_guard::config::Config;
use rs_guard::pipeline::{run_pipeline, PipelineResult};
use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path_regex};
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

const POSITIVE_RESPONSE: &str = "Looks good.\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0";

const NEGATIVE_RESPONSE: &str = "Found issues.\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: NEGATIVE\nCriticalIssues: 2\nSecurityIssues: 1\nImportantIssues: 0\nSuggestions: 0";

/// LLM response with 2 important issues and no critical/security — should yield COMMENT.
const IMPORTANT_ISSUES_RESPONSE: &str = "Review complete.\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 2\nSuggestions: 1";

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
            "body": "Previous review\n\n<!-- rs-guard-bot -->"
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

/// Builds a valid diff body that exceeds the 1,500-line pipeline limit.
fn oversized_diff_body() -> String {
    let mut body = String::from("diff --git a/big.rs b/big.rs\n--- a/big.rs\n+++ b/big.rs\n");
    for i in 0..1_600 {
        body.push_str(&format!("+line {i}\n"));
    }
    body
}

#[tokio::test]
async fn test_full_pipeline_ci_diff_too_large_submits_comment() {
    let github = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+"))
        .respond_with(ResponseTemplate::new(200).set_body_string(oversized_diff_body()))
        .mount(&github)
        .await;

    Mock::given(method("POST"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+/reviews"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&github)
        .await;

    let mut config = ci_config(42, "deepseek", "test-token");
    config.github_base_url = github.uri();
    config.no_cache = true;
    // Keep this test independent of the raised default hard limits (5000 lines).
    config.max_diff_lines = 1500;
    config.max_diff_bytes = 100 * 1024;

    let result = run_pipeline(config, None).await;
    assert!(
        matches!(result, Ok(PipelineResult::Success)),
        "expected Success on DiffTooLarge CI path, got: {result:?}"
    );
}

#[tokio::test]
async fn test_full_pipeline_empty_diff_file() {
    let dir = tempfile::tempdir().unwrap();
    let diff_path = dir.path().join("empty.diff");
    std::fs::write(&diff_path, "").unwrap();

    let config = local_config();
    let result = run_pipeline(config, Some(diff_path.to_str().unwrap())).await;
    assert!(matches!(result, Ok(PipelineResult::Success)));
}

#[tokio::test]
async fn test_full_pipeline_with_variant_deepseek_pro() {
    let github = MockServer::start().await;
    let llm = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+"))
        .respond_with(ResponseTemplate::new(200).set_body_string(VALID_DIFF))
        .mount(&github)
        .await;

    // Exercise the variant: "pro" should resolve to deepseek-v4-pro via apply_variant + ModelAlias
    Mock::given(method("POST"))
        .and(path_regex(r"/chat/completions"))
        .and(body_partial_json(json!({"model": "deepseek-v4-pro"})))
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
    config.variant = Some("pro".to_string());
    config.provider_config.variant = Some("pro".to_string());
    config.no_cache = true; // Disable cache to avoid conflicts

    let result = run_pipeline(config, None).await;
    assert!(matches!(result, Ok(PipelineResult::Success)));
}

#[tokio::test]
#[serial_test::serial]
async fn test_full_pipeline_cache_hit() {
    // Clear cache before this test to ensure clean state
    let cache_dir = std::path::Path::new(".rs-guard/cache");
    if cache_dir.exists() {
        let _ = std::fs::remove_dir_all(cache_dir);
    }

    let github = MockServer::start().await;
    let llm = MockServer::start().await;

    // Use unique diff content to avoid cache collisions with other tests
    let unique_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let unique_diff = format!("diff --git a/unique{unique_id}.rs b/unique{unique_id}.rs\n+line42");

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
    std::env::set_var("RS_GUARD_METRICS_PATH", metrics_path);

    let result = run_pipeline(config, None).await;
    assert!(matches!(result, Ok(PipelineResult::Success)));

    // Verify metrics file contains expected fields
    let content = std::fs::read_to_string(metrics_path).unwrap();
    assert!(content.contains("provider"));
    assert!(content.contains("estimated_tokens_in"));
    assert!(content.contains("estimated_tokens_out"));
    assert!(content.contains("latency_secs"));
    assert!(content.contains("estimated_cost_cents"));

    // Temp file is automatically cleaned up on drop
    std::env::remove_var("RS_GUARD_METRICS_PATH");
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

#[tokio::test]
async fn test_full_pipeline_ci_important_issues_yield_comment_not_blocked() {
    // Arrange: LLM returns 2 important issues (below the 3-issue REQUEST_CHANGES threshold).
    // The pipeline should succeed (COMMENT state is not a ReviewBlocked result).
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
            "choices": [{"message": {"content": IMPORTANT_ISSUES_RESPONSE}}]
        })))
        .mount(&llm)
        .await;

    // COMMENT review submission
    Mock::given(method("POST"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+/reviews"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&github)
        .await;

    let mut config = ci_config(42, "deepseek", "test-token");
    config.github_base_url = github.uri();
    config.provider_config.base_url = Some(llm.uri());
    config.no_cache = true;

    let result = run_pipeline(config, None).await;
    // Assert: pipeline succeeds — important issues produce COMMENT, not ReviewBlocked
    assert!(matches!(result, Ok(PipelineResult::Success)));

    // Assert: the review POST body sent to GitHub contains event=COMMENT, not APPROVE or
    // REQUEST_CHANGES — verifying that the correct review state was actually submitted.
    let requests = github.received_requests().await.unwrap_or_default();
    let review_request = requests
        .iter()
        .find(|r| r.method == wiremock::http::Method::POST && r.url.path().ends_with("/reviews"))
        .expect("expected a POST to /reviews");
    let body: serde_json::Value =
        serde_json::from_slice(&review_request.body).expect("review body is valid JSON");
    assert_eq!(
        body["event"].as_str(),
        Some("COMMENT"),
        "expected COMMENT event, got: {}",
        body["event"]
    );
}

// ============================================================================
// Grok and GLM full-pipeline integration tests (F8)
// ============================================================================
//
// These tests verify that the new first-class providers (grok, glm) work
// end-to-end through the full pipeline, not just at the factory level.

#[tokio::test]
async fn test_full_pipeline_grok_approve() {
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

    let mut config = ci_config(42, "grok", "test-token");
    config.github_base_url = github.uri();
    config.provider_config.base_url = Some(llm.uri());
    config.no_cache = true;

    let result = run_pipeline(config, None).await;
    assert!(matches!(result, Ok(PipelineResult::Success)));
}

#[tokio::test]
async fn test_full_pipeline_empty_content_retried_then_succeeds() {
    let github = MockServer::start().await;
    let llm = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+"))
        .respond_with(ResponseTemplate::new(200).set_body_string(VALID_DIFF))
        .mount(&github)
        .await;

    // First call: null content (DeepSeek thinking shape) — retryable
    Mock::given(method("POST"))
        .and(path_regex(r"/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "content": null,
                    "reasoning_content": "long internal reasoning"
                }
            }]
        })))
        .up_to_n_times(1)
        .mount(&llm)
        .await;

    // Second call: valid review
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
    config.no_cache = true;

    let result = run_pipeline(config, None).await;
    assert!(
        matches!(result, Ok(PipelineResult::Success)),
        "expected retry after null content to succeed, got: {:?}",
        result
    );
}

#[tokio::test]
#[serial_test::serial]
async fn test_full_pipeline_empty_content_not_cached_on_failure() {
    let temp_dir = tempfile::tempdir().unwrap();
    let cache_path = temp_dir.path().join("cache");

    let github = MockServer::start().await;
    let llm = MockServer::start().await;

    let unique_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let unique_diff =
        format!("diff --git a/empty-cache{unique_id}.rs b/empty-cache{unique_id}.rs\n+line99");

    Mock::given(method("GET"))
        .and(path_regex(r"/repos/test-owner/test-repo/pulls/\d+"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&unique_diff))
        .mount(&github)
        .await;

    // Always return null content — exhaust retries, pipeline must fail
    Mock::given(method("POST"))
        .and(path_regex(r"/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "content": null,
                    "reasoning_content": "reasoning only"
                }
            }]
        })))
        .mount(&llm)
        .await;

    let mut config = ci_config(42, "deepseek", "test-token");
    config.github_base_url = github.uri();
    config.provider_config.base_url = Some(llm.uri());
    config.no_cache = false;
    config.cache_dir = Some(cache_path.to_string_lossy().into_owned());

    let result = run_pipeline(config.clone(), None).await;
    assert!(
        result.is_err(),
        "expected pipeline failure after empty retries"
    );

    // Cache must not contain a poisoned empty entry
    let cache = rs_guard::cache::DiffCache::new(rs_guard::cache::CacheConfig {
        enabled: true,
        cache_dir: cache_path,
        ..rs_guard::cache::CacheConfig::default()
    })
    .unwrap();
    let cached = cache.get(
        &unique_diff,
        &config.prompt,
        &config.provider,
        &config.model,
        config.variant.as_deref(),
        config.temperature,
        config.provider_config.base_url.as_deref().unwrap_or(""),
        config.provider_config.max_tokens,
        config.provider_config.result_format.as_deref(),
    );
    assert!(
        cached.is_none(),
        "empty/failed responses must not be cached"
    );
}

#[tokio::test]
async fn test_full_pipeline_glm_approve() {
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

    let mut config = ci_config(42, "glm", "test-token");
    config.github_base_url = github.uri();
    config.provider_config.base_url = Some(llm.uri());
    config.no_cache = true;

    let result = run_pipeline(config, None).await;
    assert!(matches!(result, Ok(PipelineResult::Success)));
}
