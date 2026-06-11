//! Terminal output formatting and review artifact writing.
//!
//! Provides functions for writing structured review artifacts and printing
//! color-coded summaries to the terminal for local mode.

use crate::verdict::{ReviewState, Verdict};
use colored::Colorize;
use serde::Serialize;
use std::io::Write;

/// Default filename for the review result artifact.
pub const ARTIFACT_FILENAME: &str = "review-result.txt";

/// Default filename for the metrics JSON artifact.
pub const METRICS_FILENAME: &str = "rs-guard-metrics.json";

/// Per-run metrics for observability and cost tracking.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewMetrics {
    /// LLM provider name.
    pub provider: String,
    /// Model identifier.
    pub model: String,
    /// Estimated input tokens sent to the LLM (character count / 4 heuristic).
    pub estimated_tokens_in: usize,
    /// Estimated output tokens received from the LLM (character count / 4 heuristic).
    pub estimated_tokens_out: usize,
    /// API latency in seconds.
    pub latency_secs: f64,
    /// Estimated cost in cents (USD).
    pub estimated_cost_cents: f64,
    /// Diff size in lines.
    pub diff_lines: usize,
    /// Parsed verdict string.
    pub verdict: String,
    /// Review state.
    pub state: String,
}

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
/// and the parsed verdict metadata. The `--- Parsed Metadata ---` section
/// renders all five verdict fields: `Verdict`, `CriticalIssues`,
/// `SecurityIssues`, `ImportantIssues`, and `Suggestions`.
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
        "rs-guard Review Result
==========================
Provider: {}
Model: {}
Temperature: {}
Diff Size: {} lines ({} bytes)
Review State: {}

--- LLM Review ---
{}

--- Parsed Metadata ---
Verdict:         {}
CriticalIssues:  {}
SecurityIssues:  {}
ImportantIssues: {}
Suggestions:     {}
",
        config.provider,
        config.model,
        config.temperature,
        config.diff_line_count,
        config.diff_size_bytes,
        state,
        review,
        verdict.verdict,
        verdict.critical_issues,
        verdict.security_issues,
        verdict.important_issues,
        verdict.suggestions,
    );

    // Create parent directory if it doesn't exist
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let mut file = std::fs::File::create(path)?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

/// Prints the full review text with a color-coded state header to a writer.
///
/// Renders all five verdict fields: `Verdict`, `Critical Issues`,
/// `Security Issues`, `Important Issues`, and `Suggestions`.
/// Accepts any [`std::io::Write`] implementation for testability.
/// The `colored` crate's ANSI codes are preserved in the output.
///
/// # Arguments
///
/// * `review` — The full LLM review text.
/// * `verdict` — Parsed verdict metadata.
/// * `state` — Determined review state.
/// * `writer` — Output destination (e.g. `std::io::stdout()`, `Vec<u8>`).
///
/// # Errors
///
/// Returns [`std::io::Error`] if writing to the output fails.
pub fn print_colored_report(
    review: &str,
    verdict: &Verdict,
    state: &ReviewState,
    writer: &mut impl Write,
) -> std::io::Result<()> {
    writeln!(writer, "{}", "rs-guard Review".bold().underline())?;
    writeln!(writer)?;

    match state {
        ReviewState::Approve => {
            writeln!(writer, "{}", "✓ State: APPROVE".green().bold())?;
        }
        ReviewState::RequestChanges => {
            writeln!(writer, "{}", "✗ State: REQUEST_CHANGES".red().bold())?;
        }
        ReviewState::Comment => {
            writeln!(writer, "{}", "→ State: COMMENT".yellow().bold())?;
        }
    }

    writeln!(writer)?;
    writeln!(writer, "Verdict:          {}", verdict.verdict)?;
    writeln!(writer, "Critical Issues:  {}", verdict.critical_issues)?;
    writeln!(writer, "Security Issues:  {}", verdict.security_issues)?;
    writeln!(writer, "Important Issues: {}", verdict.important_issues)?;
    writeln!(writer, "Suggestions:      {}", verdict.suggestions)?;
    writeln!(writer)?;
    writeln!(writer, "{}", review)?;
    Ok(())
}

/// Writes a structured metrics JSON file for downstream analysis.
///
/// # Errors
///
/// Returns [`std::io::Error`] if the file cannot be created or written.
pub fn write_metrics(metrics: &ReviewMetrics, path: &str) -> std::io::Result<()> {
    // Create parent directory if it doesn't exist
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let json = serde_json::to_string_pretty(metrics).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

/// Prints a colored summary of the review with full metadata to a writer.
///
/// Convenience wrapper around [`print_colored_report`] that adds a metadata
/// section. Accepts any [`std::io::Write`] implementation for testability.
///
/// # Arguments
///
/// * `review` — The full LLM review text.
/// * `verdict` — Parsed verdict metadata.
/// * `state` — Determined review state.
/// * `config` — Review configuration (provider, model, etc.).
/// * `writer` — Output destination.
///
/// # Errors
///
/// Returns [`std::io::Error`] if writing to the output fails.
pub fn print_colored_summary(
    review: &str,
    verdict: &Verdict,
    state: &ReviewState,
    config: &ReviewConfig,
    writer: &mut impl Write,
) -> std::io::Result<()> {
    print_colored_report(review, verdict, state, writer)?;

    writeln!(writer)?;
    writeln!(writer, "{}", "--- Metadata ---".dimmed())?;
    writeln!(writer, "Provider:    {}", config.provider)?;
    writeln!(writer, "Model:       {}", config.model)?;
    writeln!(writer, "Temperature: {}", config.temperature)?;
    writeln!(writer, "Diff Lines:  {}", config.diff_line_count)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_metrics_creates_json_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("metrics.json");
        let path_str = path.to_str().unwrap();

        let metrics = ReviewMetrics {
            provider: "deepseek".to_string(),
            model: "deepseek-v4-flash".to_string(),
            estimated_tokens_in: 4230,
            estimated_tokens_out: 892,
            latency_secs: 8.4,
            estimated_cost_cents: 3.0,
            diff_lines: 150,
            verdict: "POSITIVE".to_string(),
            state: "APPROVE".to_string(),
        };

        write_metrics(&metrics, path_str).unwrap();

        let content = std::fs::read_to_string(path_str).unwrap();
        assert!(content.contains("deepseek"));
        assert!(content.contains("4230"));
        assert!(content.contains("APPROVE"));
        assert!(content.contains("3"));
    }

    #[test]
    fn test_write_artifact_creates_file_with_correct_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("review.txt");
        let path_str = path.to_str().unwrap();

        let verdict = Verdict {
            verdict: "POSITIVE".to_string(),
            critical_issues: 0,
            security_issues: 0,
            important_issues: 0,
            suggestions: 0,
        };
        let state = ReviewState::Approve;
        let config = ReviewConfig {
            provider: "deepseek".to_string(),
            model: "deepseek-v4-flash".to_string(),
            temperature: 0.1,
            pr_number: Some(42),
            diff_size_bytes: 1024,
            diff_line_count: 50,
        };

        write_artifact("looks good", &verdict, &state, &config, path_str).unwrap();

        let content = std::fs::read_to_string(path_str).unwrap();
        assert!(content.contains("APPROVE"));
        assert!(content.contains("POSITIVE"));
        assert!(content.contains("deepseek"));
        assert!(content.contains("deepseek-v4-flash"));
        assert!(content.contains("looks good"));
        assert!(content.contains("CriticalIssues:  0"));
        assert!(content.contains("SecurityIssues:  0"));
        assert!(content.contains("ImportantIssues: 0"));
        assert!(content.contains("Suggestions:     0"));
    }

    #[test]
    fn test_write_artifact_propagates_io_error() {
        let result = write_artifact(
            "test",
            &Verdict {
                verdict: "POSITIVE".to_string(),
                critical_issues: 0,
                security_issues: 0,
                important_issues: 0,
                suggestions: 0,
            },
            &ReviewState::Comment,
            &ReviewConfig {
                provider: "test".to_string(),
                model: "test".to_string(),
                temperature: 0.0,
                pr_number: None,
                diff_size_bytes: 0,
                diff_line_count: 0,
            },
            "/nonexistent/dir/artifact.txt",
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_print_colored_report_approve() {
        let verdict = Verdict {
            verdict: "POSITIVE".to_string(),
            critical_issues: 0,
            security_issues: 0,
            important_issues: 0,
            suggestions: 0,
        };
        let mut buf = Vec::new();
        print_colored_report("all good", &verdict, &ReviewState::Approve, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("APPROVE"));
        assert!(output.contains("all good"));
    }

    #[test]
    fn test_print_colored_report_request_changes() {
        let verdict = Verdict {
            verdict: "NEGATIVE".to_string(),
            critical_issues: 3,
            security_issues: 1,
            important_issues: 0,
            suggestions: 0,
        };
        let mut buf = Vec::new();
        print_colored_report(
            "fix these issues",
            &verdict,
            &ReviewState::RequestChanges,
            &mut buf,
        )
        .unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("REQUEST_CHANGES"));
        assert!(output.contains("fix these issues"));
        assert!(output.contains("Critical Issues:  3"));
        assert!(output.contains("Security Issues:  1"));
    }

    #[test]
    fn test_print_colored_summary_includes_metadata() {
        let verdict = Verdict {
            verdict: "POSITIVE".to_string(),
            critical_issues: 0,
            security_issues: 0,
            important_issues: 0,
            suggestions: 0,
        };
        let config = ReviewConfig {
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            temperature: 0.5,
            pr_number: None,
            diff_size_bytes: 512,
            diff_line_count: 25,
        };
        let mut buf = Vec::new();
        print_colored_summary(
            "review text",
            &verdict,
            &ReviewState::Comment,
            &config,
            &mut buf,
        )
        .unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("openai"));
        assert!(output.contains("gpt-4o"));
        assert!(output.contains("0.5"));
        assert!(output.contains("25"));
        assert!(output.contains("COMMENT"));
    }

    #[test]
    fn test_write_artifact_includes_important_and_suggestions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("review.txt");
        let path_str = path.to_str().unwrap();

        let verdict = Verdict {
            verdict: "POSITIVE".to_string(),
            critical_issues: 0,
            security_issues: 0,
            important_issues: 2,
            suggestions: 5,
        };
        let config = ReviewConfig {
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            temperature: 0.1,
            pr_number: None,
            diff_size_bytes: 512,
            diff_line_count: 20,
        };

        write_artifact("review", &verdict, &ReviewState::Comment, &config, path_str).unwrap();

        let content = std::fs::read_to_string(path_str).unwrap();
        assert!(
            content.contains("ImportantIssues: 2"),
            "ImportantIssues missing"
        );
        assert!(
            content.contains("Suggestions:     5"),
            "Suggestions missing"
        );
    }

    #[test]
    fn test_print_colored_report_shows_important_and_suggestions() {
        let verdict = Verdict {
            verdict: "POSITIVE".to_string(),
            critical_issues: 0,
            security_issues: 0,
            important_issues: 2,
            suggestions: 4,
        };
        let mut buf = Vec::new();
        print_colored_report("ok", &verdict, &ReviewState::Comment, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("Important Issues: 2"),
            "important issues missing"
        );
        assert!(
            output.contains("Suggestions:      4"),
            "suggestions missing"
        );
    }

    #[test]
    fn test_review_config_display_format() {
        let config = ReviewConfig {
            provider: "test-provider".to_string(),
            model: "test-model".to_string(),
            temperature: 0.3,
            pr_number: Some(99),
            diff_size_bytes: 2048,
            diff_line_count: 100,
        };
        assert_eq!(config.provider, "test-provider");
        assert_eq!(config.model, "test-model");
        assert_eq!(config.pr_number, Some(99));
    }
}
