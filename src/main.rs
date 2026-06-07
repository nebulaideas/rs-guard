//! diffguard CLI entry point.
//!
//! Orchestrates the review pipeline: fetch diff → call LLM → parse verdict
//! → submit review → write artifact.

use anyhow::Context;
use clap::Parser;
use diffguard::cli::Args;
use diffguard::config::{load_toml_config, Config};
use diffguard::diff::{fetch_file_diff, fetch_local_diff, fetch_pr_diff};
use diffguard::github::{dismiss_previous_reviews, submit_review};
use diffguard::llm::factory::create_provider;
use diffguard::output::{print_colored_summary, write_artifact, ReviewConfig, ARTIFACT_FILENAME};
use diffguard::redact::{log_redacted, redact_secrets};
use diffguard::verdict::{parse_verdict, ReviewState};

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
pub async fn run_pipeline(config: Config, diff_file: Option<&str>) -> anyhow::Result<()> {
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
                if let diffguard::error::DiffguardError::DiffTooLarge {
                    size_bytes,
                    line_count,
                } = &e
                {
                    eprintln!(
                        "⚠️  Diff too large: {} bytes ({} lines). Cannot review.",
                        size_bytes, line_count
                    );
                    return Ok(());
                }
                if let diffguard::error::DiffguardError::EmptyDiff = &e {
                    eprintln!("ℹ️  Diff file is empty: {}", path);
                    return Ok(());
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
                if let diffguard::error::DiffguardError::DiffTooLarge {
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
                    return Ok(());
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
                if let diffguard::error::DiffguardError::DiffTooLarge {
                    size_bytes,
                    line_count,
                } = &e
                {
                    eprintln!(
                        "⚠️  Diff too large: {} bytes ({} lines). Cannot review.",
                        size_bytes, line_count
                    );
                    return Ok(());
                }
                if let diffguard::error::DiffguardError::EmptyDiff = &e {
                    eprintln!("ℹ️  No staged changes to review.");
                    return Ok(());
                }
                return Err(e).context("Failed to fetch local diff");
            }
        }
    };

    log::info!("Calling {} ({})...", config.provider, config.model);
    let provider = create_provider(&config.provider, &config.api_key, &config.provider_config)
        .context("Failed to create LLM provider")?;

    let llm_response = provider
        .chat_completion(&config.prompt, &diff_result.content, config.temperature)
        .await
        .context("LLM API call failed")?;

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

    if config.is_ci {
        let owner = config.repo_owner.as_ref().unwrap();
        let repo = config.repo_name.as_ref().unwrap();
        let pr = config.pr_number.unwrap();
        let token = config.github_token.as_ref().unwrap();

        submit_review(
            &base_url,
            owner,
            repo,
            pr,
            state.clone(),
            &sanitized_response,
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
        println!("Diff Lines:  {}", diff_result.line_count);
        println!("Verdict:     {}", verdict.verdict);
        println!("State:       {}", state);
    } else {
        print_colored_summary(&sanitized_response, &verdict, &state, &review_config);

        if state == ReviewState::RequestChanges {
            std::process::exit(2);
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Args::parse();

    let toml_config =
        load_toml_config(&args.config).context("Failed to load TOML configuration")?;

    let mut config = Config::from_env(toml_config).context("Failed to load configuration")?;
    config
        .apply_args(&args)
        .context("Failed to apply CLI arguments")?;
    config
        .load_prompt_file(&args.prompt_file)
        .context("Failed to load prompt file")?;
    let diff_file = args.diff_file.as_deref();

    config
        .validate_for_ci()
        .context("Configuration validation failed")?;

    log::info!(
        "diffguard-rs starting (provider: {}, model: {})",
        config.provider,
        config.model
    );

    run_pipeline(config, diff_file).await
}
