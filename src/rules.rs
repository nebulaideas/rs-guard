//! Project rules detection and loading.
//!
//! Auto-detects AI-agent instruction files (`AGENTS.md`, `CLAUDE.md`,
//! `.github/copilot-instructions.md`, `.gemini/styleguide.md`,
//! `.cursor/rules/*.md`, `.windsurfrules`) and loads their content for
//! injection into the LLM review prompt as a "Project Conventions" section.
//!
//! # Detection Priority
//!
//! Files are scanned in a fixed priority order. The **first match** wins;
//! remaining files are ignored. This avoids token bloat and conflicting rules.
//!
//! 1. `AGENTS.md`
//! 2. `CLAUDE.md`
//! 3. `.github/copilot-instructions.md`
//! 4. `.gemini/styleguide.md`
//! 5. `.cursor/rules/*.md` (glob — first file alphabetically)
//! 6. `.windsurfrules`
//!
//! # Soft Cap
//!
//! Rules file content is capped at [`DEFAULT_RULES_CAP_BYTES`] (32 KB). If the
//! file exceeds the cap, content is truncated and a warning banner is appended
//! informing the LLM that the rules were truncated. The review still runs.
//!
//! # Builder API
//!
//! Use [`RulesDetector::builder()`] for a fluent, validated construction:
//!
//! ```
//! use rs_guard::rules::RulesDetector;
//! use std::path::PathBuf;
//!
//! let detector = RulesDetector::builder()
//!     .repo_root(PathBuf::from("."))
//!     .cap_bytes(16_384)
//!     .build()
//!     .expect("builder should succeed");
//! ```

use crate::error::RsGuardError;
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Default soft cap for rules file content: 32 KB (~8k tokens).
pub const DEFAULT_RULES_CAP_BYTES: usize = 32 * 1024;

/// Priority order for rules file detection (first match wins).
///
/// The `.cursor/rules/*.md` entry is handled specially via glob expansion
/// and is not listed here as a simple string — see [`RulesDetector::detect`].
const RULES_FILE_PRIORITY: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    ".github/copilot-instructions.md",
    ".gemini/styleguide.md",
];

/// Directory containing Cursor editor rules (globbed for `*.md` files).
const CURSOR_RULES_DIR: &str = ".cursor/rules";

/// Windsurf rules file (no extension).
const WINDSURF_RULES_FILE: &str = ".windsurfrules";

// ---------------------------------------------------------------------------
// Newtypes — type safety over primitive types
// ---------------------------------------------------------------------------

/// Path to a detected project rules file, relative to the repo root.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RulesFilePath(PathBuf);

impl RulesFilePath {
    /// Creates a new [`RulesFilePath`] from the given path.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    /// Returns the path as a [`Path`].
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Consumes the newtype and returns the inner [`PathBuf`].
    #[must_use]
    pub fn into_inner(self) -> PathBuf {
        self.0
    }
}

impl From<&str> for RulesFilePath {
    fn from(s: &str) -> Self {
        Self(PathBuf::from(s))
    }
}

impl From<String> for RulesFilePath {
    fn from(s: String) -> Self {
        Self(PathBuf::from(s))
    }
}

impl From<PathBuf> for RulesFilePath {
    fn from(p: PathBuf) -> Self {
        Self(p)
    }
}

impl fmt::Display for RulesFilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

/// Size of a rules file in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RulesFileSize(usize);

impl RulesFileSize {
    /// Creates a new [`RulesFileSize`].
    #[must_use]
    pub fn new(size: usize) -> Self {
        Self(size)
    }

    /// Returns the raw byte count.
    #[must_use]
    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl From<usize> for RulesFileSize {
    fn from(size: usize) -> Self {
        Self(size)
    }
}

impl fmt::Display for RulesFileSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Content of a project rules file, possibly truncated to fit the soft cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesContent(String);

impl RulesContent {
    /// Creates a new [`RulesContent`] from the given string.
    #[must_use]
    pub fn new(content: String) -> Self {
        Self(content)
    }

    /// Returns the content as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the length of the content in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if the content is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<String> for RulesContent {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl fmt::Display for RulesContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for RulesContent {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// DetectedRules — the result of a successful detection + load
// ---------------------------------------------------------------------------

/// A detected project rules file with its loaded content.
///
/// Returned by [`RulesDetector::detect`] when a rules file is found.
/// The content may be truncated if the file exceeds [`DEFAULT_RULES_CAP_BYTES`].
#[derive(Debug, Clone)]
pub struct DetectedRules {
    /// Relative path to the rules file (relative to the repo root).
    path: RulesFilePath,
    /// Loaded content (possibly truncated).
    content: RulesContent,
    /// Original file size in bytes (before truncation).
    original_size: RulesFileSize,
    /// Whether the content was truncated to fit the soft cap.
    truncated: bool,
}

impl DetectedRules {
    /// Creates a new [`DetectedRules`] instance.
    #[must_use]
    pub fn new(
        path: RulesFilePath,
        content: RulesContent,
        original_size: RulesFileSize,
        truncated: bool,
    ) -> Self {
        Self {
            path,
            content,
            original_size,
            truncated,
        }
    }

    /// Returns the relative path to the rules file.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    /// Returns the loaded content (possibly truncated).
    #[must_use]
    pub fn content(&self) -> &str {
        self.content.as_str()
    }

    /// Returns the original file size in bytes (before truncation).
    #[must_use]
    pub fn original_size(&self) -> usize {
        self.original_size.as_usize()
    }

    /// Returns `true` if the content was truncated to fit the soft cap.
    #[must_use]
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    /// Returns the relative path as a [`RulesFilePath`] newtype.
    #[must_use]
    pub fn file_path(&self) -> &RulesFilePath {
        &self.path
    }
}

// ---------------------------------------------------------------------------
// RulesDetector — the detector with builder pattern
// ---------------------------------------------------------------------------

/// Detector for project rules files.
///
/// Scans a repository root for AI-agent instruction files in a fixed priority
/// order and loads the first match. Use the builder API for construction:
///
/// ```
/// use rs_guard::rules::RulesDetector;
/// use std::path::PathBuf;
///
/// let detector = RulesDetector::builder()
///     .repo_root(PathBuf::from("."))
///     .build()
///     .expect("builder should succeed");
/// ```
#[derive(Debug, Clone)]
pub struct RulesDetector {
    /// Repository root to scan for rules files.
    repo_root: PathBuf,
    /// Maximum content size in bytes before truncation.
    cap_bytes: usize,
}

impl RulesDetector {
    /// Creates a new [`RulesDetectorBuilder`] for fluent construction.
    #[must_use]
    pub fn builder() -> RulesDetectorBuilder {
        RulesDetectorBuilder::default()
    }

    /// Returns the configured repo root.
    #[must_use]
    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    /// Returns the configured soft cap in bytes.
    #[must_use]
    pub fn cap_bytes(&self) -> usize {
        self.cap_bytes
    }

    /// Detects and loads the first matching rules file.
    ///
    /// Scans the priority order and returns the first file found. If the file
    /// content exceeds [`RulesDetector::cap_bytes`], it is truncated and a
    /// warning banner is appended.
    ///
    /// # Errors
    ///
    /// Returns [`RsGuardError::Config`] if a rules file exists but cannot be read.
    ///
    /// # Returns
    ///
    /// - `Ok(Some(DetectedRules))` if a rules file was found and loaded.
    /// - `Ok(None)` if no rules file was found.
    pub fn detect(&self) -> Result<Option<DetectedRules>, RsGuardError> {
        let candidates = self.scan_priority_order();

        for relative_path in candidates {
            let full_path = self.repo_root.join(&relative_path);
            if full_path.is_file() {
                let (content, original_size, truncated) =
                    self.read_with_cap(&full_path, &relative_path)?;

                return Ok(Some(DetectedRules::new(
                    RulesFilePath::new(relative_path),
                    RulesContent::new(content),
                    RulesFileSize::new(original_size),
                    truncated,
                )));
            }
        }

        Ok(None)
    }

    /// Reads a rules file, loading only up to `cap_bytes + 1` bytes to avoid
    /// reading unnecessarily large files into memory.
    ///
    /// If the file size exceeds the cap, only the first `cap_bytes` bytes are
    /// read and a truncation warning banner is appended. The `original_size`
    /// is obtained from file metadata, not from the read content.
    ///
    /// # Errors
    ///
    /// Returns [`RsGuardError::Config`] if the file cannot be opened, read, or
    /// its metadata cannot be obtained.
    fn read_with_cap(
        &self,
        full_path: &Path,
        relative_path: &Path,
    ) -> Result<(String, usize, bool), RsGuardError> {
        read_rules_file_with_cap(full_path, relative_path, self.cap_bytes)
    }

    /// Returns all matching rules files in priority order.
    ///
    /// Unlike [`RulesDetector::detect`], this does not load file content — it
    /// only returns the paths. Used by the Phase 2 interactive picker to
    /// present all options to the user.
    #[must_use]
    pub fn detect_all_files(&self) -> Vec<RulesFilePath> {
        let candidates = self.scan_priority_order();
        let mut found = Vec::new();

        for relative_path in candidates {
            let full_path = self.repo_root.join(&relative_path);
            if full_path.is_file() {
                found.push(RulesFilePath::new(relative_path));
            }
        }

        found
    }

    /// Builds the full priority-ordered list of candidate relative paths.
    ///
    /// The `.cursor/rules/*.md` glob is expanded to **all** matching `.md`
    /// files (sorted alphabetically). For single-match detection
    /// ([`RulesDetector::detect`]), the first file in the sorted list wins.
    /// For [`RulesDetector::detect_all_files`], all cursor `.md` files are
    /// included so the Phase 2 interactive picker can present every option.
    fn scan_priority_order(&self) -> Vec<PathBuf> {
        let mut candidates: Vec<PathBuf> = RULES_FILE_PRIORITY.iter().map(PathBuf::from).collect();

        // Expand .cursor/rules/*.md glob — sorted alphabetically
        let cursor_dir = self.repo_root.join(CURSOR_RULES_DIR);
        if cursor_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&cursor_dir) {
                let mut md_files: Vec<PathBuf> = entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let path = e.path();
                        if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
                            // Convert to relative path
                            path.strip_prefix(&self.repo_root).ok().map(PathBuf::from)
                        } else {
                            None
                        }
                    })
                    .collect();
                md_files.sort();
                candidates.extend(md_files);
            }
        }

        // Windsurf rules file (lowest priority)
        candidates.push(PathBuf::from(WINDSURF_RULES_FILE));

        candidates
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Builder for [`RulesDetector`].
///
/// Provides fluent, validated construction. The terminal [`build`](RulesDetectorBuilder::build)
/// method returns `Result`, erroring if required fields are missing.
///
/// ```
/// use rs_guard::rules::RulesDetectorBuilder;
/// use std::path::PathBuf;
///
/// let detector = RulesDetectorBuilder::default()
///     .repo_root(PathBuf::from("."))
///     .cap_bytes(16_384)
///     .build()
///     .expect("builder should succeed");
/// ```
#[derive(Debug, Clone, Default)]
pub struct RulesDetectorBuilder {
    /// Repository root to scan. Required.
    repo_root: Option<PathBuf>,
    /// Maximum content size in bytes before truncation.
    cap_bytes: usize,
}

impl RulesDetectorBuilder {
    /// Sets the repository root to scan for rules files.
    #[must_use]
    pub fn repo_root(mut self, root: PathBuf) -> Self {
        self.repo_root = Some(root);
        self
    }

    /// Sets the soft cap for content truncation in bytes.
    #[must_use]
    pub fn cap_bytes(mut self, cap: usize) -> Self {
        self.cap_bytes = cap;
        self
    }

    /// Builds the [`RulesDetector`], validating required fields.
    ///
    /// # Errors
    ///
    /// Returns [`RsGuardError::Config`] if `repo_root` was not set.
    pub fn build(self) -> Result<RulesDetector, RsGuardError> {
        let repo_root = self.repo_root.ok_or_else(|| {
            RsGuardError::Config("RulesDetector requires repo_root to be set".to_string())
        })?;

        let cap_bytes = if self.cap_bytes == 0 {
            DEFAULT_RULES_CAP_BYTES
        } else {
            self.cap_bytes
        };

        Ok(RulesDetector {
            repo_root,
            cap_bytes,
        })
    }
}

// ---------------------------------------------------------------------------
// Convenience function — matches the ticket's required public API
// ---------------------------------------------------------------------------

/// Detects and loads the first matching project rules file from a repo root.
///
/// Convenience wrapper around [`RulesDetector`] with default settings
/// (32 KB soft cap). Returns `None` if no rules file is found.
///
/// # Errors
///
/// Returns [`RsGuardError::Config`] if a rules file exists but cannot be read.
pub fn detect_project_rules(repo_root: &Path) -> Result<Option<DetectedRules>, RsGuardError> {
    RulesDetector::builder()
        .repo_root(repo_root.to_path_buf())
        .build()?
        .detect()
}

/// Returns all detected project rules files in priority order.
///
/// Unlike [`detect_project_rules`], this does not load file content and returns
/// every matching file, not just the first. The returned paths are relative to
/// `repo_root` and ordered by the standard priority list.
///
/// # Errors
///
/// Returns [`RsGuardError::Config`] if scanning the directory fails.
pub fn detect_all_rules_files(repo_root: &Path) -> Result<Vec<PathBuf>, RsGuardError> {
    let detector = RulesDetector::builder()
        .repo_root(repo_root.to_path_buf())
        .build()?;
    Ok(detector
        .detect_all_files()
        .into_iter()
        .map(|p| repo_root.join(p.as_path()))
        .collect())
}

/// Loads a specific project rules file with the default soft cap.
///
/// The file path may be relative to the current working directory or absolute.
/// If the file exceeds [`DEFAULT_RULES_CAP_BYTES`], its content is truncated
/// and a warning banner is appended.
///
/// # Errors
///
/// Returns [`RsGuardError::Config`] if the file does not exist or cannot be read.
pub fn load_rules_file(path: &Path) -> Result<DetectedRules, RsGuardError> {
    if !path.exists() {
        return Err(RsGuardError::Config(format!(
            "Rules file not found: {}",
            path.display()
        )));
    }

    let (content, original_size, truncated) =
        read_rules_file_with_cap(path, path, DEFAULT_RULES_CAP_BYTES)?;
    Ok(DetectedRules::new(
        RulesFilePath::new(path.to_path_buf()),
        RulesContent::new(content),
        RulesFileSize::new(original_size),
        truncated,
    ))
}

/// Selects a rules file from a list of detected files.
///
/// If fewer than two files are detected, or if stdin is not a TTY, the first
/// file (highest priority) is returned. Otherwise, `select_fn` is invoked with
/// the display labels and its returned index selects the file. If `select_fn`
/// errors or returns an invalid index, the first file is used as a safe
/// fallback.
///
/// This function is designed to be testable: production code passes a
/// `dialoguer::Select`-based closure, while tests pass a mock selector.
pub fn select_rules_file<F>(files: &[PathBuf], is_tty: bool, select_fn: F) -> Option<&Path>
where
    F: FnOnce(&[String]) -> Result<usize, RsGuardError>,
{
    if files.is_empty() {
        return None;
    }
    if files.len() < 2 || !is_tty {
        return files.first().map(PathBuf::as_path);
    }

    let labels: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    match select_fn(&labels) {
        Ok(index) => files.get(index).map(PathBuf::as_path),
        Err(_) => files.first().map(PathBuf::as_path),
    }
}

/// Decides whether the interactive rules file picker should be shown.
///
/// The picker is shown only in local mode, when two or more rules files are
/// detected, no explicit `--rules-file` is set, `--no-project-rules` is not
/// set, and stdin is a TTY.
#[must_use]
pub fn should_show_picker(
    is_ci: bool,
    file_count: usize,
    rules_file: Option<&Path>,
    no_project_rules: bool,
    is_tty: bool,
) -> bool {
    !is_ci && file_count >= 2 && rules_file.is_none() && !no_project_rules && is_tty
}

// ---------------------------------------------------------------------------
// Soft cap truncation logic
// ---------------------------------------------------------------------------

/// Reads a rules file, loading only up to `cap_bytes + 1` bytes to avoid
/// reading unnecessarily large files into memory.
///
/// If the file size exceeds the cap, only the first `cap_bytes` bytes are
/// read and a truncation warning banner is appended. The `original_size` is
/// obtained from file metadata, not from the read content.
///
/// # Errors
///
/// Returns [`RsGuardError::Config`] if the file cannot be opened, read, or
/// its metadata cannot be obtained.
fn read_rules_file_with_cap(
    full_path: &Path,
    relative_path: &Path,
    cap_bytes: usize,
) -> Result<(String, usize, bool), RsGuardError> {
    // Get original size from metadata — avoids reading the entire file
    // just to learn its length.
    let original_size = fs::metadata(full_path)
        .map(|m| m.len() as usize)
        .map_err(|e| {
            RsGuardError::Config(format!(
                "Failed to read rules file metadata {}: {}",
                full_path.display(),
                e
            ))
        })?;

    if original_size <= cap_bytes {
        // File fits within the cap — read it fully
        let content = fs::read_to_string(full_path).map_err(|e| {
            RsGuardError::Config(format!(
                "Failed to read rules file {}: {}",
                full_path.display(),
                e
            ))
        })?;
        return Ok((content, original_size, false));
    }

    // File exceeds the cap — read only up to cap_bytes + 1 byte.
    // The extra byte lets us confirm the file is indeed larger than the cap
    // (defensive: metadata size and actual readable content may differ on
    // some filesystems). We then truncate to fit the banner overhead.
    let mut file = File::open(full_path).map_err(|e| {
        RsGuardError::Config(format!(
            "Failed to open rules file {}: {}",
            full_path.display(),
            e
        ))
    })?;

    // Read only what we need: cap_bytes + 1 to detect overflow
    let mut raw_bytes = vec![0u8; cap_bytes + 1];
    let bytes_read = file
        .read(&mut raw_bytes)
        .map_err(|e| RsGuardError::Config(format!("Failed to read rules file: {}", e)))?;
    raw_bytes.truncate(bytes_read);

    // Convert to string — truncate at UTF-8 boundary if we read a
    // partial multi-byte character at the end of the buffer.
    let content = match std::str::from_utf8(&raw_bytes) {
        Ok(s) => s.to_string(),
        Err(e) => {
            // Truncate at the last valid UTF-8 boundary
            let valid_up_to = e.valid_up_to();
            raw_bytes.truncate(valid_up_to);
            String::from_utf8(raw_bytes)
                .unwrap_or_else(|_| String::from_utf8_lossy(&[]).into_owned())
        }
    };

    // Apply the soft cap with banner (subtracts banner overhead from cap)
    let (result, _) = apply_soft_cap(content, cap_bytes, relative_path, original_size);
    Ok((result, original_size, true))
}

/// Applies the soft cap to content. If the content exceeds the cap, it is
/// truncated and a warning banner is appended.
///
/// Returns `(truncated_content, was_truncated)`.
fn apply_soft_cap(
    content: String,
    cap_bytes: usize,
    file_path: &Path,
    original_size: usize,
) -> (String, bool) {
    if content.len() <= cap_bytes {
        return (content, false);
    }

    // The final output must fit within cap_bytes. Since we append a warning
    // banner + separator after the truncated body, we must subtract their
    // combined length from the cap to determine how many bytes of the original
    // content we can keep. This ensures: body.len() + separator.len() +
    // banner.len() <= cap_bytes.
    let banner = truncation_banner(file_path, original_size, cap_bytes);
    let separator = "\n\n";
    let overhead = banner.len() + separator.len();
    let available = cap_bytes.saturating_sub(overhead);

    // Truncate at a char boundary to avoid splitting UTF-8
    let truncated_body = truncate_at_char_boundary(&content, available);

    let result = format!("{truncated_body}{separator}{banner}");
    (result, true)
}

/// Truncates a string to at most `max_bytes`, ensuring we don't split a
/// multi-byte UTF-8 character.
fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }

    // Walk backwards from max_bytes to find a char boundary
    let mut idx = max_bytes;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    &s[..idx]
}

/// Generates the truncation warning banner injected into the prompt.
fn truncation_banner(file_path: &Path, original_size: usize, cap_bytes: usize) -> String {
    format!(
        "--- BEGIN TRUNCATION WARNING ---\n\
         The project rules file ({path}) was truncated from {original} bytes to fit within \
         the {cap} byte content limit. The rules above are incomplete — refer to the full \
         file for complete project conventions.\n\
         --- END TRUNCATION WARNING ---",
        path = file_path.display(),
        original = original_size,
        cap = cap_bytes,
    )
}

// ---------------------------------------------------------------------------
// Inline unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_truncate_at_char_boundary_ascii() {
        let s = "abcdefghij";
        assert_eq!(truncate_at_char_boundary(s, 5), "abcde");
    }

    #[test]
    fn test_truncate_at_char_boundary_multibyte() {
        // Each emoji is 4 bytes in UTF-8
        let s = "😀😁😂😃😄";
        // Truncate at 5 bytes — should fall back to 4 (one emoji)
        assert_eq!(truncate_at_char_boundary(s, 5), "😀");
        // Truncate at 8 bytes — exactly two emojis
        assert_eq!(truncate_at_char_boundary(s, 8), "😀😁");
    }

    #[test]
    fn test_truncate_at_char_boundary_no_truncation_needed() {
        let s = "abc";
        assert_eq!(truncate_at_char_boundary(s, 10), "abc");
    }

    #[test]
    fn test_apply_soft_cap_no_truncation() {
        let content = "hello".to_string();
        let (result, truncated) =
            apply_soft_cap(content.clone(), 100, Path::new("AGENTS.md"), content.len());
        assert!(!truncated);
        assert_eq!(result, content);
    }

    #[test]
    fn test_apply_soft_cap_with_truncation() {
        // Use a realistic cap that is larger than the banner itself
        let cap = 500;
        let content = "x".repeat(1000);
        let (result, truncated) = apply_soft_cap(content, cap, Path::new("AGENTS.md"), 1000);
        assert!(truncated);
        assert!(
            result.len() <= cap,
            "truncated content should fit within the cap"
        );
        assert!(result.contains("TRUNCATION WARNING"));
        assert!(result.contains("AGENTS.md"));
    }

    #[test]
    fn test_truncation_banner_contains_key_info() {
        let banner = truncation_banner(Path::new("CLAUDE.md"), 50000, 32768);
        assert!(banner.contains("TRUNCATION WARNING"));
        assert!(banner.contains("CLAUDE.md"));
        assert!(banner.contains("50000"));
        assert!(banner.contains("32768"));
    }

    #[test]
    fn test_truncation_banner_full_structure() {
        let banner = truncation_banner(Path::new("AGENTS.md"), 40960, 32768);
        // Verify the banner has clear delimiters for the LLM
        assert!(
            banner.contains("--- BEGIN TRUNCATION WARNING ---"),
            "banner should start with a clear delimiter"
        );
        assert!(
            banner.contains("--- END TRUNCATION WARNING ---"),
            "banner should end with a clear delimiter"
        );
        // Verify it names the file, original size, and cap
        assert!(banner.contains("AGENTS.md"));
        assert!(banner.contains("40960"));
        assert!(banner.contains("32768"));
        // Verify it instructs the LLM that rules are incomplete
        assert!(
            banner.contains("incomplete"),
            "banner should tell the LLM the rules are incomplete"
        );
        assert!(
            banner.contains("full file"),
            "banner should point the LLM to the full file"
        );
    }

    #[test]
    fn test_rules_file_path_display() {
        let path = RulesFilePath::from("AGENTS.md");
        assert_eq!(format!("{}", path), "AGENTS.md");
    }

    #[test]
    fn test_rules_content_as_ref() {
        let content = RulesContent::new("hello".to_string());
        let s: &str = content.as_ref();
        assert_eq!(s, "hello");
    }

    #[test]
    fn test_rules_file_size_ordering() {
        let small = RulesFileSize::new(100);
        let large = RulesFileSize::new(200);
        assert!(small < large);
    }

    #[test]
    fn test_detected_rules_builder() {
        let rules = DetectedRules::new(
            RulesFilePath::from("AGENTS.md"),
            RulesContent::new("rules".to_string()),
            RulesFileSize::new(5),
            false,
        );
        assert_eq!(rules.path(), Path::new("AGENTS.md"));
        assert_eq!(rules.content(), "rules");
        assert_eq!(rules.original_size(), 5);
        assert!(!rules.is_truncated());
    }

    #[test]
    fn test_detector_builder_default_cap() {
        let detector = RulesDetector::builder()
            .repo_root(PathBuf::from("."))
            .build()
            .expect("build should succeed");
        assert_eq!(detector.cap_bytes(), DEFAULT_RULES_CAP_BYTES);
    }

    #[test]
    fn test_detector_builder_custom_cap() {
        let detector = RulesDetector::builder()
            .repo_root(PathBuf::from("."))
            .cap_bytes(1024)
            .build()
            .expect("build should succeed");
        assert_eq!(detector.cap_bytes(), 1024);
    }

    #[test]
    fn test_detector_builder_missing_repo_root() {
        let result = RulesDetector::builder().build();
        assert!(result.is_err());
    }

    #[test]
    fn test_scan_priority_order_includes_all_slots() {
        let dir = TempDir::new().expect("temp dir");
        // Create .cursor/rules with a .md file
        let cursor_dir = dir.path().join(".cursor/rules");
        fs::create_dir_all(&cursor_dir).expect("create cursor dir");
        fs::write(cursor_dir.join("rules.md"), "# Cursor\n").expect("write cursor");

        let detector = RulesDetector::builder()
            .repo_root(dir.path().to_path_buf())
            .build()
            .expect("build");
        let candidates = detector.scan_priority_order();

        // Should include 4 static + cursor glob + windsurf = 6
        assert!(candidates.len() >= 6);
        // First 4 should be the static priority
        assert_eq!(candidates[0], PathBuf::from("AGENTS.md"));
        assert_eq!(candidates[1], PathBuf::from("CLAUDE.md"));
        assert_eq!(
            candidates[2],
            PathBuf::from(".github/copilot-instructions.md")
        );
        assert_eq!(candidates[3], PathBuf::from(".gemini/styleguide.md"));
        // Last should be windsurfrules
        assert_eq!(candidates.last().unwrap(), &PathBuf::from(".windsurfrules"));
    }

    #[test]
    fn test_detect_all_rules_files_empty() {
        let dir = TempDir::new().expect("temp dir");
        let found = detect_all_rules_files(dir.path()).expect("detect should not error");
        assert!(found.is_empty(), "no rules files should be found");
    }

    #[test]
    fn test_detect_all_rules_files_priority_order() {
        let dir = TempDir::new().expect("temp dir");
        // Create two files. AGENTS.md has higher priority than CLAUDE.md.
        fs::write(dir.path().join("CLAUDE.md"), "# Claude\n").expect("write CLAUDE.md");
        fs::write(dir.path().join("AGENTS.md"), "# Agents\n").expect("write AGENTS.md");

        let found = detect_all_rules_files(dir.path()).expect("detect should not error");
        assert_eq!(found.len(), 2, "both files should be detected");
        assert_eq!(
            found[0],
            dir.path().join("AGENTS.md"),
            "AGENTS.md should be first"
        );
        assert_eq!(
            found[1],
            dir.path().join("CLAUDE.md"),
            "CLAUDE.md should be second"
        );
    }

    #[test]
    fn test_detect_all_rules_files_single_file() {
        let dir = TempDir::new().expect("temp dir");
        fs::write(dir.path().join("CLAUDE.md"), "# Claude\n").expect("write CLAUDE.md");

        let found = detect_all_rules_files(dir.path()).expect("detect should not error");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], dir.path().join("CLAUDE.md"));
    }

    #[test]
    fn test_detect_all_rules_files_includes_cursor_glob() {
        let dir = TempDir::new().expect("temp dir");
        let cursor_dir = dir.path().join(".cursor/rules");
        fs::create_dir_all(&cursor_dir).expect("create cursor dir");
        fs::write(cursor_dir.join("a.md"), "# A\n").expect("write a.md");
        fs::write(cursor_dir.join("b.md"), "# B\n").expect("write b.md");

        let found = detect_all_rules_files(dir.path()).expect("detect should not error");
        let cursor_a = dir.path().join(".cursor/rules/a.md");
        let cursor_b = dir.path().join(".cursor/rules/b.md");
        assert!(
            found.contains(&cursor_a),
            "cursor a.md should be in the detected list"
        );
        assert!(
            found.contains(&cursor_b),
            "cursor b.md should be in the detected list"
        );
    }

    #[test]
    fn test_select_rules_file_empty_returns_none() {
        let files: Vec<PathBuf> = Vec::new();
        let result = select_rules_file(&files, true, |_| Ok(0));
        assert!(result.is_none(), "empty list should return None");
    }

    #[test]
    fn test_select_rules_file_single_file_returns_first() {
        let files = vec![PathBuf::from("AGENTS.md")];
        let result = select_rules_file(&files, true, |_| panic!("selector should not be called"));
        assert_eq!(result, Some(Path::new("AGENTS.md")));
    }

    #[test]
    fn test_select_rules_file_non_tty_returns_first() {
        let files = vec![PathBuf::from("AGENTS.md"), PathBuf::from("CLAUDE.md")];
        let result = select_rules_file(&files, false, |_| panic!("selector should not be called"));
        assert_eq!(
            result,
            Some(Path::new("AGENTS.md")),
            "non-TTY should use first match"
        );
    }

    #[test]
    fn test_select_rules_file_tty_uses_selector() {
        let files = vec![PathBuf::from("AGENTS.md"), PathBuf::from("CLAUDE.md")];
        let result = select_rules_file(&files, true, |_| Ok(1));
        assert_eq!(
            result,
            Some(Path::new("CLAUDE.md")),
            "TTY should use selector result"
        );
    }

    #[test]
    fn test_select_rules_file_selector_error_falls_back_to_first() {
        let files = vec![PathBuf::from("AGENTS.md"), PathBuf::from("CLAUDE.md")];
        let result = select_rules_file(&files, true, |_| {
            Err(RsGuardError::Config("cancelled".to_string()))
        });
        assert_eq!(
            result,
            Some(Path::new("AGENTS.md")),
            "selector error should fall back to first match"
        );
    }

    #[test]
    fn test_should_show_picker_all_conditions_met() {
        assert!(
            should_show_picker(false, 2, None, false, true),
            "local mode + 2 files + no override + tty should show picker"
        );
    }

    #[test]
    fn test_should_show_picker_ci_mode_skips() {
        assert!(
            !should_show_picker(true, 2, None, false, true),
            "CI mode should skip picker"
        );
    }

    #[test]
    fn test_should_show_picker_single_file_skips() {
        assert!(
            !should_show_picker(false, 1, None, false, true),
            "single file should not show picker"
        );
    }

    #[test]
    fn test_should_show_picker_explicit_rules_file_skips() {
        assert!(
            !should_show_picker(false, 2, Some(Path::new("custom.md")), false, true),
            "explicit rules_file should skip picker"
        );
    }

    #[test]
    fn test_should_show_picker_no_project_rules_skips() {
        assert!(
            !should_show_picker(false, 2, None, true, true),
            "--no-project-rules should skip picker"
        );
    }

    #[test]
    fn test_should_show_picker_non_tty_skips() {
        assert!(
            !should_show_picker(false, 2, None, false, false),
            "non-TTY should skip picker"
        );
    }

    #[test]
    fn test_should_show_picker_empty_files_skips() {
        assert!(
            !should_show_picker(false, 0, None, false, true),
            "no files should skip picker"
        );
    }
}
