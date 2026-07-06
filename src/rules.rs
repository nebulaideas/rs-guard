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
use std::fs;
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
                let content = fs::read_to_string(&full_path).map_err(|e| {
                    RsGuardError::Config(format!(
                        "Failed to read rules file {}: {}",
                        full_path.display(),
                        e
                    ))
                })?;
                let original_size = content.len();
                let (truncated_content, truncated) =
                    apply_soft_cap(content, self.cap_bytes, &relative_path, original_size);

                return Ok(Some(DetectedRules::new(
                    RulesFilePath::new(relative_path),
                    RulesContent::new(truncated_content),
                    RulesFileSize::new(original_size),
                    truncated,
                )));
            }
        }

        Ok(None)
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
    /// Handles the `.cursor/rules/*.md` glob specially by expanding it to
    /// the first `.md` file alphabetically (for single-match detection) or
    /// all `.md` files (for `detect_all_files`).
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

// ---------------------------------------------------------------------------
// Soft cap truncation logic
// ---------------------------------------------------------------------------

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

    // Truncate to cap, leaving room for the warning banner + separator
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
}
