use clap::Parser;
use diffguard::config::{load_toml_config, Config, TomlConfig};
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_toml_parse_valid() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(
        br#"provider = "kimi"
model = "kimi-k2.5"
temperature = 0.5
max_tokens = 4096

[providers.kimi]
api_key_env = "KIMI_API_KEY"
base_url = "https://api.moonshot.ai/v1"
"#,
    )
    .unwrap();

    let config = load_toml_config(file.path()).unwrap();
    assert!(config.is_some());

    let toml = config.unwrap();
    assert_eq!(toml.provider, Some("kimi".to_string()));
    assert_eq!(toml.model, Some("kimi-k2.5".to_string()));
    assert_eq!(toml.temperature, Some(0.5));
    assert_eq!(toml.max_tokens, Some(4096));

    let providers = toml.providers.unwrap();
    let kimi = providers.get("kimi").unwrap();
    assert_eq!(kimi.api_key_env, Some("KIMI_API_KEY".to_string()));
    assert_eq!(
        kimi.base_url,
        Some("https://api.moonshot.ai/v1".to_string())
    );
}

#[test]
fn test_toml_missing_file_ok() {
    let path = std::env::temp_dir().join("nonexistent_diffguard_reviewer.toml");
    let config = load_toml_config(&path).unwrap();
    assert!(config.is_none());
}

#[test]
fn test_toml_parse_invalid_returns_error() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"this is not valid toml {{{").unwrap();

    let result = load_toml_config(file.path());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Failed to parse"));
}

#[test]
fn test_toml_resolution_and_fallback() {
    // Consolidated test covering resolution order and fallback scenarios
    // to avoid env-var interference between parallel tests.

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(
        br#"provider = "kimi"
model = "kimi-k2.5"
temperature = 0.5
"#,
    )
    .unwrap();

    // --- Scenario 1: Env vars override TOML ---
    std::env::set_var("DIFFGUARD_PROVIDER", "openai");
    std::env::set_var("DIFFGUARD_MODEL", "gpt-4o");
    std::env::set_var("DIFFGUARD_TEMPERATURE", "0.7");
    std::env::set_var("OPENAI_API_KEY", "test-openai-key");

    let toml = load_toml_config(file.path()).unwrap();
    let config = Config::from_env(toml).unwrap();

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4o");
    assert!((config.temperature - 0.7).abs() < f32::EPSILON);

    // --- Scenario 2: TOML fallback when no env ---
    std::env::remove_var("DIFFGUARD_PROVIDER");
    std::env::remove_var("DIFFGUARD_MODEL");
    std::env::remove_var("DIFFGUARD_TEMPERATURE");
    std::env::set_var("KIMI_API_KEY", "test-kimi-key");

    let toml = load_toml_config(file.path()).unwrap();
    let config = Config::from_env(toml).unwrap();

    assert_eq!(config.provider, "kimi");
    assert_eq!(config.model, "kimi-k2.5");
    assert!((config.temperature - 0.5).abs() < f32::EPSILON);

    // --- Scenario 3: Provider switch via apply_args ---
    std::env::set_var("DEEPSEEK_API_KEY", "test-deepseek-key");

    let toml = Some(TomlConfig {
        provider: Some("deepseek".to_string()),
        model: Some("deepseek-v4-flash".to_string()),
        temperature: Some(0.1),
        max_tokens: None,
        providers: None,
    });

    let mut config = Config::from_env(toml).unwrap();
    assert_eq!(config.provider, "deepseek");

    let args = diffguard::cli::Args::parse_from(["diffguard", "--provider", "kimi"]);
    config.apply_args(&args).unwrap();

    assert_eq!(config.provider, "kimi");
    assert_eq!(config.api_key, "test-kimi-key");

    // --- Scenario 4: CLI model override via Option ---
    let toml = Some(TomlConfig {
        provider: Some("deepseek".to_string()),
        model: Some("deepseek-v4-flash".to_string()),
        temperature: Some(0.1),
        max_tokens: None,
        providers: None,
    });

    let mut config = Config::from_env(toml).unwrap();
    assert_eq!(config.model, "deepseek-v4-flash");

    let args = diffguard::cli::Args::parse_from(["diffguard", "--model", "custom-model"]);
    config.apply_args(&args).unwrap();

    assert_eq!(config.model, "custom-model");

    // --- Scenario 5: TOML per-provider base_url is wired ---
    std::env::remove_var("DEEPSEEK_API_KEY");
    std::env::remove_var("KIMI_API_KEY");
    std::env::set_var("OPENAI_API_KEY", "test-openai-key-2");

    let mut file2 = NamedTempFile::new().unwrap();
    file2
        .write_all(
            br#"provider = "openai"
model = "gpt-4o"

[providers.openai]
base_url = "http://localhost:11434/v1"
"#,
        )
        .unwrap();

    let toml = load_toml_config(file2.path()).unwrap();
    let config = Config::from_env(toml).unwrap();

    assert_eq!(
        config.provider_config.base_url,
        Some("http://localhost:11434/v1".to_string())
    );

    // --- Scenario 6: TOML custom api_key_env override ---
    std::env::set_var("MY_CUSTOM_KEY", "custom-key-value");

    let mut file3 = NamedTempFile::new().unwrap();
    file3
        .write_all(
            br#"provider = "openai"

[providers.openai]
api_key_env = "MY_CUSTOM_KEY"
"#,
        )
        .unwrap();

    let toml = load_toml_config(file3.path()).unwrap();
    let config = Config::from_env(toml).unwrap();

    assert_eq!(config.api_key, "custom-key-value");

    // --- Scenario 7: Unknown provider returns error ---
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("MY_CUSTOM_KEY");
    std::env::set_var("DIFFGUARD_PROVIDER", "nonexistent");

    let result = Config::from_env(None);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Unknown provider"));

    // Clean up all env vars
    std::env::remove_var("DIFFGUARD_PROVIDER");
    std::env::remove_var("DIFFGUARD_MODEL");
    std::env::remove_var("DIFFGUARD_TEMPERATURE");
}
