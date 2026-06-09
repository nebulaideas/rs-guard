//! Pipeline orchestration and result types.
//!
//! Defines the [`run_pipeline`] function that orchestrates the full review
//! workflow, and [`PipelineResult`] for communicating exit intentions.

use crate::cache::{CacheConfig, DiffCache};
use crate::config::Config;
use crate::diff::{chunk_diff_with_params, fetch_file_diff, fetch_local_diff, fetch_pr_diff};
use crate::github::{dismiss_previous_reviews, submit_review};
use crate::llm::factory::create_provider;
use crate::output::{
    print_colored_summary, write_artifact, write_metrics, ReviewConfig, ReviewMetrics,
    ARTIFACT_FILENAME, METRICS_FILENAME,
};
use crate::redact::{log_redacted, redact_secrets};
use crate::retry::with_retry;
use crate::verdict::{parse_metadata_block, parse_verdict, ReviewState};
use anyhow::Context;
use std::path::PathBuf;

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
                if let crate::error::RsGuardError::DiffTooLarge {
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
                if let crate::error::RsGuardError::EmptyDiff = &e {
                    eprintln!("ℹ️  Diff file is empty: {}", path);
                    return Ok(PipelineResult::Success);
                }
                return Err(e).context("Failed to read diff file");
            }
        }
    } else if config.is_ci {
        log::info!("CI mode detected. Fetching PR diff...");
        let ci_config = config
            .validate_for_ci()
            .context("CI configuration validation failed")?;

        match fetch_pr_diff(
            &ci_config.github_base_url,
            &ci_config.repo_owner,
            &ci_config.repo_name,
            ci_config.pr_number,
            &ci_config.github_token,
        )
        .await
        {
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
                if let crate::error::RsGuardError::DiffTooLarge {
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
                        "⚠️ **rs-guard**: This PR diff exceeds the review size limit ({} lines / {} bytes).\n\n\
                        The diff is too large for an effective AI review. Consider breaking this PR into smaller, focused changes.",
                        line_count, size_bytes
                    );
                    submit_review(
                        &ci_config.github_base_url,
                        &ci_config.repo_owner,
                        &ci_config.repo_name,
                        ci_config.pr_number,
                        ReviewState::Comment,
                        &msg,
                        &ci_config.github_token,
                    )
                    .await
                    .context("Failed to submit size-limit comment")?;
                    return Ok(PipelineResult::Success);
                }
                if let crate::error::RsGuardError::EmptyDiff = &e {
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
                if let crate::error::RsGuardError::DiffTooLarge {
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
                if let crate::error::RsGuardError::EmptyDiff = &e {
                    eprintln!("ℹ️  No staged changes to review.");
                    return Ok(PipelineResult::Success);
                }
                return Err(e).context("Failed to fetch local diff");
            }
        }
    };

    // Chunk large diffs to fit within model context windows
    let (chunked_content, was_chunked, removed_lines) = chunk_diff_with_params(
        &diff_result.content,
        config.chunk_head_lines,
        config.chunk_tail_lines,
    );
    let diff_content = if was_chunked {
        log::warn!(
            "Diff chunked: omitted {} middle lines for model context window",
            removed_lines
        );
        chunked_content.into_owned()
    } else {
        diff_result.content.clone()
    };

    let cache_config = CacheConfig {
        enabled: !config.no_cache,
        cache_dir: config
            .cache_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| CacheConfig::default().cache_dir),
        ..CacheConfig::default()
    };
    let cache = DiffCache::new(cache_config).context("Failed to initialize response cache")?;

    // Only auto-add to .gitignore in local mode — CI environments are often read-only
    if !config.is_ci {
        cache.ensure_gitignored();
    }

    // --- Metrics collection ---
    let start = std::time::Instant::now();
    let estimated_tokens_in = (config.prompt.len() + diff_content.len()) / 4; // rough: ~4 chars/token

    // Check cache before calling the LLM (keyed on actual content sent to LLM)
    let llm_response = if let Some(cached) = cache.get(
        &diff_content,
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

        let response = with_retry(
            || async {
                provider
                    .chat_completion(&config.prompt, &diff_content, config.temperature)
                    .await
            },
            config.circuit_breaker.as_ref(),
        )
        .await
        .context("LLM API call failed")?;

        log::info!("Caching LLM response for future runs");
        if let Err(e) = cache.set(
            &diff_content,
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
        config.pricing.as_ref(),
    );

    log::info!("Received LLM response ({} chars)", llm_response.len());
    log_redacted("LLM response", &llm_response);

    if llm_response.trim().len() < 10 {
        log::warn!(
            "LLM response is suspiciously short ({} chars), may indicate an API error",
            llm_response.len()
        );
    }

    // Warn when the structured metadata block is absent — the fallback tag-counting
    // path activates, which may produce incorrect APPROVE verdicts on truncated
    // responses.  A missing metadata block usually means the LLM output was cut
    // short before it could write the verdict footer.
    if parse_metadata_block(&llm_response).is_none() {
        log::warn!(
            "LLM response missing [RS_GUARD_VERDICT_METADATA] block — \
             falling back to tag-counting. Response may have been truncated. \
             Consider setting a higher max_tokens value."
        );
    }

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
        estimated_tokens_in,
        estimated_tokens_out,
        latency_secs: latency.as_secs_f64(),
        estimated_cost_cents,
        diff_lines: diff_result.line_count,
        verdict: verdict.verdict.clone(),
        state: state.to_string(),
    };

    let metrics_path =
        std::env::var("RS_GUARD_METRICS_PATH").unwrap_or_else(|_| METRICS_FILENAME.to_string());
    if let Err(e) = write_metrics(&metrics, &metrics_path) {
        log::warn!("Failed to write metrics: {}", e);
    }

    if config.is_ci {
        let ci_config = config
            .validate_for_ci()
            .context("CI configuration validation failed")?;

        let review_body = if was_chunked {
            format!(
                "⚠️ **Diff was truncated**: {} middle lines were omitted to fit the model context window.\n\n---\n\n{}",
                removed_lines,
                sanitized_response
            )
        } else {
            sanitized_response.clone()
        };

        if config.dry_run {
            println!("🔍 DRY RUN — would submit review: {}", state);
            log::info!("Dry-run mode: skipping GitHub review submission");
        } else {
            submit_review(
                &ci_config.github_base_url,
                &ci_config.repo_owner,
                &ci_config.repo_name,
                ci_config.pr_number,
                state.clone(),
                &review_body,
                &ci_config.github_token,
            )
            .await
            .context("Failed to submit review")?;

            log::info!("Review submitted: {}", state);

            if state != ReviewState::RequestChanges {
                log::info!("Dismissing previous blocker reviews...");
                if let Err(e) = dismiss_previous_reviews(
                    &ci_config.github_base_url,
                    &ci_config.repo_owner,
                    &ci_config.repo_name,
                    ci_config.pr_number,
                    &ci_config.github_token,
                )
                .await
                {
                    log::warn!("Failed to dismiss previous reviews: {}", e);
                }
            }
        }

        println!("rs-guard Review Complete");
        if config.dry_run {
            println!("============================");
            println!("🔍 DRY RUN — no changes submitted");
        }
        println!("============================");
        println!("Provider:    {}", config.provider);
        println!("Model:       {}", config.model);
        println!("Est. Tokens In:  {}", estimated_tokens_in);
        println!("Est. Tokens Out: {}", estimated_tokens_out);
        println!("Latency:     {:.1}s", latency.as_secs_f64());
        println!("Est. Cost:   ${:.4}", estimated_cost_cents / 100.0);
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

        if config.dry_run {
            println!(
                "\n🔍 DRY RUN — would exit with: {}",
                if state == ReviewState::RequestChanges {
                    "2 (ReviewBlocked)"
                } else {
                    "0 (Success)"
                }
            );
            Ok(PipelineResult::Success)
        } else if state == ReviewState::RequestChanges {
            Ok(PipelineResult::ReviewBlocked)
        } else {
            Ok(PipelineResult::Success)
        }
    }
}

/// Rough cost estimation based on provider pricing.
///
/// Returns estimated cost in **cents** (USD) as `f64` to avoid integer
/// truncation for small diffs. For display, divide by 100.0.
///
/// Pricing can be overridden via `.reviewer.toml` [pricing] sections.
fn estimate_cost_cents(
    provider: &str,
    tokens_in: u64,
    tokens_out: u64,
    pricing_overrides: Option<&std::collections::HashMap<String, crate::config::PricingTomlConfig>>,
) -> f64 {
    // Prices in cents per million tokens
    let (price_in_cents, price_out_cents) = if let Some(pricing) = pricing_overrides {
        if let Some(p) = pricing.get(provider) {
            (p.input_per_million as f64, p.output_per_million as f64)
        } else {
            default_pricing(provider)
        }
    } else {
        default_pricing(provider)
    };

    let cost_in = (tokens_in as f64 * price_in_cents) / 1_000_000.0;
    let cost_out = (tokens_out as f64 * price_out_cents) / 1_000_000.0;
    cost_in + cost_out
}

/// Returns hardcoded default pricing for known providers.
fn default_pricing(provider: &str) -> (f64, f64) {
    match provider {
        "deepseek" => (7.0, 27.0),    // DeepSeek-V4: $0.07/M in, $0.27/M out
        "kimi" => (12.0, 70.0),       // Kimi K2.5: approximate
        "qwen" => (8.0, 20.0),        // Qwen-Plus: approximate
        "openrouter" => (15.0, 60.0), // OpenRouter avg: approximate
        "openai" => (15.0, 60.0),     // GPT-4o-mini: $0.15/M in, $0.60/M out
        _ => (10.0, 30.0),            // Default fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_cost_cents_deepseek() {
        // 1M tokens in, 1M tokens out should cost 7 + 27 = 34 cents
        let cost = estimate_cost_cents("deepseek", 1_000_000, 1_000_000, None);
        assert!((cost - 34.0).abs() < 0.001);
    }

    #[test]
    fn test_estimate_cost_cents_openai() {
        // 1M tokens in, 1M tokens out should cost 15 + 60 = 75 cents
        let cost = estimate_cost_cents("openai", 1_000_000, 1_000_000, None);
        assert!((cost - 75.0).abs() < 0.001);
    }

    #[test]
    fn test_estimate_cost_cents_unknown_provider() {
        // Unknown provider should use default pricing: 10 + 30 = 40 cents
        let cost = estimate_cost_cents("unknown", 1_000_000, 1_000_000, None);
        assert!((cost - 40.0).abs() < 0.001);
    }

    #[test]
    fn test_estimate_cost_cents_zero_tokens() {
        let cost = estimate_cost_cents("deepseek", 0, 0, None);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn test_estimate_cost_cents_small_tokens_no_truncation() {
        // 1000 tokens in, 500 tokens out
        // DeepSeek: (1000 * 7.0) / 1_000_000 + (500 * 27.0) / 1_000_000 = 0.007 + 0.0135 = 0.0205 cents
        let cost = estimate_cost_cents("deepseek", 1000, 500, None);
        assert!((cost - 0.0205).abs() < 0.0001);
    }

    #[test]
    fn test_estimate_cost_cents_large_tokens() {
        // 10M tokens in, 5M tokens out
        // DeepSeek: (10_000_000 * 7.0) / 1_000_000 + (5_000_000 * 27.0) / 1_000_000 = 70 + 135 = 205 cents
        let cost = estimate_cost_cents("deepseek", 10_000_000, 5_000_000, None);
        assert!((cost - 205.0).abs() < 0.001);
    }

    #[test]
    fn test_estimate_cost_cents_with_pricing_override() {
        let mut pricing = std::collections::HashMap::new();
        pricing.insert(
            "deepseek".to_string(),
            crate::config::PricingTomlConfig {
                input_per_million: 10,
                output_per_million: 50,
            },
        );
        let cost = estimate_cost_cents("deepseek", 1_000_000, 1_000_000, Some(&pricing));
        assert!((cost - 60.0).abs() < 0.001);
    }
}
