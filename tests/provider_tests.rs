//! Provider integration tests.
//!
//! All providers are exercised exclusively through the
//! [`create_provider`] factory, which returns a `Box<dyn LlmProvider>` backed
//! by the generic OpenAI-compatible client. Direct per-client construction is
//! not part of the public surface.

use rs_guard::llm::factory::create_provider;
use rs_guard::llm::{LlmProvider, ProviderConfig};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn default_config() -> ProviderConfig {
    ProviderConfig {
        base_url: None,
        http_referer: None,
        max_tokens: None,
        model: "test-model".to_string(),
        variant: None,
    }
}

/// Builds a config pointed at the given mock server for `provider`.
fn config_at(provider: &str, server_uri: &str) -> ProviderConfig {
    ProviderConfig {
        base_url: Some(server_uri.to_string()),
        http_referer: None,
        max_tokens: None,
        model: provider_default_model(provider),
        variant: None,
    }
}

/// Resolves the provider's default model id (kept in sync with providers.rs).
fn provider_default_model(provider: &str) -> String {
    match provider {
        "deepseek" => "deepseek-v4-flash".to_string(),
        "kimi" => "kimi-k2.5".to_string(),
        "qwen" => "qwen-plus".to_string(),
        "openrouter" => "openai/gpt-4o-mini".to_string(),
        "openai" => "gpt-4o-mini".to_string(),
        "grok" => "grok-3".to_string(),
        "glm" => "glm-4".to_string(),
        _ => "test-model".to_string(),
    }
}

/// Mounts a minimal success mock returning `content`.
async fn mount_success(mock_server: &MockServer, content: &str) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{ "message": { "content": content } }]
        })))
        .mount(mock_server)
        .await;
}

#[tokio::test]
async fn test_deepseek_provider_success() {
    let mock_server = MockServer::start().await;
    mount_success(
        &mock_server,
        "This looks good.\n\n[RS_GUARD_VERDICT_METADATA]\nVerdict: POSITIVE\nCriticalIssues: 0\nSecurityIssues: 0\nImportantIssues: 0\nSuggestions: 0",
    )
    .await;

    let provider = create_provider(
        "deepseek",
        "test-key",
        &config_at("deepseek", &mock_server.uri()),
    )
    .unwrap();
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("POSITIVE"));
}

#[tokio::test]
async fn test_deepseek_trait_dispatch() {
    let mock_server = MockServer::start().await;
    mount_success(&mock_server, "Trait dispatch works.").await;

    let provider: Box<dyn LlmProvider> = create_provider(
        "deepseek",
        "test-key",
        &config_at("deepseek", &mock_server.uri()),
    )
    .unwrap();
    assert_eq!(provider.name(), "deepseek");
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Trait dispatch works"));
}

#[tokio::test]
async fn test_kimi_trait_dispatch() {
    let mock_server = MockServer::start().await;
    mount_success(&mock_server, "Kimi trait dispatch works.").await;

    let provider: Box<dyn LlmProvider> =
        create_provider("kimi", "test-key", &config_at("kimi", &mock_server.uri())).unwrap();
    assert_eq!(provider.name(), "kimi");
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Kimi trait dispatch works"));
}

#[tokio::test]
async fn test_qwen_trait_dispatch() {
    let mock_server = MockServer::start().await;
    mount_success(&mock_server, "Qwen trait dispatch works.").await;

    let provider: Box<dyn LlmProvider> =
        create_provider("qwen", "test-key", &config_at("qwen", &mock_server.uri())).unwrap();
    assert_eq!(provider.name(), "qwen");
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Qwen trait dispatch works"));
}

#[tokio::test]
async fn test_openrouter_trait_dispatch() {
    let mock_server = MockServer::start().await;
    mount_success(&mock_server, "OpenRouter trait dispatch works.").await;

    let provider: Box<dyn LlmProvider> = create_provider(
        "openrouter",
        "test-key",
        &config_at("openrouter", &mock_server.uri()),
    )
    .unwrap();
    assert_eq!(provider.name(), "openrouter");
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("OpenRouter trait dispatch works"));
}

#[tokio::test]
async fn test_openai_trait_dispatch() {
    let mock_server = MockServer::start().await;
    mount_success(&mock_server, "OpenAI trait dispatch works.").await;

    let provider: Box<dyn LlmProvider> = create_provider(
        "openai",
        "test-key",
        &config_at("openai", &mock_server.uri()),
    )
    .unwrap();
    assert_eq!(provider.name(), "openai");
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("OpenAI trait dispatch works"));
}

#[tokio::test]
async fn test_grok_trait_dispatch() {
    let mock_server = MockServer::start().await;
    mount_success(&mock_server, "Grok trait dispatch works.").await;

    let provider: Box<dyn LlmProvider> =
        create_provider("grok", "test-key", &config_at("grok", &mock_server.uri())).unwrap();
    assert_eq!(provider.name(), "grok");
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Grok trait dispatch works"));
}

#[tokio::test]
async fn test_glm_trait_dispatch() {
    let mock_server = MockServer::start().await;
    mount_success(&mock_server, "GLM trait dispatch works.").await;

    let provider: Box<dyn LlmProvider> =
        create_provider("glm", "test-key", &config_at("glm", &mock_server.uri())).unwrap();
    assert_eq!(provider.name(), "glm");
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("GLM trait dispatch works"));
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
    mount_success(&mock_server, "Custom base URL works.").await;

    let provider = create_provider(
        "openai",
        "test-key",
        &config_at("openai", &mock_server.uri()),
    )
    .unwrap();
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Custom base URL works"));
}

#[tokio::test]
async fn test_factory_applies_max_tokens() {
    let mock_server = MockServer::start().await;
    mount_success(&mock_server, "max_tokens applied.").await;

    let mut config = config_at("deepseek", &mock_server.uri());
    config.max_tokens = Some(4096);

    let provider = create_provider("deepseek", "test-key", &config).unwrap();
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("max_tokens applied"));
}

#[tokio::test]
async fn test_deepseek_variant_flash_maps_to_model_id() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(serde_json::json!({
            "model": "deepseek-v4-flash"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{ "message": { "content": "Flash OK" } }]
        })))
        .mount(&mock_server)
        .await;

    let mut config = config_at("deepseek", &mock_server.uri());
    config.model = "ignored".to_string();
    config.variant = Some("flash".to_string());

    let provider = create_provider("deepseek", "test-key", &config).unwrap();
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Flash OK"));
}

#[tokio::test]
async fn test_deepseek_variant_pro_maps_to_model_id() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(serde_json::json!({
            "model": "deepseek-v4-pro"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{ "message": { "content": "Pro OK" } }]
        })))
        .mount(&mock_server)
        .await;

    let mut config = config_at("deepseek", &mock_server.uri());
    config.model = "ignored".to_string();
    config.variant = Some("pro".to_string());

    let provider = create_provider("deepseek", "test-key", &config).unwrap();
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Pro OK"));
}

#[tokio::test]
async fn test_deepseek_unknown_variant_returns_error() {
    let mock_server = MockServer::start().await;
    mount_success(&mock_server, "Should not reach").await;

    let mut config = config_at("deepseek", &mock_server.uri());
    config.variant = Some("unknown".to_string());

    let provider = create_provider("deepseek", "test-key", &config).unwrap();
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Unknown variant"));
    assert!(err.contains("unknown"));
}

#[tokio::test]
async fn test_openai_variant_ignored_sends_configured_model() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(serde_json::json!({
            "model": "gpt-4o-mini"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{ "message": { "content": "OpenAI ignore variant OK" } }]
        })))
        .mount(&mock_server)
        .await;

    let mut config = config_at("openai", &mock_server.uri());
    config.variant = Some("some-future-variant".to_string());

    let provider = create_provider("openai", "test-key", &config).unwrap();
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("ignore variant OK"));
}

#[tokio::test]
async fn test_kimi_unknown_variant_returns_error() {
    let mock_server = MockServer::start().await;
    mount_success(&mock_server, "Should not reach").await;

    let mut config = config_at("kimi", &mock_server.uri());
    config.variant = Some("nonexistent".to_string());

    let provider = create_provider("kimi", "test-key", &config).unwrap();
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Unknown variant"));
    assert!(err.contains("nonexistent"));
    assert!(err.contains("thinking-on, thinking-off"));
}

#[tokio::test]
async fn test_kimi_variant_thinking_on_sends_thinking_enabled() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(serde_json::json!({
            "thinking": { "type": "enabled" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{ "message": { "content": "Kimi thinking-on OK" } }]
        })))
        .mount(&mock_server)
        .await;

    let mut config = config_at("kimi", &mock_server.uri());
    config.variant = Some("thinking-on".to_string());

    let provider = create_provider("kimi", "test-key", &config).unwrap();
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("thinking-on OK"));
}

#[tokio::test]
async fn test_kimi_variant_thinking_off_sends_thinking_disabled() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(serde_json::json!({
            "thinking": { "type": "disabled" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{ "message": { "content": "Kimi thinking-off OK" } }]
        })))
        .mount(&mock_server)
        .await;

    let mut config = config_at("kimi", &mock_server.uri());
    config.variant = Some("thinking-off".to_string());

    let provider = create_provider("kimi", "test-key", &config).unwrap();
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("thinking-off OK"));
}

#[tokio::test]
async fn test_openrouter_default_referer_header_sent() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header(
            "HTTP-Referer",
            "https://github.com/nebulaideas/rs-guard",
        ))
        .and(header("X-Title", "rs-guard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{ "message": { "content": "default referer ok" } }]
        })))
        .mount(&mock_server)
        .await;

    let provider = create_provider(
        "openrouter",
        "test-key",
        &config_at("openrouter", &mock_server.uri()),
    )
    .unwrap();
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("default referer ok"));
}

#[tokio::test]
async fn test_openrouter_custom_referer_override() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("HTTP-Referer", "https://my-bot.example.com"))
        .and(header("X-Title", "rs-guard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{ "message": { "content": "override referer ok" } }]
        })))
        .mount(&mock_server)
        .await;

    let mut config = config_at("openrouter", &mock_server.uri());
    config.http_referer = Some("https://my-bot.example.com".to_string());

    let provider = create_provider("openrouter", "test-key", &config).unwrap();
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("override referer ok"));
}

#[tokio::test]
async fn test_qwen_result_format_sent() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(serde_json::json!({
            "result_format": "message"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{ "message": { "content": "qwen result_format ok" } }]
        })))
        .mount(&mock_server)
        .await;

    let provider =
        create_provider("qwen", "test-key", &config_at("qwen", &mock_server.uri())).unwrap();
    let result = provider
        .chat_completion("You are a reviewer.", "diff content", 0.1)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("qwen result_format ok"));
}
