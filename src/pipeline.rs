//! Pipeline orchestration and result types.
//!
//! Defines the [`run_pipeline`] function that orchestrates the full review
//! workflow, and [`PipelineResult`] for communicating exit intentions.

use crate::cache::{CacheConfig, DiffCache};
use crate::config::{CiConfig, Config};
use crate::diff::{chunk_diff_with_params, fetch_file_diff, fetch_local_diff, fetch_pr_diff};
use crate::error::RsGuardError;
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

/// Identifies the source of a diff fetch so that errors can be handled
/// uniformly while keeping source-specific user-facing messages.
enum DiffSource<'a> {
    /// Diff was read from `--diff-file`.
    File(&'a str),
    /// Diff was fetched from GitHub in CI mode.
    Ci { config: CiConfig },
    /// Diff was read from `git diff --cached` in local mode.
    Local,
}

/// Handles errors that can occur while fetching a diff.
///
/// `DiffTooLarge` and `EmptyDiff` are treated as non-fatal: the pipeline
/// stops successfully with an explanatory message. Any other error is
/// wrapped with source-specific context and returned.
async fn handle_diff_fetch_error(
    err: RsGuardError,
    source: DiffSource<'_>,
) -> anyhow::Result<PipelineResult> {
    match err {
        RsGuardError::DiffTooLarge {
            size_bytes,
            line_count,
        } => {
            if let DiffSource::Ci { config } = &source {
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
                    &config.github_base_url,
                    &config.repo_owner,
                    &config.repo_name,
                    config.pr_number,
                    ReviewState::Comment,
                    &msg,
                    &config.github_token,
                )
                .await
                .context("Failed to submit size-limit comment")?;
            } else {
                eprintln!(
                    "⚠️  Diff too large: {} bytes ({} lines). Cannot review.",
                    size_bytes, line_count
                );
            }
            Ok(PipelineResult::Success)
        }
        RsGuardError::EmptyDiff => {
            match source {
                DiffSource::File(path) => {
                    eprintln!("ℹ️  Diff file is empty: {}", path);
                }
                DiffSource::Ci { .. } => {
                    log::warn!("PR diff is empty — nothing to review.");
                }
                DiffSource::Local => {
                    eprintln!("ℹ️  No staged changes to review.");
                }
            }
            Ok(PipelineResult::Success)
        }
        other => {
            let context = match source {
                DiffSource::File(_) => "Failed to read diff file",
                DiffSource::Ci { .. } => "Failed to fetch PR diff",
                DiffSource::Local => "Failed to fetch local diff",
            };
            Err(other).context(context)
        }
    }
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
            Err(e) => return handle_diff_fetch_error(e, DiffSource::File(path)).await,
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
                return handle_diff_fetch_error(e, DiffSource::Ci { config: ci_config }).await
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
            Err(e) => return handle_diff_fetch_error(e, DiffSource::Local).await,
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
        auto_gitignore: config.auto_gitignore,
        ..CacheConfig::default()
    };
    let cache = DiffCache::new(cache_config).context("Failed to initialize response cache")?;

    // Only auto-add to .gitignore in local mode — CI environments are often read-only
    if !config.is_ci {
        if let Err(e) = cache.ensure_gitignored() {
            log::warn!("Failed to update .gitignore: {}", e);
        }
    }

    // --- Metrics collection ---
    let start = std::time::Instant::now();
    let estimated_tokens_in = estimate_tokens(&config.prompt) + estimate_tokens(&diff_content);

    // Warn if estimated tokens approach provider context window limits
    if let Some(context_window) =
        crate::llm::providers::get_provider_context_window(&config.provider)
    {
        check_token_warning(estimated_tokens_in, context_window, &config.provider);
    }

    // Check cache before calling the LLM (keyed on actual content sent to LLM).
    // base_url + max_tokens are part of the key to prevent cross-endpoint
    // poisoning and truncation staleness.
    let effective_base_url = config.provider_config.base_url.as_deref().unwrap_or("");
    let (llm_response, should_cache) = if let Some(cached) = cache.get(
        &diff_content,
        &config.prompt,
        &config.provider,
        &config.model,
        config.variant.as_deref(),
        config.temperature,
        effective_base_url,
        config.provider_config.max_tokens,
    ) {
        log::info!("Cache hit — using cached LLM response");
        (cached, false)
    } else {
        if !config.is_ci {
            println!("🤖 Calling {} ({})...", config.provider, config.model);
        }
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

        if !config.is_ci {
            println!("✅ Response received ({} chars)", response.len());
        }

        (response, !config.no_cache)
    };

    let latency = start.elapsed();
    let estimated_tokens_out = estimate_tokens(&llm_response);
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

    if should_cache {
        log::info!("Caching LLM response for future runs");
        if let Err(e) = cache.set(
            &diff_content,
            &config.prompt,
            &config.provider,
            &config.model,
            config.variant.as_deref(),
            config.temperature,
            effective_base_url,
            config.provider_config.max_tokens,
            &llm_response,
        ) {
            log::warn!("Failed to cache LLM response: {}", e);
        }
    }

    log::info!(
        "Verdict: {} (CriticalIssues: {}, SecurityIssues: {}, ImportantIssues: {}, Suggestions: {}) -> State: {}",
        verdict.verdict,
        verdict.critical_issues,
        verdict.security_issues,
        verdict.important_issues,
        verdict.suggestions,
        state
    );

    let review_config = ReviewConfig {
        provider: config.provider.clone(),
        model: config.model.clone(),
        variant: config.variant.clone(),
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
        variant: config.variant.clone(),
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
        if let Some(ref v) = config.variant {
            println!("Variant:       {}", v);
        }
        println!("Est. Tokens In:  {}", estimated_tokens_in);
        println!("Est. Tokens Out: {}", estimated_tokens_out);
        println!("Latency:     {:.1}s", latency.as_secs_f64());
        println!(
            "Est. Cost:   {}",
            match estimated_cost_cents {
                Some(c) => format!("${:.4}", c / 100.0),
                None => "unknown".to_string(),
            }
        );
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

/// Estimates token count for a piece of text using a hybrid heuristic.
///
/// ASCII characters are estimated at ~4 chars per token.
/// Non-ASCII characters (Unicode, emoji, CJK, etc.) are estimated at
/// ~1.5 chars per token, since they typically consume more bytes per token.
fn estimate_tokens(text: &str) -> usize {
    let (ascii_chars, non_ascii_chars) =
        text.chars()
            .fold((0usize, 0usize), |(ascii, non_ascii), c| {
                if c.is_ascii() {
                    (ascii + 1, non_ascii)
                } else {
                    (ascii, non_ascii + 1)
                }
            });

    let ascii_tokens = ascii_chars / 4;
    let non_ascii_tokens = (non_ascii_chars as f64 / 1.5) as usize;

    ascii_tokens + non_ascii_tokens
}

/// Warns when estimated tokens approach the provider's context window limit.
///
/// Prints a warning to stderr when `estimated_tokens` exceeds 80% of the
/// provider's known context window.
fn check_token_warning(estimated_tokens: usize, context_window: usize, provider: &str) {
    let threshold = (context_window as f64 * 0.8) as usize;
    if estimated_tokens > threshold {
        eprintln!(
            "⚠️  Warning: Estimated input tokens ({}) approach 80% of {} context window ({} tokens).\n\
             Consider using a smaller diff or a provider with a larger context window.",
            estimated_tokens, provider, context_window
        );
    }
}

/// Rough cost estimation based on provider pricing.
///
/// Returns estimated cost in **cents** (USD) as `f64` to avoid integer
/// truncation for small diffs. For display, divide by 100.0.
///
/// Returns `None` when pricing is unknown for the provider (F9: don't lie).
/// Pricing can be overridden via `.reviewer.toml` [pricing] sections.
fn estimate_cost_cents(
    provider: &str,
    tokens_in: u64,
    tokens_out: u64,
    pricing_overrides: Option<&std::collections::HashMap<String, crate::config::PricingTomlConfig>>,
) -> Option<f64> {
    // Prices in cents per million tokens
    let pricing = if let Some(pricing) = pricing_overrides {
        if let Some(p) = pricing.get(provider) {
            Some((p.input_per_million as f64, p.output_per_million as f64))
        } else {
            default_pricing(provider)
        }
    } else {
        default_pricing(provider)
    };

    match pricing {
        Some((price_in_cents, price_out_cents)) => {
            let cost_in = (tokens_in as f64 * price_in_cents) / 1_000_000.0;
            let cost_out = (tokens_out as f64 * price_out_cents) / 1_000_000.0;
            Some(cost_in + cost_out)
        }
        None => {
            log::warn!(
                "Pricing unknown for provider '{}'; cost estimate will be omitted from metrics",
                provider
            );
            None
        }
    }
}

/// Returns hardcoded default pricing for known providers.
///
/// Returns `None` for providers whose pricing cannot be verified. This is
/// intentional (F9: "don't lie") — an unknown price is reported as unknown,
/// never fabricated. Pricing is in cents per million tokens.
fn default_pricing(provider: &str) -> Option<(f64, f64)> {
    match provider {
        "deepseek" => Some((7.0, 27.0)), // DeepSeek-V4: $0.07/M in, $0.27/M out
        "kimi" => Some((12.0, 70.0)),    // Kimi K2.5: approximate
        "qwen" => Some((8.0, 20.0)),     // Qwen-Plus: approximate
        "openrouter" => Some((15.0, 60.0)), // OpenRouter avg: approximate
        "openai" => Some((15.0, 60.0)),  // GPT-4o-mini: $0.15/M in, $0.60/M out
        // Verified from https://docs.x.ai/docs/models (grok-4.3: $1.25/M in, $2.50/M out).
        // Our default is grok-3 which may differ; this is the closest verified rate.
        "grok" => Some((125.0, 250.0)),
        // GLM-4 pricing could not be verified (Zhipu pricing page requires JavaScript).
        // Do not fabricate — report "unknown" instead (F9).
        "glm" => None,
        _ => None, // Unknown/custom provider — pricing unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_cost_cents_deepseek() {
        // 1M tokens in, 1M tokens out should cost 7 + 27 = 34 cents
        let cost = estimate_cost_cents("deepseek", 1_000_000, 1_000_000, None).unwrap();
        assert!((cost - 34.0).abs() < 0.001);
    }

    #[test]
    fn test_estimate_cost_cents_openai() {
        // 1M tokens in, 1M tokens out should cost 15 + 60 = 75 cents
        let cost = estimate_cost_cents("openai", 1_000_000, 1_000_000, None).unwrap();
        assert!((cost - 75.0).abs() < 0.001);
    }

    #[test]
    fn test_estimate_cost_cents_grok() {
        // Grok (verified from docs.x.ai): 125 + 250 = 375 cents per 1M in / 1M out
        let cost = estimate_cost_cents("grok", 1_000_000, 1_000_000, None).unwrap();
        assert!((cost - 375.0).abs() < 0.001);
    }

    #[test]
    fn test_estimate_cost_cents_glm_unknown() {
        // GLM pricing unverifiable — must return None (F9: don't lie)
        let cost = estimate_cost_cents("glm", 1_000_000, 1_000_000, None);
        assert_eq!(cost, None, "GLM pricing must be unknown, not fabricated");
    }

    #[test]
    fn test_estimate_cost_cents_unknown_provider() {
        // Unknown provider must return None (F9: don't fabricate a fallback)
        let cost = estimate_cost_cents("unknown", 1_000_000, 1_000_000, None);
        assert_eq!(
            cost, None,
            "unknown provider pricing must be None, not fabricated"
        );
    }

    #[test]
    fn test_estimate_cost_cents_zero_tokens() {
        let cost = estimate_cost_cents("deepseek", 0, 0, None).unwrap();
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn test_estimate_cost_cents_small_tokens_no_truncation() {
        // 1000 tokens in, 500 tokens out
        // DeepSeek: (1000 * 7.0) / 1_000_000 + (500 * 27.0) / 1_000_000 = 0.007 + 0.0135 = 0.0205 cents
        let cost = estimate_cost_cents("deepseek", 1000, 500, None).unwrap();
        assert!((cost - 0.0205).abs() < 0.0001);
    }

    #[test]
    fn test_estimate_cost_cents_large_tokens() {
        // 10M tokens in, 5M tokens out
        // DeepSeek: (10_000_000 * 7.0) / 1_000_000 + (5_000_000 * 27.0) / 1_000_000 = 70 + 135 = 205 cents
        let cost = estimate_cost_cents("deepseek", 10_000_000, 5_000_000, None).unwrap();
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
        let cost = estimate_cost_cents("deepseek", 1_000_000, 1_000_000, Some(&pricing)).unwrap();
        assert!((cost - 60.0).abs() < 0.001);
    }

    #[test]
    fn test_estimate_cost_cents_pricing_override_for_glm() {
        // Even if default pricing is None (GLM), a TOML override must work.
        let mut pricing = std::collections::HashMap::new();
        pricing.insert(
            "glm".to_string(),
            crate::config::PricingTomlConfig {
                input_per_million: 10,
                output_per_million: 30,
            },
        );
        let cost = estimate_cost_cents("glm", 1_000_000, 1_000_000, Some(&pricing)).unwrap();
        assert!((cost - 40.0).abs() < 0.001);
    }

    #[test]
    fn test_estimate_tokens_ascii() {
        // 40 ASCII chars ≈ 10 tokens
        let text = "a".repeat(40);
        assert_eq!(estimate_tokens(&text), 10);
    }

    #[test]
    fn test_estimate_tokens_non_ascii() {
        // 15 non-ASCII chars ≈ 10 tokens (15 / 1.5 = 10)
        let text = "中".repeat(15);
        assert_eq!(estimate_tokens(&text), 10);
    }

    #[test]
    fn test_estimate_tokens_mixed() {
        // 20 ASCII chars (5 tokens) + 15 non-ASCII chars (10 tokens) = 15 tokens
        let ascii = "a".repeat(20);
        let non_ascii = "中".repeat(15);
        let text = format!("{}{}", ascii, non_ascii);
        assert_eq!(estimate_tokens(&text), 15);
    }

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_check_token_warning_below_threshold() {
        // Should not panic or print when below 80%
        check_token_warning(1000, 128_000, "deepseek");
    }

    #[test]
    fn test_check_token_warning_above_threshold() {
        check_token_warning(110_000, 128_000, "deepseek");
    }

    #[test]
    fn test_estimate_cost_cents_pricing_override_miss_falls_to_default() {
        let mut pricing = std::collections::HashMap::new();
        pricing.insert(
            "openai".to_string(),
            crate::config::PricingTomlConfig {
                input_per_million: 100,
                output_per_million: 200,
            },
        );
        let cost = estimate_cost_cents("deepseek", 1_000_000, 1_000_000, Some(&pricing)).unwrap();
        assert!((cost - 34.0).abs() < 0.001);
    }

    #[test]
    fn test_check_token_warning_at_exact_threshold() {
        check_token_warning(800, 1000, "test");
        check_token_warning(801, 1000, "test");
    }

    #[test]
    fn test_estimate_tokens_emoji() {
        let text = "\u{1f389}".repeat(15);
        assert_eq!(estimate_tokens(&text), 10);
    }

    // --- Issue #23: Test for estimate_cost_cents overflow ---

    #[test]
    fn test_estimate_cost_cents_overflow_saturates() {
        // Test with very large token counts that could cause overflow
        // The function uses f64, so it should handle large values gracefully
        let cost = estimate_cost_cents("deepseek", u64::MAX, u64::MAX, None);
        // Should return Some with a finite value (not infinity or NaN)
        assert!(cost.is_some());
        let cost_val = cost.unwrap();
        assert!(cost_val.is_finite());
        assert!(!cost_val.is_nan());
    }

    #[test]
    fn test_estimate_cost_cents_large_but_safe() {
        // Test with realistically large token counts (e.g., 1B tokens)
        let cost = estimate_cost_cents("deepseek", 1_000_000_000, 500_000_000, None);
        // Should be Some with a finite and reasonable value
        assert!(cost.is_some());
        let cost_val = cost.unwrap();
        assert!(cost_val.is_finite());
        assert!(cost_val > 0.0);
    }

    // --- Diff fetch error handling (shared `handle_diff_fetch_error` helper) ---

    #[tokio::test]
    async fn test_handle_diff_fetch_error_ci_diff_too_large_submits_comment() {
        use wiremock::matchers::{method, path_regex};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let github = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path_regex(r"/repos/.+/pulls/\d+/reviews"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&github)
            .await;

        let config = CiConfig {
            github_token: "test-token".into(),
            pr_number: 42,
            repo_owner: "test-owner".into(),
            repo_name: "test-repo".into(),
            github_base_url: github.uri(),
        };

        let result = handle_diff_fetch_error(
            RsGuardError::DiffTooLarge {
                size_bytes: 200_000,
                line_count: 2_000,
            },
            DiffSource::Ci { config },
        )
        .await;

        assert_eq!(result.unwrap(), PipelineResult::Success);
    }

    #[tokio::test]
    async fn test_handle_diff_fetch_error_file_empty_diff_returns_success() {
        let result =
            handle_diff_fetch_error(RsGuardError::EmptyDiff, DiffSource::File("/tmp/empty.diff"))
                .await;
        assert_eq!(result.unwrap(), PipelineResult::Success);
    }

    #[tokio::test]
    async fn test_handle_diff_fetch_error_local_empty_diff_returns_success() {
        let result = handle_diff_fetch_error(RsGuardError::EmptyDiff, DiffSource::Local).await;
        assert_eq!(result.unwrap(), PipelineResult::Success);
    }

    #[tokio::test]
    async fn test_handle_diff_fetch_error_file_io_error_propagates() {
        let result = handle_diff_fetch_error(
            RsGuardError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "missing file",
            )),
            DiffSource::File("/tmp/missing.diff"),
        )
        .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to read diff file"));
    }
}
