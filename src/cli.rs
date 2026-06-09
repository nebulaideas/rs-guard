//! CLI argument definitions using `clap` derive macros.

use clap::Parser;
use std::path::PathBuf;

/// Command-line arguments for the rs-guard review tool.
#[derive(Parser, Debug, Clone)]
#[command(name = "rs-guard")]
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
        let args = Args::parse_from(["rs-guard", "--dry-run"]);
        assert!(args.dry_run);
    }

    #[test]
    fn test_dry_run_flag_default_false() {
        let args = Args::parse_from(["rs-guard"]);
        assert!(!args.dry_run);
    }
}
