use clap::Parser;
use rs_guard::config::{load_toml_config, Config, ProviderTomlConfig, TomlConfig};
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
    "RS_GUARD_PROVIDER",
    "RS_GUARD_MODEL",
    "RS_GUARD_TEMPERATURE",
    "RS_GUARD_MAX_TOKENS",
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
    "RS_GUARD_DIFF_FILE",
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
    let path = std::env::temp_dir().join("nonexistent_rs_guard_reviewer.toml");
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
            ("RS_GUARD_PROVIDER", "openai"),
            ("RS_GUARD_MODEL", "gpt-4o"),
            ("RS_GUARD_TEMPERATURE", "0.7"),
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
                ..Default::default()
            });

            let mut config = Config::from_env(toml).unwrap();
            assert_eq!(config.provider, "deepseek");

            let args = rs_guard::cli::Args::parse_from(["rs-guard", "--provider", "kimi"]);
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
            ..Default::default()
        });

        let mut config = Config::from_env(toml).unwrap();
        assert_eq!(config.model, "deepseek-v4-flash");

        let args = rs_guard::cli::Args::parse_from(["rs-guard", "--model", "custom-model"]);
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
    with_env(&[("RS_GUARD_PROVIDER", "nonexistent")], || {
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
                ..Default::default()
            });

            let mut config = Config::from_env(toml).unwrap();
            assert_eq!(config.model, "deepseek-v4-flash");

            let args = rs_guard::cli::Args::parse_from(["rs-guard", "--provider", "kimi"]);
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
                ..Default::default()
            });

            let mut config = Config::from_env(toml).unwrap();
            assert_eq!(config.model, "my-custom-model");

            let args = rs_guard::cli::Args::parse_from(["rs-guard", "--provider", "kimi"]);
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
                ..Default::default()
            });

            let mut config = Config::from_env(toml).unwrap();
            let args = rs_guard::cli::Args::parse_from([
                "rs-guard",
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
                cache_dir: None,
                circuit_breaker: None,
                pricing: None,
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
                ..Default::default()
            });

            let mut config = Config::from_env(toml).unwrap();
            assert_eq!(config.provider, "deepseek");

            let args = rs_guard::cli::Args::parse_from(["rs-guard", "--provider", "kimi"]);
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
            ("RS_GUARD_PROVIDER", "deepseek"),
            ("DEEPSEEK_API_KEY", "test-key-ssrf"),
        ],
        || {
            let toml = Some(TomlConfig {
                provider: Some("deepseek".to_string()),
                model: None,
                temperature: None,
                max_tokens: None,
                cache_dir: None,
                circuit_breaker: None,
                pricing: None,
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
                ..Default::default()
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
                cache_dir: None,
                circuit_breaker: None,
                pricing: None,
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
                ..Default::default()
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
            cache_dir: None,
            circuit_breaker: None,
            pricing: None,
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
            ..Default::default()
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
                cache_dir: None,
                circuit_breaker: None,
                pricing: None,
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
                ..Default::default()
            });

            let mut config = Config::from_env(toml).unwrap();
            assert_eq!(config.provider, "deepseek");

            let args = rs_guard::cli::Args::parse_from(["rs-guard", "--provider", "kimi"]);
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
                cache_dir: None,
                circuit_breaker: None,
                pricing: None,
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
                ..Default::default()
            });

            let mut config = Config::from_env(toml).unwrap();
            assert_eq!(
                config.provider_config.base_url,
                Some("https://api.deepseek.com".to_string())
            );

            let args = rs_guard::cli::Args::parse_from(["rs-guard", "--provider", "kimi"]);
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
                cache_dir: None,
                circuit_breaker: None,
                pricing: None,
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
                ..Default::default()
            });

            let mut config = Config::from_env(toml).unwrap();
            let args = rs_guard::cli::Args::parse_from(["rs-guard", "--provider", "kimi"]);
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
                ..Default::default()
            });

            let mut config = Config::from_env(toml).unwrap();
            assert_eq!(config.provider_config.model, "deepseek-v4-flash");

            let args = rs_guard::cli::Args::parse_from([
                "rs-guard",
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

// ---------------------------------------------------------------------------
// Issue #9 — RS_GUARD_TEMPERATURE env var validation
// ---------------------------------------------------------------------------

#[test]
#[serial]
fn test_invalid_temperature_env_var_returns_error() {
    // RS_GUARD_TEMPERATURE=abc must return a Config error, not silently fall back to 0.1
    with_env(
        &[
            ("DEEPSEEK_API_KEY", "test-deepseek-key"),
            ("RS_GUARD_TEMPERATURE", "abc"),
        ],
        || {
            let result = Config::from_env(None);
            assert!(
                result.is_err(),
                "expected error for invalid temperature 'abc', got Ok"
            );
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains("RS_GUARD_TEMPERATURE") || err.contains("temperature"),
                "expected temperature-related error, got: {}",
                err
            );
        },
    );
}

#[test]
#[serial]
fn test_temperature_env_var_out_of_range_returns_error() {
    // RS_GUARD_TEMPERATURE=3.0 must return a Config error (range is [0.0, 2.0])
    with_env(
        &[
            ("DEEPSEEK_API_KEY", "test-deepseek-key"),
            ("RS_GUARD_TEMPERATURE", "3.0"),
        ],
        || {
            let result = Config::from_env(None);
            assert!(
                result.is_err(),
                "expected error for out-of-range temperature 3.0, got Ok"
            );
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains("Temperature") || err.contains("temperature"),
                "expected temperature-related error, got: {}",
                err
            );
        },
    );
}

#[test]
#[serial]
fn test_valid_temperature_env_var_accepted() {
    with_env(
        &[
            ("DEEPSEEK_API_KEY", "test-deepseek-key"),
            ("RS_GUARD_TEMPERATURE", "0.7"),
        ],
        || {
            let result = Config::from_env(None);
            assert!(result.is_ok(), "expected Ok for valid temperature 0.7");
            assert!((result.unwrap().temperature - 0.7).abs() < f32::EPSILON);
        },
    );
}

#[test]
#[serial]
fn test_temperature_env_var_negative_returns_error() {
    // Negative temperatures are out of range
    with_env(
        &[
            ("DEEPSEEK_API_KEY", "test-deepseek-key"),
            ("RS_GUARD_TEMPERATURE", "-0.1"),
        ],
        || {
            let result = Config::from_env(None);
            assert!(
                result.is_err(),
                "expected error for negative temperature -0.1, got Ok"
            );
        },
    );
}

// ---------------------------------------------------------------------------
// Issues #7 & #29 — Configurable chunking thresholds via TOML
// ---------------------------------------------------------------------------

#[test]
#[serial]
fn test_toml_chunk_thresholds_parsed_and_applied() {
    // chunk_head_lines and chunk_tail_lines in .reviewer.toml must flow
    // through Config and be used by the pipeline's chunk_diff_with_params call.
    let file = write_toml(
        br#"provider = "deepseek"
chunk_head_lines = 200
chunk_tail_lines = 100
"#,
    );

    with_env(&[("DEEPSEEK_API_KEY", "test-deepseek-key")], || {
        let toml = load_toml_config(file.path()).unwrap();
        let config = Config::from_env(toml).unwrap();
        assert_eq!(config.chunk_head_lines, 200);
        assert_eq!(config.chunk_tail_lines, 100);
    });
}

#[test]
#[serial]
fn test_default_chunk_thresholds_when_not_set() {
    // When not set in TOML, config uses the DEFAULT_CHUNK_HEAD/TAIL_LINES values.
    with_env(&[("DEEPSEEK_API_KEY", "test-deepseek-key")], || {
        let config = Config::from_env(None).unwrap();
        assert_eq!(
            config.chunk_head_lines,
            rs_guard::diff::DEFAULT_CHUNK_HEAD_LINES
        );
        assert_eq!(
            config.chunk_tail_lines,
            rs_guard::diff::DEFAULT_CHUNK_TAIL_LINES
        );
    });
}

// ---------------------------------------------------------------------------
// Issue #30 — max_tokens safe default (DEFAULT_MAX_TOKENS = 4096)
// ---------------------------------------------------------------------------

#[test]
#[serial]
fn test_default_max_tokens_applied_when_not_set() {
    // When RS_GUARD_MAX_TOKENS is unset and TOML has no max_tokens,
    // Config must use DEFAULT_MAX_TOKENS (4096) to prevent truncated verdicts.
    with_env(&[("DEEPSEEK_API_KEY", "test-deepseek-key")], || {
        let config = Config::from_env(None).unwrap();
        assert_eq!(
            config.provider_config.max_tokens,
            Some(rs_guard::config::DEFAULT_MAX_TOKENS),
            "max_tokens should default to DEFAULT_MAX_TOKENS when not configured"
        );
    });
}

#[test]
#[serial]
fn test_env_max_tokens_overrides_default() {
    with_env(
        &[
            ("DEEPSEEK_API_KEY", "test-deepseek-key"),
            ("RS_GUARD_MAX_TOKENS", "8192"),
        ],
        || {
            let config = Config::from_env(None).unwrap();
            assert_eq!(config.provider_config.max_tokens, Some(8192));
        },
    );
}

#[test]
#[serial]
fn test_toml_max_tokens_overrides_default() {
    let file = write_toml(
        br#"provider = "deepseek"
max_tokens = 2048
"#,
    );
    with_env(&[("DEEPSEEK_API_KEY", "test-deepseek-key")], || {
        let toml = load_toml_config(file.path()).unwrap();
        let config = Config::from_env(toml).unwrap();
        assert_eq!(config.provider_config.max_tokens, Some(2048));
    });
}
