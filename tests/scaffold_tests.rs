//! Integration tests for the scaffolding subcommands (`init`, `generate-prompt`,
//! `generate-workflow`, `validate-config`) and the configurable
//! `important_issues_threshold`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Returns the path to the compiled `rs-guard` binary.
fn rs_guard_bin() -> PathBuf {
    // CARGO_BIN_EXE_rs_guard is set when running integration tests via cargo.
    std::env::var_os("CARGO_BIN_EXE_rs_guard")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            path.push("target");
            path.push("debug");
            path.push("rs-guard");
            path
        })
}

#[test]
fn test_init_creates_scaffold_files() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(rs_guard_bin())
        .arg("init")
        .arg("--type")
        .arg("rust")
        .arg("--provider")
        .arg("kimi")
        .current_dir(dir.path())
        .output()
        .expect("failed to execute rs-guard init");

    assert!(
        output.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let prompt_path = dir.path().join(".github/review-prompt.md");
    let workflow_path = dir.path().join(".github/workflows/rs-guard-review.yml");
    let config_path = dir.path().join(".reviewer.toml");

    assert!(prompt_path.exists(), "review-prompt.md should be created");
    assert!(workflow_path.exists(), "workflow should be created");
    assert!(config_path.exists(), ".reviewer.toml should be created");

    let workflow = fs::read_to_string(&workflow_path).unwrap();
    assert!(workflow.contains("rs-guard --provider kimi"));
    assert!(workflow.contains("KIMI_API_KEY"));
    assert!(workflow.contains(&format!("v{}", env!("CARGO_PKG_VERSION"))));

    let config = fs::read_to_string(&config_path).unwrap();
    assert!(config.contains("provider = \"kimi\""));
}

#[test]
fn test_init_skips_existing_files_without_force() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join(".reviewer.toml");
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    fs::write(&config_path, "# existing\n").unwrap();

    let output = Command::new(rs_guard_bin())
        .arg("init")
        .current_dir(dir.path())
        .output()
        .expect("failed to execute rs-guard init");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("already exists"));

    let config = fs::read_to_string(&config_path).unwrap();
    assert_eq!(config, "# existing\n");
}

#[test]
fn test_generate_prompt_to_stdout() {
    let output = Command::new(rs_guard_bin())
        .arg("generate-prompt")
        .arg("--template")
        .arg("backend-api")
        .arg("--focus")
        .arg("No N+1 queries")
        .arg("--language")
        .arg("rust")
        .output()
        .expect("failed to execute rs-guard generate-prompt");

    assert!(
        output.status.success(),
        "generate-prompt failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("## Project-Specific Focus"));
    assert!(stdout.contains("- No N+1 queries"));
    assert!(stdout.contains("## rust Guardrails"));
    assert!(stdout.contains("[RS_GUARD_VERDICT_METADATA]"));
}

#[test]
fn test_generate_workflow_to_stdout() {
    let output = Command::new(rs_guard_bin())
        .arg("generate-workflow")
        .arg("--provider")
        .arg("openai")
        .arg("--model")
        .arg("gpt-4o-mini")
        .arg("--secret")
        .arg("OPENAI_API_KEY")
        .output()
        .expect("failed to execute rs-guard generate-workflow");

    assert!(
        output.status.success(),
        "generate-workflow failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("rs-guard --provider openai --model gpt-4o-mini"));
    assert!(stdout.contains("OPENAI_API_KEY"));
    assert!(stdout.contains("pull_request:"));
    assert!(!stdout.contains("pull_request_target"));
}

#[test]
fn test_generate_workflow_fork_safe() {
    let output = Command::new(rs_guard_bin())
        .arg("generate-workflow")
        .arg("--fork-safe")
        .output()
        .expect("failed to execute rs-guard generate-workflow");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("pull_request_target:"));
    assert!(stdout.contains("head.repo.full_name == github.repository"));
}

#[test]
fn test_validate_config_passes_with_valid_config() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join(".reviewer.toml");
    fs::write(
        &config_path,
        r#"provider = "deepseek"
temperature = 0.1
important_issues_threshold = 1
"#,
    )
    .unwrap();

    let output = Command::new(rs_guard_bin())
        .arg("validate-config")
        .arg("--config")
        .arg(&config_path)
        .env("DEEPSEEK_API_KEY", "test-key")
        .output()
        .expect("failed to execute rs-guard validate-config");

    assert!(
        output.status.success(),
        "validate-config failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Configuration is valid"));
    assert!(stdout.contains("Important threshold: 1"));
}

#[test]
fn test_validate_config_fails_on_typo() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join(".reviewer.toml");
    fs::write(
        &config_path,
        r#"providor = "deepseek"
"#,
    )
    .unwrap();

    let output = Command::new(rs_guard_bin())
        .arg("validate-config")
        .arg("--config")
        .arg(&config_path)
        .output()
        .expect("failed to execute rs-guard validate-config");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown key `providor`"));
}

#[test]
fn test_cli_important_threshold_flag_in_help() {
    let help = Command::new(rs_guard_bin())
        .arg("--help")
        .output()
        .expect("failed to execute rs-guard --help");
    assert!(help.status.success());
    let stdout = String::from_utf8_lossy(&help.stdout);
    assert!(stdout.contains("--important-threshold"));
}
