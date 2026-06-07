//! CLI argument definitions using `clap` derive macros.

use clap::Parser;
use std::path::PathBuf;

/// Command-line arguments for the diffguard review tool.
#[derive(Parser, Debug, Clone)]
#[command(name = "diffguard")]
#[command(about = "AI-powered code review CLI for GitHub PRs")]
#[command(version = env!("CARGO_PKG_VERSION"))]
pub struct Args {
    /// Path to system prompt markdown file.
    #[arg(
        short,
        long,
        default_value = ".github/review-prompt.md",
        help = "Path to system prompt markdown file"
    )]
    pub prompt_file: PathBuf,

    /// LLM model identifier.
    #[arg(
        short,
        long,
        default_value = "deepseek-v4-flash",
        help = "LLM model identifier"
    )]
    pub model: String,

    /// Sampling temperature (0.0 - 2.0).
    #[arg(
        short,
        long,
        default_value_t = 0.1,
        help = "Sampling temperature (0.0 - 2.0)"
    )]
    pub temperature: f32,

    /// LLM provider to use.
    #[arg(
        long,
        env = "DIFFGUARD_PROVIDER",
        default_value = "deepseek",
        help = "LLM provider to use"
    )]
    pub provider: String,
}
