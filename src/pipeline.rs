//! Pipeline orchestration and result types.
//!
//! Defines the [`run_pipeline`] function that orchestrates the full review
//! workflow, and [`PipelineResult`] for communicating exit intentions.

use crate::cache::{CacheConfig, DiffCache};
use crate::config::Config;
use crate::diff::{chunk_diff, fetch_file_diff, fetch_local_diff, fetch_pr_diff};
use crate::github::{dismiss_previous_reviews, submit_review};
use crate::llm::factory::create_provider;
use crate::output::{
    print_colored_summary, write_artifact, write_metrics, ReviewConfig, ReviewMetrics,
    ARTIFACT_FILENAME, METRICS_FILENAME,
};
use crate::redact::{log_redacted, redact_secrets};
use crate::verdict::{parse_verdict, ReviewState};
use anyhow::Context;

/// Signals from the pipeline to the entry point about how to exit.
///
/// Separates orchestration logic from process exit decisions, enabling
/// integration testing of the full pipeline without process termination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineResult {
    /// Pipeline completed normally — exit code 0.
    Success,
    /// Local mode returned `REQUEST_CHANGES` — exit code 2.
    ReviewBlocked,
}

/// Runs the full review pipeline with the given configuration.
///
/// This function is separated from `main` to enable integration testing
/// without spawning a subprocess.
///
/// # Arguments
///
/// * `config` — Resolved application configuration.
/// * `diff_file` — Optional path to a pre-existing diff file. When provided,
///   the GitHub API diff fetch is skipped entirely.
pub async fn run_pipeline(
    config: Config,
    diff_file: Option<&str>,
) -> anyhow::Result<PipelineResult> {
    let base_url = config.github_base_url.clone();

    let diff_result = if let Some(path) = diff_file {
        log::info!("Reading diff from file: {}", path);
        match fetch_file_diff(path) {
            Ok(diff) => {
                log::info!(
                    "Read diff from file: {} lines ({} bytes)",
                    diff.line_count,
                    diff.size_bytes
                );
                log_redacted("Diff content", &diff.content);
                diff
            }
            Err(e) => {
                if let crate::error::DiffguardError::DiffTooLarge {
                    size_bytes,
                    line_count,
                } = &e
                {
                    eprintln!(
                        "⚠️  Diff too large: {} bytes ({} lines). Cannot review.",
                        size_bytes, line_count
                    );
                    return Ok(PipelineResult::Success);
                }
                if let crate::error::DiffguardError::EmptyDiff = &e {
                    eprintln!("ℹ️  Diff file is empty: {}", path);
                    return Ok(PipelineResult::Success);
                }
                return Err(e).context("Failed to read diff file");
            }
        }
    } else if config.is_ci {
        log::info!("CI mode detected. Fetching PR diff...");
        let owner = config.repo_owner.as_ref().unwrap();
        let repo = config.repo_name.as_ref().unwrap();
        let pr = config.pr_number.unwrap();
        let token = config.github_token.as_ref().unwrap();

        match fetch_pr_diff(&base_url, owner, repo, pr, token).await {
            Ok(diff) => {
                log::info!(
                    "Fetched diff: {} lines ({} bytes)",
                    diff.line_count,
                    diff.size_bytes
                );
                log_redacted("Diff content", &diff.content);
                diff
            }
            Err(e) => {
                if let crate::error::DiffguardError::DiffTooLarge {
                    size_bytes,
                    line_count,
                } = &e
                {
                    log::warn!(
                        "Diff too large: {} bytes ({} lines). Submitting explanatory comment.",
                        size_bytes,
                        line_count
                    );
                    let msg = format!(
                        "⚠️ **diffguard-rs**: This PR diff exceeds the review size limit ({} lines / {} bytes).\n\n\
                        The diff is too large for an effective AI review. Consider breaking this PR into smaller, focused changes.",
                        line_count, size_bytes
                    );
                    submit_review(
                        &base_url,
                        owner,
                        repo,
                        pr,
                        ReviewState::Comment,
                        &msg,
                        token,
                    )
                    .await
                    .context("Failed to submit size-limit comment")?;
                    return Ok(PipelineResult::Success);
                }
                if let crate::error::DiffguardError::EmptyDiff = &e {
                    log::warn!("PR diff is empty — nothing to review.");
                    return Ok(PipelineResult::Success);
                }
                return Err(e).context("Failed to fetch PR diff");
            }
        }
    } else {
        log::info!("Local mode detected. Fetching staged diff...");
        match fetch_local_diff() {
            Ok(diff) => {
                log::info!(
                    "Fetched local diff: {} lines ({} bytes)",
                    diff.line_count,
                    diff.size_bytes
                );
                log_redacted("Diff content", &diff.content);
                diff
            }
            Err(e) => {
                if let crate::error::DiffguardError::DiffTooLarge {
                    size_bytes,
                    line_count,
                } = &e
                {
                    eprintln!(
                        "⚠️  Diff too large: {} bytes ({} lines). Cannot review.",
                        size_bytes, line_count
                    );
                    return Ok(PipelineResult::Success);
                }
                if let crate::error::DiffguardError::EmptyDiff = &e {
                    eprintln!("ℹ️  No staged changes to review.");
                    return Ok(PipelineResult::Success);
                }
                return Err(e).context("Failed to fetch local diff");
            }
        }
    };

    // Chunk large diffs to fit within model context windows
    let (chunked_content, was_chunked, removed_lines) = chunk_diff(&diff_result.content);
    let diff_content = if was_chunked {
        log::warn!(
            "Diff chunked: omitted {} middle lines for model context window",
            removed_lines
        );
        chunked_content.into_owned()
    } else {
        diff_result.content.clone()
    };

    let cache = DiffCache::new(CacheConfig {
        enabled: !config.no_cache,
        ..CacheConfig::default()
    })
    .context("Failed to initialize response cache")?;

    cache.ensure_gitignored();

    // --- Metrics collection ---
    let start = std::time::Instant::now();
    let estimated_tokens_in = (config.prompt.len() + diff_content.len()) / 4; // rough: ~4 chars/token

    // Check cache before calling the LLM (keyed on original content, not chunked)
    let llm_response = if let Some(cached) = cache.get(
        &diff_result.content,
        &config.prompt,
        &config.provider,
        &config.model,
        config.temperature,
    ) {
        log::info!("Cache hit — using cached LLM response");
        cached
    } else {
        log::info!("Calling {} ({})...", config.provider, config.model);
        let provider = create_provider(&config.provider, &config.api_key, &config.provider_config)
            .context("Failed to create LLM provider")?;

        let response = provider
            .chat_completion(&config.prompt, &diff_content, config.temperature)
            .await
            .context("LLM API call failed")?;

        log::info!("Caching LLM response for future runs");
        if let Err(e) = cache.set(
            &diff_result.content,
            &config.prompt,
            &config.provider,
            &config.model,
            config.temperature,
            &response,
        ) {
            log::warn!("Failed to cache LLM response: {}", e);
        }

        response
    };

    let latency = start.elapsed();
    let estimated_tokens_out = llm_response.len() / 4; // rough: ~4 chars/token
    let estimated_cost_cents = estimate_cost_cents(
        &config.provider,
        estimated_tokens_in as u64,
        estimated_tokens_out as u64,
    );

    log::info!("Received LLM response ({} chars)", llm_response.len());
    log_redacted("LLM response", &llm_response);

    let (verdict, state) =
        parse_verdict(&llm_response).context("Failed to parse verdict from LLM response")?;

    log::info!(
        "Verdict: {} (CriticalBugs: {}, SecurityIssues: {}) -> State: {}",
        verdict.verdict,
        verdict.critical_bugs,
        verdict.security_issues,
        state
    );

    let review_config = ReviewConfig {
        provider: config.provider.clone(),
        model: config.model.clone(),
        temperature: config.temperature,
        pr_number: config.pr_number,
        diff_size_bytes: diff_result.size_bytes,
        diff_line_count: diff_result.line_count,
    };

    let sanitized_response = redact_secrets(&llm_response);

    write_artifact(
        &sanitized_response,
        &verdict,
        &state,
        &review_config,
        ARTIFACT_FILENAME,
    )
    .context("Failed to write review artifact")?;

    let metrics = ReviewMetrics {
        provider: config.provider.clone(),
        model: config.model.clone(),
        tokens_in: estimated_tokens_in,
        tokens_out: estimated_tokens_out,
        latency_secs: latency.as_secs_f64(),
        estimated_cost_cents,
        diff_lines: diff_result.line_count,
        verdict: verdict.verdict.clone(),
        state: state.to_string(),
    };

    if let Err(e) = write_metrics(&metrics, METRICS_FILENAME) {
        log::warn!("Failed to write metrics: {}", e);
    }

    if config.is_ci {
        let owner = config.repo_owner.as_ref().unwrap();
        let repo = config.repo_name.as_ref().unwrap();
        let pr = config.pr_number.unwrap();
        let token = config.github_token.as_ref().unwrap();

        let review_body = if was_chunked {
            format!(
                "⚠️ **Diff was truncated**: {} middle lines were omitted to fit the model context window.\n\n---\n\n{}",
                removed_lines,
                sanitized_response
            )
        } else {
            sanitized_response.clone()
        };

        submit_review(
            &base_url,
            owner,
            repo,
            pr,
            state.clone(),
            &review_body,
            token,
        )
        .await
        .context("Failed to submit review")?;

        log::info!("Review submitted: {}", state);

        if state != ReviewState::RequestChanges {
            log::info!("Dismissing previous blocker reviews...");
            if let Err(e) = dismiss_previous_reviews(&base_url, owner, repo, pr, token).await {
                log::warn!("Failed to dismiss previous reviews: {}", e);
            }
        }

        println!("diffguard-rs Review Complete");
        println!("============================");
        println!("Provider:    {}", config.provider);
        println!("Model:       {}", config.model);
        println!("Tokens In:   {}", estimated_tokens_in);
        println!("Tokens Out:  {}", estimated_tokens_out);
        println!("Latency:     {:.1}s", latency.as_secs_f64());
        println!("Est. Cost:   ${:.4}", estimated_cost_cents as f64 / 100.0);
        println!("Diff Lines:  {}", diff_result.line_count);
        println!("Verdict:     {}", verdict.verdict);
        println!("State:       {}", state);

        Ok(PipelineResult::Success)
    } else {
        // Print chunking warning in local mode too
        if was_chunked {
            eprintln!(
                "⚠️  Diff was truncated: {} middle lines were omitted to fit the model context window.",
                removed_lines
            );
        }

        print_colored_summary(
            &sanitized_response,
            &verdict,
            &state,
            &review_config,
            &mut std::io::stdout(),
        )
        .context("Failed to print review summary")?;

        if state == ReviewState::RequestChanges {
            Ok(PipelineResult::ReviewBlocked)
        } else {
            Ok(PipelineResult::Success)
        }
    }
}

/// Rough cost estimation based on provider pricing.
///
/// Returns estimated cost in cents (USD) for a given provider's token usage.
fn estimate_cost_cents(provider: &str, tokens_in: u64, tokens_out: u64) -> u64 {
    // Prices in cents per million tokens
    let (price_in_cents, price_out_cents) = match provider {
        "deepseek" => (7, 27),    // DeepSeek-V4: $0.07/M in, $0.27/M out
        "kimi" => (12, 70),       // Kimi K2.5: approximate
        "qwen" => (8, 20),        // Qwen-Plus: approximate
        "openrouter" => (15, 60), // OpenRouter avg: approximate
        "openai" => (15, 60),     // GPT-4o-mini: $0.15/M in, $0.60/M out
        _ => (10, 30),            // Default fallback
    };
    let cost_in = (tokens_in * price_in_cents) / 1_000_000;
    let cost_out = (tokens_out * price_out_cents) / 1_000_000;
    cost_in + cost_out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_cost_cents_deepseek() {
        // 1M tokens in, 1M tokens out should cost 7 + 27 = 34 cents
        let cost = estimate_cost_cents("deepseek", 1_000_000, 1_000_000);
        assert_eq!(cost, 34);
    }

    #[test]
    fn test_estimate_cost_cents_openai() {
        // 1M tokens in, 1M tokens out should cost 15 + 60 = 75 cents
        let cost = estimate_cost_cents("openai", 1_000_000, 1_000_000);
        assert_eq!(cost, 75);
    }

    #[test]
    fn test_estimate_cost_cents_unknown_provider() {
        // Unknown provider should use default pricing: 10 + 30 = 40 cents
        let cost = estimate_cost_cents("unknown", 1_000_000, 1_000_000);
        assert_eq!(cost, 40);
    }

    #[test]
    fn test_estimate_cost_cents_zero_tokens() {
        let cost = estimate_cost_cents("deepseek", 0, 0);
        assert_eq!(cost, 0);
    }

    #[test]
    fn test_estimate_cost_cents_small_tokens() {
        // 1000 tokens in, 500 tokens out
        // DeepSeek: (1000 * 7) / 1_000_000 + (500 * 27) / 1_000_000 = 0 + 0 = 0 cents
        let cost = estimate_cost_cents("deepseek", 1000, 500);
        assert_eq!(cost, 0); // Rounds down to 0
    }

    #[test]
    fn test_estimate_cost_cents_large_tokens() {
        // 10M tokens in, 5M tokens out
        // DeepSeek: (10_000_000 * 7) / 1_000_000 + (5_000_000 * 27) / 1_000_000 = 70 + 135 = 205 cents
        let cost = estimate_cost_cents("deepseek", 10_000_000, 5_000_000);
        assert_eq!(cost, 205);
    }
}
