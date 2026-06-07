use clap::Parser;
use diffguard::config::{load_toml_config, Config, ProviderTomlConfig, TomlConfig};
use serial_test::serial;
use std::collections::HashMap;
use std::io::Write;
use tempfile::NamedTempFile;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// All known env vars that tests may set. Cleared at the start of each test
/// to prevent parallel test interference.
const ALL_TEST_ENV_VARS: &[&str] = &[
    "DIFFGUARD_PROVIDER",
    "DIFFGUARD_MODEL",
    "DIFFGUARD_TEMPERATURE",
    "DEEPSEEK_API_KEY",
    "KIMI_API_KEY",
    "MY_KIMI_KEY",
    "OPENAI_API_KEY",
    "DASHSCOPE_API_KEY",
    "OPENROUTER_API_KEY",
    "MY_CUSTOM_KEY",
    "GITHUB_ACTIONS",
    "GITHUB_TOKEN",
    "PR_NUMBER",
    "REPO_FULL_NAME",
    "DIFFGUARD_DIFF_FILE",
];

/// Removes all known env vars to guarantee a clean slate.
fn clean_env() {
    for var in ALL_TEST_ENV_VARS {
        std::env::remove_var(var);
    }
}

/// Sets env vars for the duration of a closure, then cleans all known vars
/// when done. Used together with `#[serial]` to guarantee isolation.
fn with_env<K, V>(vars: &[(K, V)], f: impl FnOnce())
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    clean_env();
    for (k, v) in vars {
        std::env::set_var(k.as_ref(), v.as_ref());
    }
    f();
    clean_env();
}

fn write_toml(content: &[u8]) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(content).unwrap();
    file
}

// ---------------------------------------------------------------------------
// TOML parsing (pure, no env vars needed)
// ---------------------------------------------------------------------------

#[test]
fn test_toml_parse_valid() {
    let file = write_toml(
        br#"provider = "kimi"
model = "kimi-k2.5"
temperature = 0.5
max_tokens = 4096

[providers.kimi]
api_key_env = "KIMI_API_KEY"
base_url = "https://api.moonshot.ai/v1"
"#,
    );

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
    let file = write_toml(b"this is not valid toml {{{");
    let result = load_toml_config(file.path());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Failed to parse"));
}

// ---------------------------------------------------------------------------
// Env-dependent tests — all marked #[serial] to prevent parallel corruption
// of the global environment.
// ---------------------------------------------------------------------------

#[test]
#[serial]
fn test_env_overrides_toml() {
    let file = write_toml(
        br#"provider = "kimi"
model = "kimi-k2.5"
temperature = 0.5
"#,
    );

    with_env(
        &[
            ("DIFFGUARD_PROVIDER", "openai"),
            ("DIFFGUARD_MODEL", "gpt-4o"),
            ("DIFFGUARD_TEMPERATURE", "0.7"),
            ("OPENAI_API_KEY", "test-openai-key"),
        ],
        || {
            let toml = load_toml_config(file.path()).unwrap();
            let config = Config::from_env(toml).unwrap();
            assert_eq!(config.provider, "openai");
            assert_eq!(config.model, "gpt-4o");
            assert!((config.temperature - 0.7).abs() < f32::EPSILON);
        },
    );
}

#[test]
#[serial]
fn test_toml_fallback_when_no_env() {
    let file = write_toml(
        br#"provider = "kimi"
model = "kimi-k2.5"
temperature = 0.5
"#,
    );

    with_env(&[("KIMI_API_KEY", "test-kimi-key")], || {
        let toml = load_toml_config(file.path()).unwrap();
        let config = Config::from_env(toml).unwrap();

        assert_eq!(config.provider, "kimi");
        assert_eq!(config.model, "kimi-k2.5");
        assert!((config.temperature - 0.5).abs() < f32::EPSILON);
    });
}

#[test]
#[serial]
fn test_provider_switch_via_apply_args() {
    with_env(
        &[
            ("DEEPSEEK_API_KEY", "test-deepseek-key"),
            ("KIMI_API_KEY", "test-kimi-key"),
        ],
        || {
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
        },
    );
}

#[test]
#[serial]
fn test_cli_model_override() {
    with_env(&[("DEEPSEEK_API_KEY", "test-deepseek-key")], || {
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
    });
}

#[test]
#[serial]
fn test_toml_per_provider_base_url_wired() {
    with_env(&[("OPENAI_API_KEY", "test-openai-key-2")], || {
        let file = write_toml(
            br#"provider = "openai"
model = "gpt-4o"

[providers.openai]
base_url = "http://localhost:11434/v1"
"#,
        );

        let toml = load_toml_config(file.path()).unwrap();
        let config = Config::from_env(toml).unwrap();

        assert_eq!(
            config.provider_config.base_url,
            Some("http://localhost:11434/v1".to_string())
        );
    });
}

#[test]
#[serial]
fn test_toml_custom_api_key_env() {
    with_env(&[("MY_CUSTOM_KEY", "custom-key-value")], || {
        let file = write_toml(
            br#"provider = "openai"

[providers.openai]
api_key_env = "MY_CUSTOM_KEY"
"#,
        );

        let toml = load_toml_config(file.path()).unwrap();
        let config = Config::from_env(toml).unwrap();

        assert_eq!(config.api_key, "custom-key-value");
    });
}

#[test]
#[serial]
fn test_unknown_provider_returns_error() {
    with_env(&[("DIFFGUARD_PROVIDER", "nonexistent")], || {
        let result = Config::from_env(None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unknown provider"));
    });
}

#[test]
#[serial]
fn test_model_resets_on_provider_change_when_not_explicit() {
    with_env(
        &[
            ("DEEPSEEK_API_KEY", "test-deepseek-key-2"),
            ("KIMI_API_KEY", "test-kimi-key-2"),
        ],
        || {
            let toml = Some(TomlConfig {
                provider: Some("deepseek".to_string()),
                model: None,
                temperature: None,
                max_tokens: None,
                providers: None,
            });

            let mut config = Config::from_env(toml).unwrap();
            assert_eq!(config.model, "deepseek-v4-flash");

            let args = diffguard::cli::Args::parse_from(["diffguard", "--provider", "kimi"]);
            config.apply_args(&args).unwrap();

            assert_eq!(config.provider, "kimi");
            assert_eq!(config.model, "kimi-k2.5");
        },
    );
}

#[test]
#[serial]
fn test_toml_model_not_carried_across_provider_change() {
    with_env(
        &[
            ("DEEPSEEK_API_KEY", "test-deepseek-key"),
            ("KIMI_API_KEY", "test-kimi-key"),
        ],
        || {
            let toml = Some(TomlConfig {
                provider: Some("deepseek".to_string()),
                model: Some("my-custom-model".to_string()),
                temperature: None,
                max_tokens: None,
                providers: None,
            });

            let mut config = Config::from_env(toml).unwrap();
            assert_eq!(config.model, "my-custom-model");

            let args = diffguard::cli::Args::parse_from(["diffguard", "--provider", "kimi"]);
            config.apply_args(&args).unwrap();

            assert_eq!(config.provider, "kimi");
            assert_eq!(config.model, "kimi-k2.5");
        },
    );
}

#[test]
#[serial]
fn test_cli_model_preserved_across_provider_change() {
    with_env(
        &[
            ("DEEPSEEK_API_KEY", "test-deepseek-key"),
            ("KIMI_API_KEY", "test-kimi-key"),
        ],
        || {
            let toml = Some(TomlConfig {
                provider: Some("deepseek".to_string()),
                model: None,
                temperature: None,
                max_tokens: None,
                providers: None,
            });

            let mut config = Config::from_env(toml).unwrap();
            let args = diffguard::cli::Args::parse_from([
                "diffguard",
                "--provider",
                "kimi",
                "--model",
                "cli-model",
            ]);
            config.apply_args(&args).unwrap();

            assert_eq!(config.provider, "kimi");
            assert_eq!(config.model, "cli-model");
        },
    );
}

#[test]
#[serial]
fn test_apply_args_respects_toml_api_key_env_on_switch() {
    with_env(
        &[
            ("DEEPSEEK_API_KEY", "test-deepseek-key"),
            ("MY_KIMI_KEY", "custom-kimi-key"),
        ],
        || {
            let toml = Some(TomlConfig {
                provider: Some("deepseek".to_string()),
                model: None,
                temperature: None,
                max_tokens: None,
                providers: Some({
                    let mut m = HashMap::new();
                    m.insert(
                        "kimi".to_string(),
                        ProviderTomlConfig {
                            api_key_env: Some("MY_KIMI_KEY".to_string()),
                            base_url: None,
                            http_referer: None,
                        },
                    );
                    m
                }),
            });

            let mut config = Config::from_env(toml).unwrap();
            assert_eq!(config.provider, "deepseek");

            let args = diffguard::cli::Args::parse_from(["diffguard", "--provider", "kimi"]);
            config.apply_args(&args).unwrap();

            assert_eq!(config.provider, "kimi");
            assert_eq!(config.api_key, "custom-kimi-key");
        },
    );
}

#[test]
#[serial]
fn test_ssrf_rejection_in_ci_mode() {
    with_env(
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_TOKEN", "test-token"),
            ("PR_NUMBER", "42"),
            ("REPO_FULL_NAME", "owner/repo"),
            ("DIFFGUARD_PROVIDER", "deepseek"),
            ("DEEPSEEK_API_KEY", "test-key-ssrf"),
        ],
        || {
            let toml = Some(TomlConfig {
                provider: Some("deepseek".to_string()),
                model: None,
                temperature: None,
                max_tokens: None,
                providers: Some({
                    let mut m = HashMap::new();
                    m.insert(
                        "deepseek".to_string(),
                        ProviderTomlConfig {
                            api_key_env: None,
                            base_url: Some("https://evil.example.com/v1".to_string()),
                            http_referer: None,
                        },
                    );
                    m
                }),
            });

            let result = Config::from_env(toml);
            assert!(result.is_err());
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("not in the CI allowlist"));
        },
    );
}

#[test]
#[serial]
fn test_ssrf_allows_known_host_in_ci() {
    with_env(
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_TOKEN", "test-token"),
            ("PR_NUMBER", "42"),
            ("REPO_FULL_NAME", "owner/repo"),
            ("DEEPSEEK_API_KEY", "test-key-ssrf"),
        ],
        || {
            let toml = Some(TomlConfig {
                provider: Some("deepseek".to_string()),
                model: None,
                temperature: None,
                max_tokens: None,
                providers: Some({
                    let mut m = HashMap::new();
                    m.insert(
                        "deepseek".to_string(),
                        ProviderTomlConfig {
                            api_key_env: None,
                            base_url: Some("https://api.deepseek.com".to_string()),
                            http_referer: None,
                        },
                    );
                    m
                }),
            });

            let config = Config::from_env(toml).unwrap();
            assert_eq!(
                config.provider_config.base_url,
                Some("https://api.deepseek.com".to_string())
            );
        },
    );
}

#[test]
#[serial]
fn test_ssrf_allows_any_host_in_local_mode() {
    with_env(&[("DEEPSEEK_API_KEY", "test-key-local")], || {
        let toml = Some(TomlConfig {
            provider: Some("deepseek".to_string()),
            model: None,
            temperature: None,
            max_tokens: None,
            providers: Some({
                let mut m = HashMap::new();
                m.insert(
                    "deepseek".to_string(),
                    ProviderTomlConfig {
                        api_key_env: None,
                        base_url: Some("https://my-local-llm.example.com/v1".to_string()),
                        http_referer: None,
                    },
                );
                m
            }),
        });

        let config = Config::from_env(toml).unwrap();
        assert_eq!(
            config.provider_config.base_url,
            Some("https://my-local-llm.example.com/v1".to_string())
        );
    });
}

#[test]
#[serial]
fn test_ssrf_rejection_on_apply_args_switch_in_ci() {
    with_env(
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_TOKEN", "test-token"),
            ("PR_NUMBER", "42"),
            ("REPO_FULL_NAME", "owner/repo"),
            ("DEEPSEEK_API_KEY", "test-deepseek-ssrf"),
            ("KIMI_API_KEY", "test-kimi-ssrf-switch"),
        ],
        || {
            let toml = Some(TomlConfig {
                provider: Some("deepseek".to_string()),
                model: None,
                temperature: None,
                max_tokens: None,
                providers: Some({
                    let mut m = HashMap::new();
                    m.insert(
                        "deepseek".to_string(),
                        ProviderTomlConfig {
                            api_key_env: None,
                            base_url: None,
                            http_referer: None,
                        },
                    );
                    m.insert(
                        "kimi".to_string(),
                        ProviderTomlConfig {
                            api_key_env: None,
                            base_url: Some("https://evil.example.com/v1".to_string()),
                            http_referer: None,
                        },
                    );
                    m
                }),
            });

            let mut config = Config::from_env(toml).unwrap();
            assert_eq!(config.provider, "deepseek");

            let args = diffguard::cli::Args::parse_from(["diffguard", "--provider", "kimi"]);
            let result = config.apply_args(&args);
            assert!(result.is_err());
            let err = result.unwrap_err().to_string();
            assert!(err.contains("not in the CI allowlist"));
        },
    );
}

#[test]
#[serial]
fn test_base_url_cleared_on_switch_without_toml_entry() {
    with_env(
        &[
            ("DEEPSEEK_API_KEY", "test-key"),
            ("KIMI_API_KEY", "test-kimi-clear-url"),
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_TOKEN", "test-token"),
            ("PR_NUMBER", "42"),
            ("REPO_FULL_NAME", "owner/repo"),
        ],
        || {
            let toml = Some(TomlConfig {
                provider: Some("deepseek".to_string()),
                model: None,
                temperature: None,
                max_tokens: None,
                providers: Some({
                    let mut m = HashMap::new();
                    m.insert(
                        "deepseek".to_string(),
                        ProviderTomlConfig {
                            api_key_env: None,
                            base_url: Some("https://api.deepseek.com".to_string()),
                            http_referer: None,
                        },
                    );
                    m
                }),
            });

            let mut config = Config::from_env(toml).unwrap();
            assert_eq!(
                config.provider_config.base_url,
                Some("https://api.deepseek.com".to_string())
            );

            let args = diffguard::cli::Args::parse_from(["diffguard", "--provider", "kimi"]);
            config.apply_args(&args).unwrap();
            assert_eq!(config.provider, "kimi");
            assert_eq!(config.provider_config.base_url, None);
        },
    );
}

#[test]
#[serial]
fn test_base_url_preserved_on_switch_with_toml_entry() {
    with_env(
        &[
            ("DEEPSEEK_API_KEY", "test-key"),
            ("KIMI_API_KEY", "test-kimi-preserve-url"),
        ],
        || {
            let toml = Some(TomlConfig {
                provider: Some("deepseek".to_string()),
                model: None,
                temperature: None,
                max_tokens: None,
                providers: Some({
                    let mut m = HashMap::new();
                    m.insert(
                        "deepseek".to_string(),
                        ProviderTomlConfig {
                            api_key_env: None,
                            base_url: Some("https://api.deepseek.com".to_string()),
                            http_referer: None,
                        },
                    );
                    m.insert(
                        "kimi".to_string(),
                        ProviderTomlConfig {
                            api_key_env: None,
                            base_url: Some("http://localhost:8080/v1".to_string()),
                            http_referer: None,
                        },
                    );
                    m
                }),
            });

            let mut config = Config::from_env(toml).unwrap();
            let args = diffguard::cli::Args::parse_from(["diffguard", "--provider", "kimi"]);
            config.apply_args(&args).unwrap();

            assert_eq!(
                config.provider_config.base_url,
                Some("http://localhost:8080/v1".to_string())
            );
        },
    );
}

#[test]
#[serial]
fn test_model_synced_after_switch_with_cli_model() {
    with_env(
        &[
            ("KIMI_API_KEY", "test-kimi-model-sync"),
            ("DEEPSEEK_API_KEY", "test-key"),
        ],
        || {
            let toml = Some(TomlConfig {
                provider: Some("deepseek".to_string()),
                model: None,
                temperature: None,
                max_tokens: None,
                providers: None,
            });

            let mut config = Config::from_env(toml).unwrap();
            assert_eq!(config.provider_config.model, "deepseek-v4-flash");

            let args = diffguard::cli::Args::parse_from([
                "diffguard",
                "--provider",
                "kimi",
                "--model",
                "my-custom-model",
            ]);
            config.apply_args(&args).unwrap();

            assert_eq!(config.model, "my-custom-model");
            assert_eq!(config.provider_config.model, "my-custom-model");
        },
    );
}
