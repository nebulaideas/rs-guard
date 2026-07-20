//! CLI argument definitions using `clap` derive macros.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Top-level CLI entry point for rs-guard.
///
/// Supports subcommands for setup automation while preserving the original
/// bare-flag invocation for running reviews.
#[derive(Parser, Debug, Clone)]
#[command(name = "rs-guard")]
#[command(about = "AI-powered code review CLI for GitHub PRs")]
#[command(version = env!("CARGO_PKG_VERSION"))]
pub struct Cli {
    /// Subcommand to run. When omitted, rs-guard runs the review pipeline
    /// using the top-level review flags.
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Arguments for the default review command.
    #[command(flatten)]
    pub review: ReviewArgs,
}

/// Available rs-guard subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Scaffold rs-guard configuration and workflow in the current repository.
    Init(InitArgs),

    /// Generate a review prompt template.
    GeneratePrompt(GeneratePromptArgs),

    /// Generate a GitHub Actions workflow file.
    GenerateWorkflow(GenerateWorkflowArgs),

    /// Validate configuration without running a review.
    ValidateConfig(ValidateConfigArgs),
}

/// Arguments for the default review command.
///
/// These flags remain available at the top level so existing invocations such
/// as `rs-guard --prompt-file .github/review-prompt.md` continue to work.
#[derive(Parser, Debug, Clone)]
pub struct ReviewArgs {
    /// Path to system prompt markdown file.
    #[arg(
        short,
        long,
        default_value = ".github/review-prompt.md",
        help = "Path to system prompt markdown file"
    )]
    pub prompt_file: PathBuf,

    /// LLM model identifier (default: provider-specific).
    #[arg(
        short,
        long,
        help = "LLM model identifier (default: provider-specific)"
    )]
    pub model: Option<String>,

    /// Provider-specific model variant (e.g. "flash", "pro" for deepseek).
    #[arg(
        long,
        env = "RS_GUARD_VARIANT",
        help = "Provider-specific model variant (e.g. flash/pro for deepseek). Has no effect for providers that do not declare variants."
    )]
    pub variant: Option<String>,

    /// Sampling temperature (0.0 - 2.0). Default: 0.1.
    #[arg(
        short,
        long,
        help = "Sampling temperature (0.0 - 2.0) [default: 0.1]",
        value_parser = parse_temperature
    )]
    pub temperature: Option<f32>,

    /// LLM provider to use. Default: deepseek.
    #[arg(
        long,
        env = "RS_GUARD_PROVIDER",
        help = "LLM provider to use [default: deepseek]"
    )]
    pub provider: Option<String>,

    /// Path to configuration TOML file.
    #[arg(
        short,
        long,
        default_value = ".reviewer.toml",
        help = "Path to configuration TOML file"
    )]
    pub config: PathBuf,

    /// Maximum tokens for LLM completions.
    #[arg(long, help = "Maximum tokens for LLM completions")]
    pub max_tokens: Option<u32>,

    /// Timeout in seconds for LLM API requests (total).
    #[arg(long, help = "Timeout in seconds for LLM API requests [default: 120]")]
    pub llm_timeout: Option<u64>,

    /// Threshold of "Important" issues required to REQUEST_CHANGES.
    #[arg(
        long,
        env = "RS_GUARD_IMPORTANT_THRESHOLD",
        help = "Threshold of \"Important\" issues required to REQUEST_CHANGES [default: 3]"
    )]
    pub important_threshold: Option<u32>,

    /// Path to a pre-existing diff file to review instead of fetching from GitHub.
    ///
    /// When set, rs-guard reads the diff content from this file path
    /// instead of calling the GitHub API. Useful in CI when the diff has
    /// already been generated (e.g. by `git diff` or a prior workflow step).
    /// If the file does not exist, an error is returned.
    #[arg(
        long,
        env = "RS_GUARD_DIFF_FILE",
        help = "Path to a pre-existing diff file to review"
    )]
    pub diff_file: Option<String>,

    /// Bypass the response cache, forcing an LLM API call.
    #[arg(long, help = "Bypass response cache and force LLM API call")]
    pub no_cache: bool,

    /// Run the full pipeline but do not submit reviews or block commits.
    ///
    /// Useful for testing configuration and prompt changes without affecting
    /// the repository. Always exits with code 0.
    #[arg(long, help = "Dry-run mode: review without submitting or blocking")]
    pub dry_run: bool,

    /// Maximum diff size in bytes (default: 512000).
    #[arg(
        long,
        env = "RS_GUARD_MAX_DIFF_BYTES",
        help = "Maximum accepted diff size in bytes"
    )]
    pub max_diff_bytes: Option<usize>,

    /// Maximum diff line count (default: 5000).
    #[arg(
        long,
        env = "RS_GUARD_MAX_DIFF_LINES",
        help = "Maximum accepted diff line count"
    )]
    pub max_diff_lines: Option<usize>,

    /// Comma-separated path include globs (empty = all paths).
    #[arg(
        long,
        env = "RS_GUARD_INCLUDE_PATHS",
        help = "Comma-separated path include globs"
    )]
    pub include_paths: Option<String>,

    /// Comma-separated path exclude globs.
    #[arg(
        long,
        env = "RS_GUARD_EXCLUDE_PATHS",
        help = "Comma-separated path exclude globs"
    )]
    pub exclude_paths: Option<String>,

    /// Disable auto-detection of project rules files (AGENTS.md, CLAUDE.md, etc.).
    ///
    /// When set, rs-guard will not scan for or inject project-specific coding
    /// conventions into the review prompt. Overrides `RS_GUARD_NO_PROJECT_RULES`
    /// env var and `project_rules_enabled` TOML key.
    #[arg(long, help = "Disable project rules auto-detection and injection")]
    pub no_project_rules: bool,

    /// Path to an explicit project rules file.
    ///
    /// When set, rs-guard loads this file instead of auto-detecting
    /// `AGENTS.md`, `CLAUDE.md`, etc. The path may be relative to the current
    /// working directory or absolute. Overrides `RS_GUARD_RULES_FILE` env var
    /// and `rules_file` TOML key. Mutually exclusive with `--no-project-rules`.
    #[arg(
        long,
        env = "RS_GUARD_RULES_FILE",
        help = "Path to explicit project rules file (overrides auto-detection)"
    )]
    pub rules_file: Option<PathBuf>,
}

/// Project type used by `rs-guard init` to select appropriate templates.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectType {
    /// Rust project.
    Rust,
    /// Backend / API service.
    BackendApi,
    /// Frontend single-page application.
    FrontendSpa,
    /// CLI tool or systems program.
    CliTooling,
    /// Language-agnostic general review.
    General,
}

/// Arguments for the `init` subcommand.
#[derive(Parser, Debug, Clone)]
pub struct InitArgs {
    /// Project type to scaffold for.
    #[arg(long = "type", value_enum, help = "Project type to scaffold for")]
    pub project_type: Option<ProjectType>,

    /// LLM provider to configure.
    #[arg(long, help = "LLM provider to configure [default: deepseek]")]
    pub provider: Option<String>,

    /// Overwrite existing scaffold files.
    #[arg(long, help = "Overwrite existing scaffold files")]
    pub force: bool,
}

/// Arguments for the `generate-prompt` subcommand.
#[derive(Parser, Debug, Clone)]
pub struct GeneratePromptArgs {
    /// Prompt template to base the output on.
    #[arg(
        long,
        value_enum,
        default_value = "general",
        help = "Prompt template to use"
    )]
    pub template: PromptTemplate,

    /// Additional focus item to inject into the prompt.
    #[arg(long, help = "Focus item to add (can be repeated)")]
    pub focus: Vec<String>,

    /// Programming language to add stack-specific guardrails for.
    #[arg(long, help = "Programming language for stack-specific guardrails")]
    pub language: Option<String>,

    /// Output file path. When omitted, prints to stdout.
    #[arg(short, long, help = "Output file path")]
    pub output: Option<PathBuf>,
}

/// Prompt template used by `generate-prompt`.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptTemplate {
    /// Canonical agnostic baseline.
    General,
    /// Backend services and APIs.
    BackendApi,
    /// Frontend single-page applications.
    FrontendSpa,
    /// CLI tools and systems programs.
    CliTooling,
}

/// Arguments for the `generate-workflow` subcommand.
#[derive(Parser, Debug, Clone)]
pub struct GenerateWorkflowArgs {
    /// LLM provider to use in the workflow.
    #[arg(long, help = "LLM provider to use [default: deepseek]")]
    pub provider: Option<String>,

    /// LLM model identifier.
    #[arg(short, long, help = "LLM model identifier")]
    pub model: Option<String>,

    /// Environment variable name holding the provider API key.
    #[arg(long, help = "API key secret name [default: <PROVIDER>_API_KEY]")]
    pub secret: Option<String>,

    /// Emit a fork-safe workflow using `pull_request_target`.
    #[arg(long, help = "Emit a fork-safe workflow")]
    pub fork_safe: bool,

    /// Output file path. When omitted, prints to stdout.
    #[arg(short, long, help = "Output file path")]
    pub output: Option<PathBuf>,
}

/// Arguments for the `validate-config` subcommand.
#[derive(Parser, Debug, Clone)]
pub struct ValidateConfigArgs {
    /// Path to configuration TOML file.
    #[arg(
        short,
        long,
        default_value = ".reviewer.toml",
        help = "Path to configuration TOML file"
    )]
    pub config: PathBuf,
}

/// Validates that a temperature value is within the OpenAI-compatible range (0.0 - 2.0).
fn parse_temperature(s: &str) -> Result<f32, String> {
    let v: f32 = s
        .parse()
        .map_err(|e| format!("Invalid temperature '{}': {}", s, e))?;
    if !(0.0..=2.0).contains(&v) {
        return Err(format!(
            "Temperature must be between 0.0 and 2.0, got: {}",
            v
        ));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_temperature_valid() {
        assert_eq!(parse_temperature("0.0").unwrap(), 0.0);
        assert_eq!(parse_temperature("0.1").unwrap(), 0.1);
        assert_eq!(parse_temperature("1.0").unwrap(), 1.0);
        assert_eq!(parse_temperature("2.0").unwrap(), 2.0);
    }

    #[test]
    fn test_parse_temperature_out_of_range() {
        assert!(parse_temperature("-0.1").is_err());
        assert!(parse_temperature("2.1").is_err());
        assert!(parse_temperature("5.0").is_err());
    }

    #[test]
    fn test_parse_temperature_invalid_string() {
        assert!(parse_temperature("not-a-number").is_err());
        assert!(parse_temperature("").is_err());
    }

    #[test]
    fn test_dry_run_flag_parsing() {
        let cli = Cli::parse_from(["rs-guard", "--dry-run"]);
        assert!(cli.review.dry_run);
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_dry_run_flag_default_false() {
        let cli = Cli::parse_from(["rs-guard"]);
        assert!(!cli.review.dry_run);
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_no_project_rules_flag_default_false() {
        let cli = Cli::parse_from(["rs-guard"]);
        assert!(
            !cli.review.no_project_rules,
            "no_project_rules should default to false"
        );
    }

    #[test]
    fn test_no_project_rules_flag_parsing() {
        let cli = Cli::parse_from(["rs-guard", "--no-project-rules"]);
        assert!(
            cli.review.no_project_rules,
            "--no-project-rules should set no_project_rules to true"
        );
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_rules_file_flag_parsing() {
        let cli = Cli::parse_from(["rs-guard", "--rules-file", "custom-rules.md"]);
        assert_eq!(
            cli.review.rules_file,
            Some(PathBuf::from("custom-rules.md")),
            "--rules-file should set rules_file"
        );
    }

    #[test]
    fn test_rules_file_flag_default_none() {
        let cli = Cli::parse_from(["rs-guard"]);
        assert!(
            cli.review.rules_file.is_none(),
            "rules_file should default to None"
        );
    }

    #[test]
    fn test_init_subcommand_parsing() {
        let cli = Cli::parse_from([
            "rs-guard",
            "init",
            "--type",
            "rust",
            "--provider",
            "kimi",
            "--force",
        ]);
        match cli.command {
            Some(Commands::Init(args)) => {
                assert_eq!(args.project_type, Some(ProjectType::Rust));
                assert_eq!(args.provider, Some("kimi".to_string()));
                assert!(args.force);
            }
            _ => panic!("expected Init subcommand"),
        }
    }

    #[test]
    fn test_generate_prompt_subcommand_parsing() {
        let cli = Cli::parse_from([
            "rs-guard",
            "generate-prompt",
            "--template",
            "backend-api",
            "--focus",
            "No N+1 queries",
            "--language",
            "rust",
        ]);
        match cli.command {
            Some(Commands::GeneratePrompt(args)) => {
                assert_eq!(args.template, PromptTemplate::BackendApi);
                assert_eq!(args.focus, vec!["No N+1 queries".to_string()]);
                assert_eq!(args.language, Some("rust".to_string()));
            }
            _ => panic!("expected GeneratePrompt subcommand"),
        }
    }

    #[test]
    fn test_generate_workflow_subcommand_parsing() {
        let cli = Cli::parse_from([
            "rs-guard",
            "generate-workflow",
            "--provider",
            "openai",
            "--secret",
            "OPENAI_API_KEY",
            "--fork-safe",
        ]);
        match cli.command {
            Some(Commands::GenerateWorkflow(args)) => {
                assert_eq!(args.provider, Some("openai".to_string()));
                assert_eq!(args.secret, Some("OPENAI_API_KEY".to_string()));
                assert!(args.fork_safe);
            }
            _ => panic!("expected GenerateWorkflow subcommand"),
        }
    }

    #[test]
    fn test_validate_config_subcommand_parsing() {
        let cli = Cli::parse_from(["rs-guard", "validate-config", "--config", "custom.toml"]);
        match cli.command {
            Some(Commands::ValidateConfig(args)) => {
                assert_eq!(args.config, PathBuf::from("custom.toml"));
            }
            _ => panic!("expected ValidateConfig subcommand"),
        }
    }
}
