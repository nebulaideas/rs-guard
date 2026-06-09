//! rs-guard CLI entry point.
//!
//! Parses CLI args, loads configuration, runs the pipeline, and maps
//! [`PipelineResult`] to process exit codes.

use clap::Parser;
use rs_guard::cli::Args;
use rs_guard::config::{load_toml_config, Config};
use rs_guard::pipeline::{run_pipeline, PipelineResult};
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
    let args = Args::parse();

    let toml_config = exit_on_error(
        load_toml_config(&args.config),
        "Failed to load TOML configuration",
    );

    let mut config = exit_on_error(
        Config::from_env(toml_config),
        "Failed to load configuration",
    );

    exit_on_error(config.apply_args(&args), "Failed to apply CLI arguments");

    exit_on_error(
        config.load_prompt_file(&args.prompt_file),
        "Failed to load prompt file",
    );

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
