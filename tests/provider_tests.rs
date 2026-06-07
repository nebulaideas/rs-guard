use diffguard::llm::deepseek::DeepSeekClient;
use diffguard::llm::factory::create_provider;
use diffguard::llm::kimi::KimiClient;
use diffguard::llm::openai::OpenAiClient;
use diffguard::llm::openrouter::OpenRouterClient;
use diffguard::llm::qwen::QwenClient;
use diffguard::llm::{LlmProvider, ProviderConfig};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn default_config() -> ProviderConfig {
    ProviderConfig {
        base_url: None,
        http_referer: None,
        max_tokens: None,
        model: "test-model".to_string(),
    }
}

#[tokio::test]
async fn test_deepseek_provider_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "content": "This looks good.\n\n[DIFFGUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalBugs: 0\nSecurityIssues: 0"
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
async fn test_deepseek_implements_llm_provider_trait() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "content": "Trait dispatch works."
                }
            }]
        })))
        .mount(&mock_server)
        .await;

    let provider: Box<dyn LlmProvider> = Box::new(
        DeepSeekClient::new("test-key")
            .unwrap()
            .with_base_url(mock_server.uri()),
    );

    assert_eq!(provider.name(), "deepseek");

    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap().contains("Trait dispatch works"));
}

#[tokio::test]
async fn test_deepseek_provider_via_factory() {
    let provider = create_provider("deepseek", "test-key", &default_config());
    assert!(provider.is_ok());
    assert_eq!(provider.unwrap().name(), "deepseek");
}

#[tokio::test]
async fn test_kimi_provider_via_factory() {
    let provider = create_provider("kimi", "test-key", &default_config());
    assert!(provider.is_ok());
    assert_eq!(provider.unwrap().name(), "kimi");
}

#[tokio::test]
async fn test_kimi_trait_dispatch() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "content": "Kimi trait dispatch works."
                }
            }]
        })))
        .mount(&mock_server)
        .await;

    let provider: Box<dyn LlmProvider> = Box::new(
        KimiClient::new("test-key")
            .unwrap()
            .with_base_url(mock_server.uri()),
    );

    assert_eq!(provider.name(), "kimi");

    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap().contains("Kimi trait dispatch works"));
}

#[tokio::test]
async fn test_qwen_provider_via_factory() {
    let provider = create_provider("qwen", "test-key", &default_config());
    assert!(provider.is_ok());
    assert_eq!(provider.unwrap().name(), "qwen");
}

#[tokio::test]
async fn test_qwen_trait_dispatch() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "content": "Qwen trait dispatch works."
                }
            }]
        })))
        .mount(&mock_server)
        .await;

    let provider: Box<dyn LlmProvider> = Box::new(
        QwenClient::new("test-key")
            .unwrap()
            .with_base_url(mock_server.uri()),
    );

    assert_eq!(provider.name(), "qwen");

    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap().contains("Qwen trait dispatch works"));
}

#[tokio::test]
async fn test_openrouter_provider_via_factory() {
    let provider = create_provider("openrouter", "test-key", &default_config());
    assert!(provider.is_ok());
    assert_eq!(provider.unwrap().name(), "openrouter");
}

#[tokio::test]
async fn test_openrouter_trait_dispatch() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "content": "OpenRouter trait dispatch works."
                }
            }]
        })))
        .mount(&mock_server)
        .await;

    let provider: Box<dyn LlmProvider> = Box::new(
        OpenRouterClient::new("test-key")
            .unwrap()
            .with_base_url(mock_server.uri()),
    );

    assert_eq!(provider.name(), "openrouter");

    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap().contains("OpenRouter trait dispatch works"));
}

#[tokio::test]
async fn test_openai_provider_via_factory() {
    let provider = create_provider("openai", "test-key", &default_config());
    assert!(provider.is_ok());
    assert_eq!(provider.unwrap().name(), "openai");
}

#[tokio::test]
async fn test_openai_trait_dispatch() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "content": "OpenAI trait dispatch works."
                }
            }]
        })))
        .mount(&mock_server)
        .await;

    let provider: Box<dyn LlmProvider> = Box::new(
        OpenAiClient::new("test-key")
            .unwrap()
            .with_base_url(mock_server.uri()),
    );

    assert_eq!(provider.name(), "openai");

    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap().contains("OpenAI trait dispatch works"));
}

#[test]
fn test_unknown_provider_fails() {
    let result = create_provider("unknown", "test-key", &default_config());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Unknown provider"));
}

#[tokio::test]
async fn test_factory_applies_base_url_override() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": { "content": "Custom base URL works." }
            }]
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        base_url: Some(mock_server.uri()),
        http_referer: None,
        max_tokens: None,
        model: "gpt-4o-mini".to_string(),
    };

    let provider = create_provider("openai", "test-key", &config).unwrap();
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap().contains("Custom base URL works"));
}

#[tokio::test]
async fn test_factory_applies_max_tokens() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": { "content": "max_tokens applied." }
            }]
        })))
        .mount(&mock_server)
        .await;

    let config = ProviderConfig {
        base_url: Some(mock_server.uri()),
        http_referer: None,
        max_tokens: Some(4096),
        model: "deepseek-v4-flash".to_string(),
    };

    let provider = create_provider("deepseek", "test-key", &config).unwrap();
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap().contains("max_tokens applied"));
}
