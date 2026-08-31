#![deny(missing_docs)]
//! rs-guard — AI-powered code review CLI for GitHub Pull Requests.
//!
//! This crate provides the core functionality for fetching PR diffs,
//! sending them to an LLM for review, parsing structured verdicts,
//! and submitting review states back to GitHub.
//!
//! System design: `docs/ARCHITECTURE.md`. Pipeline entry: [`pipeline::run_pipeline`].
//!
//! # Modules
//!
//! - [`cache`] — LLM response caching with SHA-256 keyed entries
//! - [`cli`] — Command-line argument parsing
//! - [`config`] — Environment and configuration resolution
//! - [`diff`] — PR diff fetching (GitHub API and local git)
//! - [`error`] — Unified error types
//! - [`github`] — GitHub review submission and dismissal
//! - [`http`] — Shared HTTP utilities and URL validation
//! - [`llm`] — LLM provider abstraction and implementations
//! - [`output`] — Terminal output and artifact writing
//! - [`pipeline`] — Orchestration of the full review workflow
//! - [`prompt_select`] — Language-aware prompt auto-selection
//! - [`redact`] — Secret redaction and content filtering
//! - [`retry`] — Transient failure retry logic and circuit breaker
//! - [`rules`] — Project rules file detection and loading
//! - [`verdict`] — Verdict parsing and review state determination

pub mod cache;
pub mod cli;
pub mod config;
pub mod diff;
pub mod error;
pub mod github;
pub mod http;
pub mod llm;
pub mod output;
pub mod pipeline;
pub mod prompt_select;
pub mod redact;
pub mod repo;
pub mod retry;
pub mod rules;
pub mod scaffold;
pub mod verdict;
