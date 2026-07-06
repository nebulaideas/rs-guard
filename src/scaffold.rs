//! Scaffolding commands: `init`, `generate-prompt`, `generate-workflow`, and
//! `validate-config`.
//!
//! These commands turn the static example files in `examples/` into interactive
//! generators so teams can adopt rs-guard without hand-copying YAML and
//! markdown templates.

use crate::cli::{
    GeneratePromptArgs, GenerateWorkflowArgs, InitArgs, ProjectType, PromptTemplate,
    ValidateConfigArgs,
};
use crate::config::{load_toml_config, Config};
use crate::llm::providers::{find_provider, known_provider_names};
use crate::rules::{detect_project_rules, load_rules_file};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Default provider used when none is specified.
const DEFAULT_PROVIDER: &str = "deepseek";

/// GitHub Actions workflow template used by `generate-workflow`.
///
/// Placeholders are replaced at generation time so the generated workflow
/// matches the selected provider, model, and release version.
const WORKFLOW_TEMPLATE: &str = r#"name: AI Code Review ({{PROVIDER}})

on:
  {{EVENT}}:
    {{TYPES}}

# Only run one review per PR at a time.
# New pushes cancel in-progress reviews so the latest commit is always reviewed.
concurrency:
  group: ${{ github.workflow }}-${{ github.event.pull_request.number }}
  cancel-in-progress: true

jobs:
  review:
    # Skip draft PRs to avoid wasting tokens on work-in-progress.
    if: github.event.pull_request.draft == false{{FORK_GUARD}}
    runs-on: ubuntu-latest
    permissions:
      contents: read
      pull-requests: write
    steps:
      # Pinned from actions/checkout@v5 (93cb6efe) to avoid Node.js 20 deprecation.
      - uses: actions/checkout@93cb6efe18208431cddfb8368fd83d5badbf9bfd

      # Download rs-guard {{VERSION}} and verify its SHA-256.
      - name: Download rs-guard
        run: |
          set -euo pipefail
          VERSION="{{VERSION}}"
          BINARY="rs-guard-x86_64-unknown-linux-gnu"
          curl -L --fail -o "${BINARY}" \
            "https://github.com/nebulaideas/rs-guard/releases/download/${VERSION}/${BINARY}"
          curl -L --fail -o "${BINARY}.sha256" \
            "https://github.com/nebulaideas/rs-guard/releases/download/${VERSION}/${BINARY}.sha256"
          sha256sum -c "${BINARY}.sha256"
          chmod +x "${BINARY}"
          mv "${BINARY}" rs-guard

      # Run the AI code review.
      # The tool reads the diff from the PR, sends it to the configured LLM,
      # parses the structured verdict, and posts the review back to GitHub.
      - name: AI Code Review
        run: {{RUN_LINE}}
        env:
          {{SECRET}}: ${{ secrets.{{SECRET}} }}
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          PR_NUMBER: ${{ github.event.pull_request.number }}
          REPO_FULL_NAME: ${{ github.repository }}
"#;

/// Runs the `init` subcommand, scaffolding configuration and workflow files in
/// the current working directory.
///
/// # Errors
///
/// Returns [`io::Error`] if any file cannot be written, or a descriptive error
/// string if the provider is unknown.
pub fn run_init(args: &InitArgs) -> Result<(), Box<dyn std::error::Error>> {
    let project_type = args.project_type.unwrap_or_else(detect_project_type);
    let provider = args.provider.as_deref().unwrap_or(DEFAULT_PROVIDER);

    if find_provider(provider).is_none() {
        return Err(format!(
            "Unknown provider '{}'. Supported: {}",
            provider,
            known_provider_names().join(", ")
        )
        .into());
    }

    let prompt_template = match project_type {
        ProjectType::BackendApi => PromptTemplate::BackendApi,
        ProjectType::FrontendSpa => PromptTemplate::FrontendSpa,
        ProjectType::CliTooling => PromptTemplate::CliTooling,
        ProjectType::Rust | ProjectType::General => PromptTemplate::General,
    };

    let prompt = generate_prompt(&GeneratePromptArgs {
        template: prompt_template,
        focus: Vec::new(),
        language: language_for_project_type(project_type),
        output: Some(Path::new(".github/review-prompt.md").into()),
    });

    let workflow = generate_workflow(&GenerateWorkflowArgs {
        provider: Some(provider.to_string()),
        model: None,
        secret: None,
        fork_safe: false,
        output: Some(Path::new(".github/workflows/rs-guard-review.yml").into()),
    })?;

    let config = generate_config(provider);

    write_file(".github/review-prompt.md", &prompt, args.force)?;
    write_file(
        ".github/workflows/rs-guard-review.yml",
        &workflow,
        args.force,
    )?;
    write_file(".reviewer.toml", &config, args.force)?;

    let detected_label = match project_type {
        ProjectType::Rust => "Rust",
        ProjectType::BackendApi => "Backend/API",
        ProjectType::FrontendSpa => "Frontend SPA",
        ProjectType::CliTooling => "CLI/tooling",
        ProjectType::General => "general",
    };
    println!(
        "✅ rs-guard scaffolding complete for {} project.",
        detected_label
    );
    if args.project_type.is_none() {
        println!(
            "   Project type auto-detected. Override with: rs-guard init --type <rust|backend-api|frontend-spa|cli-tooling|general>"
        );
    }
    println!();
    println!("Generated files:");
    println!("  - .github/workflows/rs-guard-review.yml");
    println!("  - .github/review-prompt.md");
    println!("  - .reviewer.toml");

    let rules = detect_project_rules(Path::new("."));
    println!();
    match rules {
        Ok(rules) => println!("{}", format_project_rules_init_notice(rules.as_ref())),
        Err(e) => {
            println!(
                "⚠️  Could not scan for project rules files: {}. Continuing with initialization.",
                e
            );
        }
    }

    println!();
    println!("Next steps:");
    println!(
        "  1. Add your {} API key as a GitHub repository secret.",
        api_key_secret_name(provider)
    );
    println!("  2. Review and customize .github/review-prompt.md for your conventions.");
    println!("  3. Commit these files and open a test pull request.");

    Ok(())
}

/// Runs the `generate-prompt` subcommand, printing the generated prompt to
/// stdout or writing it to the requested file.
///
/// # Errors
///
/// Returns an error if writing to the output fails.
pub fn run_generate_prompt(args: &GeneratePromptArgs) -> Result<(), Box<dyn std::error::Error>> {
    let prompt = generate_prompt(args);
    if let Some(path) = &args.output {
        fs::write(path, prompt)?;
        println!("Generated prompt written to {}", path.display());
    } else {
        io::stdout().write_all(prompt.as_bytes())?;
    }
    Ok(())
}

/// Runs the `generate-workflow` subcommand, printing the generated workflow to
/// stdout or writing it to the requested file.
///
/// # Errors
///
/// Returns an error if the requested provider is unknown or the file cannot be
/// written.
pub fn run_generate_workflow(
    args: &GenerateWorkflowArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = generate_workflow(args)?;
    if let Some(path) = &args.output {
        fs::write(path, workflow)?;
        println!("Generated workflow written to {}", path.display());
    } else {
        io::stdout().write_all(workflow.as_bytes())?;
    }
    Ok(())
}

/// Runs the `validate-config` subcommand, loading and checking the
/// configuration without calling any external API.
///
/// # Errors
///
/// Returns an error if the configuration is invalid or required values are
/// missing.
pub fn run_validate_config(args: &ValidateConfigArgs) -> Result<(), Box<dyn std::error::Error>> {
    let toml = load_toml_config(&args.config)?;
    let config = Config::from_env(toml.clone())?;

    let rules_enabled = Config::resolve_project_rules_enabled(
        false,
        toml.as_ref().and_then(|t| t.project_rules_enabled),
    );

    println!("✅ Configuration is valid.");
    println!("  Provider: {}", config.provider);
    println!("  Model:    {}", config.model);
    if let Some(variant) = &config.variant {
        println!("  Variant:  {}", variant);
    }
    println!("  API key env: {}", api_key_env_for(&config.provider));
    println!(
        "  API key set: {}",
        if std::env::var(api_key_env_for(&config.provider)).is_ok() {
            "yes"
        } else {
            "no"
        }
    );
    println!("  Important threshold: {}", config.important_threshold);

    let detected_rules = if let Some(path) = &config.rules_file {
        match load_rules_file(path) {
            Ok(rules) => Some(rules),
            Err(e) => {
                println!("  Warning: could not load explicit rules file: {}", e);
                None
            }
        }
    } else if rules_enabled {
        detect_project_rules(Path::new("."))?
    } else {
        None
    };

    for line in format_project_rules_validate_lines(
        rules_enabled,
        config.rules_file.as_deref(),
        detected_rules.as_ref(),
    ) {
        println!("{line}");
    }

    Ok(())
}

/// Formats the project rules status lines shown by `rs-guard validate-config`.
///
/// Each line includes the two-space indent used by the rest of the validation
/// output so callers can print them directly.
fn format_project_rules_validate_lines(
    enabled: bool,
    rules_file: Option<&Path>,
    rules: Option<&crate::rules::DetectedRules>,
) -> Vec<String> {
    let mut lines = Vec::new();
    if enabled {
        lines.push("  Project rules: ENABLED".to_string());
    } else {
        lines.push("  Project rules: DISABLED (project_rules_enabled=false)".to_string());
    }
    if let Some(path) = rules_file {
        lines.push(format!(
            "  Explicit rules file: {} (overrides auto-detection)",
            path.display()
        ));
        if let Some(r) = rules {
            lines.push(format_rules_size_line(r));
        }
    } else if enabled {
        match rules {
            Some(r) => {
                lines.push(format!("  Detected rules file: {}", r.path().display()));
                lines.push(format_rules_size_line(r));
            }
            None => {
                lines.push("  No rules file detected (using review-prompt.md fallback)".to_string())
            }
        }
    }
    lines
}

/// Formats the size and optional truncation status of a loaded rules file.
fn format_rules_size_line(rules: &crate::rules::DetectedRules) -> String {
    if rules.is_truncated() {
        format!(
            "  Rules file size: {} bytes (TRUNCATED to {} bytes)",
            rules.original_size(),
            rules.content().len()
        )
    } else {
        format!("  Rules file size: {} bytes", rules.original_size())
    }
}

/// Formats the project rules notice shown by `rs-guard init`.
fn format_project_rules_init_notice(rules: Option<&crate::rules::DetectedRules>) -> String {
    match rules {
        Some(r) => format!(
            "Detected project rules file: {} — rs-guard will use this for reviews. To disable project rules when reviewing, use `--no-project-rules`.",
            r.path().display()
        ),
        None => "No project rules file detected. rs-guard will use .github/review-prompt.md only. Consider adding an AGENTS.md for project-specific conventions.".to_string(),
    }
}

/// Detects the project type by inspecting files in the current directory.
///
/// Detection order:
/// 1. `Cargo.toml` → Rust
/// 2. `package.json` → Frontend SPA or Backend API (inspected for framework hints)
/// 3. `go.mod` → CLI tooling / systems program
/// 4. `pyproject.toml` / `requirements.txt` → Backend API
/// 5. fallback → General
fn detect_project_type() -> ProjectType {
    if Path::new("Cargo.toml").exists() {
        return ProjectType::Rust;
    }
    if Path::new("package.json").exists() {
        return detect_node_project_type();
    }
    if Path::new("go.mod").exists() {
        return ProjectType::CliTooling;
    }
    if Path::new("pyproject.toml").exists() || Path::new("requirements.txt").exists() {
        return ProjectType::BackendApi;
    }
    ProjectType::General
}

/// Inspects `package.json` to distinguish frontend SPAs from backend APIs.
///
/// Looks for common framework/runtime indicators in `dependencies` and
/// `devDependencies`. When neither side is strongly indicated, defaults to
/// `FrontendSpa` because `package.json` is most commonly used for web UIs.
fn detect_node_project_type() -> ProjectType {
    let content = fs::read_to_string("package.json").unwrap_or_default();
    let deps: String = content.to_ascii_lowercase();

    let frontend_indicators = [
        "react", "vue", "angular", "svelte", "solid-js", "preact", "lit", "vite", "webpack",
        "parcel", "rollup", "next", "nuxt", "astro", "remix", "gatsby", "expo",
    ];
    let backend_indicators = [
        "express",
        "fastify",
        "koa",
        "hapi",
        "restify",
        "nestjs",
        "@nestjs",
        "apollo-server",
        "trpc",
        "socket.io",
        "bull",
        "bullmq",
    ];

    let frontend_score = frontend_indicators
        .iter()
        .filter(|&&ind| deps.contains(&format!("\"{ind}\"")))
        .count();
    let backend_score = backend_indicators
        .iter()
        .filter(|&&ind| deps.contains(&format!("\"{ind}\"")))
        .count();

    if backend_score > frontend_score {
        ProjectType::BackendApi
    } else {
        ProjectType::FrontendSpa
    }
}

/// Maps a project type to a language string for generated guardrails.
fn language_for_project_type(project_type: ProjectType) -> Option<String> {
    match project_type {
        ProjectType::Rust => Some("rust".to_string()),
        ProjectType::CliTooling => Some("go".to_string()),
        ProjectType::BackendApi => Some("python".to_string()),
        ProjectType::FrontendSpa => Some("typescript".to_string()),
        ProjectType::General => None,
    }
}

/// Generates a review prompt from a template, focus items, and optional
/// language guardrails.
fn generate_prompt(args: &GeneratePromptArgs) -> String {
    let template = match args.template {
        PromptTemplate::General => include_str!("../examples/prompts/general-code-review.md"),
        PromptTemplate::BackendApi => include_str!("../examples/prompts/backend-api.md"),
        PromptTemplate::FrontendSpa => include_str!("../examples/prompts/frontend-spa.md"),
        PromptTemplate::CliTooling => include_str!("../examples/prompts/cli-tooling.md"),
    };

    let focus_items: Vec<String> = args.focus.clone();

    if focus_items.is_empty() && args.language.is_none() {
        return template.to_string();
    }

    let heading = "## Project-Specific Focus";
    let start = template.find(heading);
    let end = template.find("[RS_GUARD_VERDICT_METADATA]");

    let (before, after) = match (start, end) {
        (Some(s), Some(e)) if s < e => (&template[..s], &template[e..]),
        _ => (template, ""),
    };

    let mut result = String::new();
    result.push_str(before);
    result.push_str(heading);
    result.push('\n');
    for item in &focus_items {
        result.push_str("- ");
        result.push_str(item);
        result.push('\n');
    }
    if let Some(lang) = &args.language {
        let _ = write!(result, "\n## {} Guardrails\n", lang);
        if let Some(rules) = language_guardrails(lang) {
            result.push_str(rules);
            result.push('\n');
        }
    }
    result.push('\n');
    result.push_str(after);
    result
}

/// Generates a GitHub Actions workflow file from the provided arguments.
fn generate_workflow(args: &GenerateWorkflowArgs) -> Result<String, Box<dyn std::error::Error>> {
    let provider = args.provider.as_deref().unwrap_or(DEFAULT_PROVIDER);

    if find_provider(provider).is_none() {
        return Err(format!(
            "Unknown provider '{}'. Supported: {}",
            provider,
            known_provider_names().join(", ")
        )
        .into());
    }

    let secret = args
        .secret
        .clone()
        .unwrap_or_else(|| api_key_env_for(provider));
    let model = args.model.clone();
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));

    let (event, types, fork_guard) = if args.fork_safe {
        (
            "pull_request_target",
            "types: [opened, synchronize, reopened]",
            "\n    if: github.event.pull_request.head.repo.full_name == github.repository",
        )
    } else {
        ("pull_request", "types: [opened, synchronize, reopened]", "")
    };

    let mut run_line = format!("./rs-guard --provider {}", provider);
    if let Some(m) = &model {
        run_line.push_str(&format!(" --model {}", m));
    }
    run_line.push_str(" --prompt-file .github/review-prompt.md");

    let workflow = WORKFLOW_TEMPLATE
        .replace("{{PROVIDER}}", provider)
        .replace("{{EVENT}}", event)
        .replace("{{TYPES}}", types)
        .replace("{{FORK_GUARD}}", fork_guard)
        .replace("{{VERSION}}", &version)
        .replace("{{RUN_LINE}}", &run_line)
        .replace("{{SECRET}}", &secret);

    Ok(workflow)
}

/// Generates a minimal `.reviewer.toml` for the given provider.
fn generate_config(provider: &str) -> String {
    format!(
        r#"# rs-guard configuration
# See https://github.com/nebulaideas/rs-guard/blob/main/docs/CONFIGURATION.md

provider = "{provider}"
# model = "{default_model}"
temperature = 0.1
# important_issues_threshold = 3

# Project rules injection (v1.5.0)
# Auto-detects AGENTS.md, CLAUDE.md, .github/copilot-instructions.md,
# .gemini/styleguide.md, .cursor/rules/*.md, and .windsurfrules.
project_rules_enabled = true
# rules_file = "docs/my-project-rules.md"

[providers.{provider}]
api_key_env = "{api_key_env}"
# base_url = "{default_base_url}"
"#,
        default_model = find_provider(provider)
            .map(|m| m.default_model)
            .unwrap_or(""),
        api_key_env = api_key_env_for(provider),
        default_base_url = find_provider(provider)
            .map(|m| m.default_base_url)
            .unwrap_or(""),
    )
}

/// Writes `content` to `path`, creating parent directories as needed.
///
/// When `force` is `false`, an existing file is left untouched and a message is
/// printed instead.
fn write_file(path: &str, content: &str, force: bool) -> Result<(), io::Error> {
    let path = Path::new(path);
    if path.exists() && !force {
        println!(
            "⚠️  {} already exists; skipping (use --force to overwrite)",
            path.display()
        );
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)
}

/// Returns the standard API key environment variable name for a provider.
fn api_key_env_for(provider: &str) -> String {
    find_provider(provider)
        .map(|m| m.api_key_env.to_string())
        .unwrap_or_else(|| format!("{}_API_KEY", provider.to_uppercase()))
}

/// Returns the secret name as it appears in GitHub Actions.
fn api_key_secret_name(provider: &str) -> String {
    format!("${{{{ secrets.{} }}}}", api_key_env_for(provider))
}

/// Returns language-specific guardrail bullets, or `None` if the language is
/// not recognized.
fn language_guardrails(language: &str) -> Option<&'static str> {
    match language.to_ascii_lowercase().as_str() {
        "rust" => Some(
            "- No `unwrap()` or `expect()` outside `#[cfg(test)]` or `main()`.\n\
             - Prefer `?` and `anyhow::Context` for error propagation.\n\
             - Avoid `unsafe` blocks unless justified and documented.\n\
             - `tokio::spawn` tasks must be awaited or joined; no detached tasks.\n\
             - All public functions and types require doc comments (`#![deny(missing_docs)]`).",
        ),
        "typescript" | "ts" | "javascript" | "js" => Some(
            "- No `any` types in new code unless explicitly escaped.\n\
             - Avoid raw `fetch()` in components; use the project's data-fetching layer.\n\
             - Keep dependency arrays in hooks exhaustive and stable.\n\
             - Do not suppress accessibility warnings without a documented reason.",
        ),
        "go" | "golang" => Some(
            "- Check every error return; never ignore `err`.\n\
             - Use `context.Context` for cancellation in I/O and RPC paths.\n\
             - Avoid `panic` outside of `main` or initialization.\n\
             - Keep package APIs small and well-documented.",
        ),
        "python" | "py" => Some(
            "- Type hints are required for public function signatures.\n\
             - Handle exceptions at system boundaries; do not swallow `Exception`.\n\
             - Avoid blocking I/O inside async functions.\n\
             - Keep dependencies pinned and scanned for CVEs.",
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_prompt_with_focus_items() {
        let args = GeneratePromptArgs {
            template: PromptTemplate::General,
            focus: vec!["No unwrap in production code".to_string()],
            language: None,
            output: None,
        };
        let prompt = generate_prompt(&args);
        assert!(prompt.contains("## Project-Specific Focus"));
        assert!(prompt.contains("- No unwrap in production code"));
        assert!(prompt.contains("[RS_GUARD_VERDICT_METADATA]"));
    }

    #[test]
    fn test_generate_prompt_with_language_guardrails() {
        let args = GeneratePromptArgs {
            template: PromptTemplate::General,
            focus: Vec::new(),
            language: Some("rust".to_string()),
            output: None,
        };
        let prompt = generate_prompt(&args);
        assert!(prompt.contains("## rust Guardrails"));
        assert!(prompt.contains("No `unwrap()`"));
    }

    #[test]
    fn test_generate_prompt_without_focus_preserves_template() {
        let args = GeneratePromptArgs {
            template: PromptTemplate::General,
            focus: Vec::new(),
            language: None,
            output: None,
        };
        let prompt = generate_prompt(&args);
        assert!(prompt.contains("Five Review Axes"));
    }

    #[test]
    fn test_generate_workflow_replaces_provider_and_secret() {
        let args = GenerateWorkflowArgs {
            provider: Some("kimi".to_string()),
            model: Some("kimi-k2.5".to_string()),
            secret: Some("KIMI_API_KEY".to_string()),
            fork_safe: false,
            output: None,
        };
        let workflow = generate_workflow(&args).unwrap();
        assert!(workflow.contains("rs-guard --provider kimi --model kimi-k2.5"));
        assert!(workflow.contains("KIMI_API_KEY"));
        assert!(!workflow.contains("DEEPSEEK_API_KEY"));
        assert!(workflow.contains(&format!("v{}", env!("CARGO_PKG_VERSION"))));
    }

    #[test]
    fn test_generate_workflow_fork_safe() {
        let args = GenerateWorkflowArgs {
            provider: None,
            model: None,
            secret: None,
            fork_safe: true,
            output: None,
        };
        let workflow = generate_workflow(&args).unwrap();
        assert!(workflow.contains("pull_request_target"));
        assert!(workflow.contains("head.repo.full_name == github.repository"));
    }

    #[test]
    fn test_generate_config_includes_provider() {
        let config = generate_config("deepseek");
        assert!(config.contains("provider = \"deepseek\""));
        assert!(config.contains("DEEPSEEK_API_KEY"));
    }

    #[test]
    #[serial_test::serial]
    fn test_detect_project_type_rust() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        assert_eq!(detect_project_type(), ProjectType::Rust);
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn test_detect_project_type_node_frontend() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies": {"react": "^18.0.0", "vite": "^4.0.0"}}"#,
        )
        .unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        assert_eq!(detect_project_type(), ProjectType::FrontendSpa);
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn test_detect_project_type_node_backend() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies": {"express": "^4.0.0", "bullmq": "^4.0.0"}}"#,
        )
        .unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        assert_eq!(detect_project_type(), ProjectType::BackendApi);
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn test_detect_project_type_go() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("go.mod"), "module example\n").unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        assert_eq!(detect_project_type(), ProjectType::CliTooling);
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn test_detect_project_type_python() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[project]\n").unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        assert_eq!(detect_project_type(), ProjectType::BackendApi);
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn test_detect_project_type_general_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        assert_eq!(detect_project_type(), ProjectType::General);
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_language_for_project_type_returns_guardrail_languages() {
        assert_eq!(
            language_for_project_type(ProjectType::Rust),
            Some("rust".to_string())
        );
        assert_eq!(
            language_for_project_type(ProjectType::CliTooling),
            Some("go".to_string())
        );
        assert_eq!(
            language_for_project_type(ProjectType::BackendApi),
            Some("python".to_string())
        );
        assert_eq!(
            language_for_project_type(ProjectType::FrontendSpa),
            Some("typescript".to_string())
        );
        assert_eq!(language_for_project_type(ProjectType::General), None);
    }

    #[test]
    fn test_language_guardrails_unknown_language() {
        assert!(language_guardrails("elixir").is_none());
    }

    #[test]
    fn test_generate_config_includes_project_rules_keys() {
        let config = generate_config("deepseek");
        assert!(config.contains("project_rules_enabled = true"));
        assert!(config.contains("# rules_file = \"docs/my-project-rules.md\""));
        assert!(config.contains("AGENTS.md"));
        assert!(config.contains("CLAUDE.md"));
    }

    #[test]
    fn test_format_project_rules_init_notice_with_rules() {
        let rules = crate::rules::DetectedRules::new(
            crate::rules::RulesFilePath::new(Path::new("AGENTS.md").to_path_buf()),
            crate::rules::RulesContent::new("# rules".to_string()),
            crate::rules::RulesFileSize::new(42),
            false,
        );
        let notice = format_project_rules_init_notice(Some(&rules));
        assert!(notice.contains("Detected project rules file: AGENTS.md"));
        assert!(
            notice.contains("To disable project rules when reviewing, use `--no-project-rules`")
        );
    }

    #[test]
    fn test_format_project_rules_init_notice_without_rules() {
        let notice = format_project_rules_init_notice(None);
        assert!(notice.contains("No project rules file detected"));
        assert!(notice.contains(".github/review-prompt.md"));
        assert!(notice.contains("AGENTS.md"));
    }

    #[test]
    fn test_format_project_rules_validate_lines_enabled_with_detected() {
        let rules = crate::rules::DetectedRules::new(
            crate::rules::RulesFilePath::new(Path::new("CLAUDE.md").to_path_buf()),
            crate::rules::RulesContent::new("# rules".to_string()),
            crate::rules::RulesFileSize::new(100),
            false,
        );
        let lines = format_project_rules_validate_lines(true, None, Some(&rules));
        assert_eq!(lines[0], "  Project rules: ENABLED");
        assert!(lines[1].contains("Detected rules file: CLAUDE.md"));
        assert!(lines[2].contains("Rules file size: 100 bytes"));
    }

    #[test]
    fn test_format_project_rules_validate_lines_disabled() {
        let lines = format_project_rules_validate_lines(false, None, None);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0],
            "  Project rules: DISABLED (project_rules_enabled=false)"
        );
    }

    #[test]
    fn test_format_project_rules_validate_lines_explicit_file() {
        let rules = crate::rules::DetectedRules::new(
            crate::rules::RulesFilePath::new(Path::new("custom.md").to_path_buf()),
            crate::rules::RulesContent::new("# rules".to_string()),
            crate::rules::RulesFileSize::new(50),
            false,
        );
        let lines =
            format_project_rules_validate_lines(true, Some(Path::new("custom.md")), Some(&rules));
        assert_eq!(lines[0], "  Project rules: ENABLED");
        assert!(lines[1].contains("Explicit rules file: custom.md"));
        assert!(lines[2].contains("Rules file size: 50 bytes"));
    }

    #[test]
    fn test_format_project_rules_validate_lines_truncated() {
        let rules = crate::rules::DetectedRules::new(
            crate::rules::RulesFilePath::new(Path::new("AGENTS.md").to_path_buf()),
            crate::rules::RulesContent::new("x".repeat(100)),
            crate::rules::RulesFileSize::new(1000),
            true,
        );
        let lines = format_project_rules_validate_lines(true, None, Some(&rules));
        assert!(lines[2].contains("TRUNCATED"));
        assert!(lines[2].contains("1000 bytes"));
        assert!(lines[2].contains("100 bytes"));
    }

    #[test]
    fn test_format_project_rules_validate_lines_no_rules() {
        let lines = format_project_rules_validate_lines(true, None, None);
        assert_eq!(lines[0], "  Project rules: ENABLED");
        assert!(lines[1].contains("No rules file detected"));
    }

    #[test]
    fn test_format_project_rules_validate_lines_explicit_missing() {
        let lines = format_project_rules_validate_lines(true, Some(Path::new("missing.md")), None);
        assert_eq!(lines[0], "  Project rules: ENABLED");
        assert!(lines[1].contains("Explicit rules file: missing.md"));
        assert_eq!(lines.len(), 2);
    }
}
