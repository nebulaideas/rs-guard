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

    /// LLM model identifier (default: provider-specific).
    #[arg(
        short,
        long,
        help = "LLM model identifier (default: provider-specific)"
    )]
    pub model: Option<String>,

    /// Sampling temperature (0.0 - 2.0). Default: 0.1.
    #[arg(short, long, help = "Sampling temperature (0.0 - 2.0) [default: 0.1]")]
    pub temperature: Option<f32>,

    /// LLM provider to use. Default: deepseek.
    #[arg(long, help = "LLM provider to use [default: deepseek]")]
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
}
