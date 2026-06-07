//! diffguard — AI-powered code review CLI for GitHub Pull Requests.
//!
//! This crate provides the core functionality for fetching PR diffs,
//! sending them to an LLM for review, parsing structured verdicts,
//! and submitting review states back to GitHub.
//!
//! # Modules
//!
//! - [`cli`] — Command-line argument parsing
//! - [`config`] — Environment and configuration resolution
//! - [`diff`] — PR diff fetching (GitHub API and local git)
//! - [`error`] — Unified error types
//! - [`github`] — GitHub review submission and dismissal
//! - [`http`] — Shared HTTP utilities and URL validation
//! - [`llm`] — LLM provider abstraction and implementations
//! - [`output`] — Terminal output and artifact writing
//! - [`redact`] — Secret redaction and content filtering
//! - [`retry`] — Transient failure retry logic
//! - [`verdict`] — Verdict parsing and review state determination

pub mod cli;
pub mod config;
pub mod diff;
pub mod error;
pub mod github;
pub mod http;
pub mod llm;
pub mod output;
pub mod redact;
pub mod retry;
pub mod verdict;
