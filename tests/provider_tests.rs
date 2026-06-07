use diffguard::llm::deepseek::DeepSeekClient;
use diffguard::llm::factory::create_provider;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_deepseek_provider_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "content": "Good code.\n\n[DIFFGUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 0\nSecurityIssues: 0"
                }
            }]
        })))
        .mount(&mock_server)
        .await;

    let client = DeepSeekClient::new("test-key")
        .unwrap()
        .with_base_url(mock_server.uri());
    let result = client
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap().contains("POSITIVE"));
}

#[tokio::test]
async fn test_deepseek_provider_via_factory() {
    let provider = create_provider("deepseek", "test-key");
    assert!(provider.is_ok());
}

#[test]
fn test_unknown_provider_fails() {
    let result = create_provider("unknown", "test-key");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Unknown provider"));
}
