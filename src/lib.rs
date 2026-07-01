#![deny(missing_docs)]
//! rs-guard — AI-powered code review CLI for GitHub Pull Requests.
//!
//! This crate provides the core functionality for fetching PR diffs,
//! sending them to an LLM for review, parsing structured verdicts,
//! and submitting review states back to GitHub.
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
//! - [`redact`] — Secret redaction and content filtering
//! - [`retry`] — Transient failure retry logic and circuit breaker
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
pub mod redact;
pub mod retry;
pub mod scaffold;
pub mod verdict;
