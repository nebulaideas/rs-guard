//! diffguard CLI entry point.
//!
//! Parses CLI args, loads configuration, runs the pipeline, and maps
//! [`PipelineResult`] to process exit codes.

use clap::Parser;
use diffguard::cli::Args;
use diffguard::config::{load_toml_config, Config};
use diffguard::pipeline::{run_pipeline, PipelineResult};
use std::process;

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();

    let toml_config = load_toml_config(&args.config).unwrap_or_else(|e| {
        eprintln!("Failed to load TOML configuration: {}", e);
        process::exit(1);
    });

    let mut config = Config::from_env(toml_config).unwrap_or_else(|e| {
        eprintln!("Failed to load configuration: {}", e);
        process::exit(1);
    });

    config.apply_args(&args).unwrap_or_else(|e| {
        eprintln!("Failed to apply CLI arguments: {}", e);
        process::exit(1);
    });

    config
        .load_prompt_file(&args.prompt_file)
        .unwrap_or_else(|e| {
            eprintln!("Failed to load prompt file: {}", e);
            process::exit(1);
        });

    let diff_file = args.diff_file.as_deref();

    config.validate_for_ci().unwrap_or_else(|e| {
        eprintln!("Configuration validation failed: {}", e);
        process::exit(1);
    });

    log::info!(
        "diffguard-rs starting (provider: {}, model: {})",
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
