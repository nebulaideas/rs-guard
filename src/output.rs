//! Terminal output formatting and review artifact writing.
//!
//! Provides functions for writing structured review artifacts and printing
//! color-coded summaries to the terminal for local mode.

use crate::verdict::{ReviewState, Verdict};
use colored::Colorize;
use std::io::Write;

/// Default filename for the review result artifact.
pub const ARTIFACT_FILENAME: &str = "review-result.txt";

/// Metadata about the review run, used for artifact and console output.
#[derive(Debug, Clone)]
pub struct ReviewConfig {
    /// LLM provider name.
    pub provider: String,
    /// Model identifier.
    pub model: String,
    /// Sampling temperature used.
    pub temperature: f32,
    /// Pull request number (if in CI mode).
    pub pr_number: Option<u64>,
    /// Diff size in bytes.
    pub diff_size_bytes: usize,
    /// Diff line count.
    pub diff_line_count: usize,
}

/// Writes a structured review result file for downstream CI jobs.
///
/// The artifact includes provider metadata, the full LLM review text,
/// and the parsed verdict metadata.
///
/// # Errors
///
/// Returns [`std::io::Error`] if the file cannot be created or written.
pub fn write_artifact(
    review: &str,
    verdict: &Verdict,
    state: &ReviewState,
    config: &ReviewConfig,
    path: &str,
) -> std::io::Result<()> {
    let content = format!(
        "diffguard-rs Review Result
==========================
Provider: {}
Model: {}
Temperature: {}
Diff Size: {} lines ({} bytes)
Review State: {}

--- LLM Review ---
{}

--- Parsed Metadata ---
Verdict: {}
CriticalBugs: {}
SecurityIssues: {}
",
        config.provider,
        config.model,
        config.temperature,
        config.diff_line_count,
        config.diff_size_bytes,
        state,
        review,
        verdict.verdict,
        verdict.critical_bugs,
        verdict.security_issues,
    );

    let mut file = std::fs::File::create(path)?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

/// Prints the full review text with a color-coded state header.
pub fn print_colored_report(review: &str, verdict: &Verdict, state: &ReviewState) {
    println!("{}", "diffguard-rs Review".bold().underline());
    println!();

    match state {
        ReviewState::Approve => {
            println!("{}", "✓ State: APPROVE".green().bold());
        }
        ReviewState::RequestChanges => {
            println!("{}", "✗ State: REQUEST_CHANGES".red().bold());
        }
        ReviewState::Comment => {
            println!("{}", "→ State: COMMENT".yellow().bold());
        }
    }

    println!();
    println!("Verdict:         {}", verdict.verdict);
    println!("Critical Bugs:   {}", verdict.critical_bugs);
    println!("Security Issues: {}", verdict.security_issues);
    println!();
    println!("{}", review);
}

/// Prints a colored summary of the review with full metadata.
///
/// Used in local mode to display the review result in the terminal.
pub fn print_colored_summary(
    review: &str,
    verdict: &Verdict,
    state: &ReviewState,
    config: &ReviewConfig,
) {
    print_colored_report(review, verdict, state);

    println!();
    println!("{}", "--- Metadata ---".dimmed());
    println!("Provider:    {}", config.provider);
    println!("Model:       {}", config.model);
    println!("Temperature: {}", config.temperature);
    println!("Diff Lines:  {}", config.diff_line_count);
}
