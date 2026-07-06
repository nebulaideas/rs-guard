//! rs-guard CLI entry point.
//!
//! Parses CLI args, loads configuration, dispatches subcommands, runs the
//! review pipeline, and maps [`PipelineResult`] to process exit codes.

use clap::Parser;
use colored::Colorize;
use rs_guard::cli::{Cli, Commands};
use rs_guard::config::{load_toml_config, Config};
use rs_guard::pipeline::{run_pipeline, PipelineResult};
use rs_guard::scaffold;
use std::process;

fn exit_on_error<T>(result: Result<T, impl std::fmt::Display>, context: &str) -> T {
    result.unwrap_or_else(|e| {
        eprintln!("{}: {}", context, e);
        process::exit(1);
    })
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let cli = Cli::parse();

    if let Some(command) = cli.command {
        let result = match command {
            Commands::Init(args) => scaffold::run_init(&args),
            Commands::GeneratePrompt(args) => scaffold::run_generate_prompt(&args),
            Commands::GenerateWorkflow(args) => scaffold::run_generate_workflow(&args),
            Commands::ValidateConfig(args) => scaffold::run_validate_config(&args),
        };
        if let Err(e) = result {
            eprintln!("Error: {:#}", e);
            process::exit(1);
        }
        return;
    }

    let args = cli.review;

    let toml_config = exit_on_error(
        load_toml_config(&args.config),
        "Failed to load TOML configuration",
    );

    // Extract project_rules_enabled before toml_config is moved into from_env
    let toml_project_rules_enabled = toml_config.as_ref().and_then(|t| t.project_rules_enabled);

    let mut config = exit_on_error(
        Config::from_env(toml_config),
        "Failed to load configuration",
    );

    exit_on_error(config.apply_args(&args), "Failed to apply CLI arguments");

    exit_on_error(
        config.load_prompt_file(&args.prompt_file),
        "Failed to load prompt file",
    );

    // Resolve and load project rules (AGENTS.md, CLAUDE.md, etc.)
    let project_rules_enabled =
        Config::resolve_project_rules_enabled(args.no_project_rules, toml_project_rules_enabled);
    let repo_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    exit_on_error(
        config.load_project_rules(&repo_root, project_rules_enabled),
        "Failed to load project rules",
    );

    // Print notice when project rules are loaded (silent opt-out with --no-project-rules)
    if !args.no_project_rules {
        if let (Some(ref rules), Some(ref path)) =
            (&config.project_rules, &config.project_rules_file)
        {
            eprintln!(
                "{} Project rules loaded from: {} ({} bytes)",
                "info:".cyan(),
                path,
                rules.len()
            );
        }
    }

    if config.is_ci {
        config.validate_for_ci().unwrap_or_else(|e| {
            eprintln!("Configuration validation failed: {}", e);
            process::exit(1);
        });
    }

    let diff_file = args.diff_file.as_deref();

    log::info!(
        "rs-guard starting (provider: {}, model: {})",
        config.provider,
        config.model
    );

    let result = run_pipeline(config, diff_file).await;

    match result {
        Ok(PipelineResult::Success) => {
            process::exit(0);
        }
        Ok(PipelineResult::ReviewBlocked) => {
            process::exit(2);
        }
        Err(e) => {
            eprintln!("Error: {:#}", e);
            process::exit(1);
        }
    }
}
