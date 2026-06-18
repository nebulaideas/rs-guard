//! Response caching for LLM results.
//!
//! Caches LLM responses by SHA-256 diff hash to avoid redundant API calls.
//! Cache location: `.rs-guard/cache/` (project-local).
//!
//! # Cache Format
//!
//! Each cache entry is a `.cache` file containing:
//! - Line 1: Unix timestamp (seconds since epoch) of when the entry was created
//! - Line 2+: The cached LLM response
//!
//! # Concurrency
//!
//! Writes are atomic: content is written to a temporary file, then renamed
//! into place. This prevents partial reads by concurrent processes.
//!
//! # Size Limits
//!
//! The cache has a configurable maximum size (default: 100MB). When the limit
//! is exceeded, the oldest entries are automatically removed.

use crate::error::RsGuardError;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Default cache directory relative to project root.
const DEFAULT_CACHE_DIR: &str = ".rs-guard/cache";

/// Default cache TTL: 24 hours.
const DEFAULT_TTL_SECS: u64 = 86400;

/// Default cache size limit: 100MB.
const DEFAULT_MAX_SIZE_BYTES: u64 = 100 * 1024 * 1024;

/// Cache entry file extension.
const CACHE_FILE_EXT: &str = "cache";

static CACHE_WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Attempts to find the git repository root by running `git rev-parse --show-toplevel`.
///
/// Returns `None` if git is not available or we're not inside a git repository.
fn find_git_root() -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout);
        Some(PathBuf::from(path.trim()))
    } else {
        None
    }
}

/// Returns the default cache directory.
///
/// Uses the git repository root if available, otherwise falls back to the
/// current working directory. This ensures cache consistency when rs-guard
/// is invoked from subdirectories (e.g., in a monorepo).
fn default_cache_dir() -> PathBuf {
    find_git_root()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join(DEFAULT_CACHE_DIR)
}

/// Cache configuration.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Directory to store cache files.
    pub cache_dir: PathBuf,
    /// Time-to-live for cached entries.
    pub ttl: Duration,
    /// Whether caching is enabled.
    pub enabled: bool,
    /// Maximum total size of cache in bytes.
    pub max_size_bytes: u64,
    /// Whether to automatically add the cache directory to `.gitignore`.
    pub auto_gitignore: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            cache_dir: default_cache_dir(),
            ttl: Duration::from_secs(DEFAULT_TTL_SECS),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        }
    }
}

/// Cache key components that uniquely identify a cache entry.
#[derive(Debug, Clone)]
struct CacheKey {
    /// SHA-256 hash of the diff content.
    diff_hash: String,
    /// SHA-256 hash of the prompt.
    prompt_hash: String,
    /// Provider name.
    provider: String,
    /// Model identifier.
    model: String,
    /// Provider-specific model variant.
    variant: Option<String>,
    /// Sampling temperature.
    temperature: f32,
    /// Effective API base URL (prevents cache poisoning across endpoint overrides).
    base_url: String,
    /// Maximum tokens cap (different caps must not share a cache entry).
    max_tokens: Option<u32>,
}

impl CacheKey {
    /// Creates a new cache key from the given components.
    ///
    /// All parameters that affect the outgoing request MUST be included here:
    /// omitting one risks cache poisoning or staleness (e.g. a `base_url`
    /// override poisoning the entry for the real provider endpoint, or a
    /// `max_tokens` cap serving a truncated response to a full-length run).
    #[allow(clippy::too_many_arguments)]
    fn new(
        diff_content: &str,
        prompt: &str,
        provider: &str,
        model: &str,
        variant: Option<&str>,
        temperature: f32,
        base_url: &str,
        max_tokens: Option<u32>,
    ) -> Self {
        let diff_hash = hash_content(diff_content);
        let prompt_hash = hash_content(prompt);
        Self {
            diff_hash,
            prompt_hash,
            provider: provider.to_string(),
            model: model.to_string(),
            variant: variant.map(|v| v.to_lowercase()),
            temperature,
            base_url: base_url.to_string(),
            max_tokens,
        }
    }

    /// Returns the cache key as a hex string.
    ///
    /// A `0x00` separator byte is written between variable-length fields so
    /// that distinct field splits cannot produce the same hash
    /// (e.g. provider="a", model="bc" vs provider="ab", model="c").
    /// Fixed-length fields (diff_hash/prompt_hash are 64-char hex;
    /// temperature is 4 bytes; max_tokens uses a presence tag) need no separator.
    fn as_string(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.diff_hash.as_bytes());
        hasher.update([0]);
        hasher.update(self.prompt_hash.as_bytes());
        hasher.update([0]);
        hasher.update(self.provider.as_bytes());
        hasher.update([0]);
        hasher.update(self.model.as_bytes());
        hasher.update([0]);
        if let Some(ref variant) = self.variant {
            hasher.update(variant.as_bytes());
        }
        hasher.update([0]);
        hasher.update(self.base_url.as_bytes());
        hasher.update([0]);
        hasher.update(self.temperature.to_le_bytes());
        // Presence tag ensures Some(x) and None never collide, and distinguishes
        // by value when set.
        match self.max_tokens {
            Some(n) => {
                hasher.update([1]);
                hasher.update(n.to_le_bytes());
            }
            None => hasher.update([0]),
        }
        hex::encode(hasher.finalize())
    }
}

/// Computes a hex-encoded SHA-256 hash of the given content.
fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// Computes a hex-encoded SHA-256 hash of the given diff content.
///
/// The hash is used as the cache key.
///
/// # Examples
///
/// ```
/// use rs_guard::cache::diff_hash;
/// let hash = diff_hash("diff --git a/f.rs b/f.rs");
/// assert_eq!(hash.len(), 64);
/// ```
pub fn diff_hash(content: &str) -> String {
    hash_content(content)
}

/// Cache for LLM review responses indexed by diff content hash.
///
/// Each cache entry is a file named `{hash}.cache` in the cache directory.
/// The file contains a timestamp on the first line, followed by the response.
#[derive(Debug, Clone)]
pub struct DiffCache {
    /// Cache configuration.
    config: CacheConfig,
}

impl DiffCache {
    /// Creates a new cache with the given configuration.
    ///
    /// The cache directory is created if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`RsGuardError::Config`] if the cache directory cannot be created.
    pub fn new(config: CacheConfig) -> Result<Self, RsGuardError> {
        let cache = Self {
            config: config.clone(),
        };
        if config.enabled {
            cache.ensure_cache_dir()?;
        }
        Ok(cache)
    }

    /// Returns the cache file path for a given hash key.
    fn cache_path(&self, key: &str) -> PathBuf {
        self.config
            .cache_dir
            .join(format!("{}.{}", key, CACHE_FILE_EXT))
    }

    /// Ensures the cache directory exists, creating it if necessary.
    fn ensure_cache_dir(&self) -> Result<(), RsGuardError> {
        fs::create_dir_all(&self.config.cache_dir)
            .map_err(|e| RsGuardError::Config(format!("Failed to create cache dir: {}", e)))
    }

    /// Returns the current time as seconds since Unix epoch.
    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Reads a cache entry and checks if it's still fresh.
    ///
    /// Returns `Some(response)` if the entry exists and is within TTL.
    /// Returns `None` if the entry doesn't exist, is expired, or is corrupt.
    fn read_entry(&self, path: &Path) -> Option<String> {
        let content = fs::read_to_string(path).ok()?;

        // First line is the timestamp
        let newline_idx = content.find('\n')?;
        let timestamp_str = &content[..newline_idx];
        let timestamp: u64 = timestamp_str.parse().ok()?;

        // Check TTL (use >= so TTL=0 means immediately expired)
        let now = Self::now_secs();
        let age = now.saturating_sub(timestamp);
        if age >= self.config.ttl.as_secs() {
            // Entry expired - remove it
            let _ = fs::remove_file(path);
            return None;
        }

        // Rest is the response (preserve trailing newlines)
        let response = &content[newline_idx + 1..];
        if response.is_empty() {
            return None;
        }

        Some(response.to_string())
    }

    /// Retrieves a cached response by diff content hash.
    ///
    /// Returns `None` if the key is not cached, the entry is expired,
    /// or caching is disabled.
    ///
    /// # Arguments
    ///
    /// * `diff_content` — Diff content to hash and look up.
    /// * `prompt` — System prompt used for the review.
    /// * `provider` — LLM provider name.
    /// * `model` — Model identifier.
    /// * `variant` — Provider-specific model variant.
    /// * `temperature` — Sampling temperature.
    /// * `base_url` — Effective API base URL (prevents cross-endpoint poisoning).
    /// * `max_tokens` — Optional maximum tokens cap (prevents truncation staleness).
    #[allow(clippy::too_many_arguments)]
    pub fn get(
        &self,
        diff_content: &str,
        prompt: &str,
        provider: &str,
        model: &str,
        variant: Option<&str>,
        temperature: f32,
        base_url: &str,
        max_tokens: Option<u32>,
    ) -> Option<String> {
        if !self.config.enabled {
            return None;
        }

        let key = CacheKey::new(
            diff_content,
            prompt,
            provider,
            model,
            variant,
            temperature,
            base_url,
            max_tokens,
        );
        let key_str = key.as_string();
        let path = self.cache_path(&key_str);

        if !path.exists() {
            return None;
        }

        match self.read_entry(&path) {
            Some(response) => {
                log::debug!("Cache hit for cache key: {}", key_str);
                Some(response)
            }
            None => {
                log::debug!("Cache miss or expired entry for cache key: {}", key_str);
                None
            }
        }
    }

    /// Stores a response in the cache, keyed by diff content hash.
    ///
    /// Writes atomically: content is first written to a temporary file
    /// in the same directory, then renamed into place.
    ///
    /// After writing, checks if the cache exceeds the size limit and
    /// removes old entries if necessary.
    ///
    /// # Arguments
    ///
    /// * `diff_content` — Diff content to hash and key by.
    /// * `prompt` — System prompt used for the review.
    /// * `provider` — LLM provider name.
    /// * `model` — Model identifier.
    /// * `variant` — Provider-specific model variant.
    /// * `temperature` — Sampling temperature.
    /// * `base_url` — Effective API base URL.
    /// * `max_tokens` — Optional maximum tokens cap.
    /// * `response` — The LLM response text to cache.
    ///
    /// # Errors
    ///
    /// Returns [`RsGuardError::Io`] if the file cannot be written.
    #[allow(clippy::too_many_arguments)]
    pub fn set(
        &self,
        diff_content: &str,
        prompt: &str,
        provider: &str,
        model: &str,
        variant: Option<&str>,
        temperature: f32,
        base_url: &str,
        max_tokens: Option<u32>,
        response: &str,
    ) -> Result<(), RsGuardError> {
        if !self.config.enabled {
            return Ok(());
        }

        let key = CacheKey::new(
            diff_content,
            prompt,
            provider,
            model,
            variant,
            temperature,
            base_url,
            max_tokens,
        );
        let key_str = key.as_string();
        let path = self.cache_path(&key_str);

        // Write to temp file with unique name in same directory, then atomically rename
        // Use timestamp + monotonic counter for uniqueness to prevent symlink attacks
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = CACHE_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_filename = format!("{}.{}.{}.tmp", key_str, timestamp, counter);
        let tmp_path = self.config.cache_dir.join(&tmp_filename);

        {
            let mut tmp = fs::File::options()
                .write(true)
                .create_new(true)
                .open(&tmp_path)?;

            // Write timestamp as first line
            writeln!(tmp, "{}", Self::now_secs())?;

            // Write response
            tmp.write_all(response.as_bytes())?;
            tmp.sync_all()?;
        }

        fs::rename(&tmp_path, &path)?;

        log::debug!("Cached response for cache key: {}", key_str);

        // Check size limit and cleanup if needed
        self.enforce_size_limit()?;

        Ok(())
    }

    /// Calculates the total size of all cache files.
    fn total_size(&self) -> Result<u64, RsGuardError> {
        let mut total = 0u64;

        let entries = fs::read_dir(&self.config.cache_dir).map_err(|e| {
            RsGuardError::Io(std::io::Error::other(format!(
                "Failed to read cache dir: {}",
                e
            )))
        })?;

        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                if metadata.is_file() {
                    total += metadata.len();
                }
            }
        }

        Ok(total)
    }

    /// Removes the oldest cache entries until total size is under the limit.
    fn enforce_size_limit(&self) -> Result<(), RsGuardError> {
        let total = self.total_size()?;

        if total <= self.config.max_size_bytes {
            return Ok(());
        }

        log::warn!(
            "Cache size {} bytes exceeds limit {} bytes, cleaning up",
            total,
            self.config.max_size_bytes
        );

        // Collect all cache files with their stored timestamps
        let mut files: Vec<(PathBuf, u64)> = Vec::new();

        let entries = fs::read_dir(&self.config.cache_dir).map_err(|e| {
            RsGuardError::Io(std::io::Error::other(format!(
                "Failed to read cache dir: {}",
                e
            )))
        })?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some(CACHE_FILE_EXT) {
                // Read the stored timestamp from the first line of the file
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Some(first_line) = content.lines().next() {
                        if let Ok(timestamp) = first_line.parse::<u64>() {
                            files.push((path, timestamp));
                        }
                    }
                }
            }
        }

        // Sort by stored timestamp (oldest first)
        files.sort_by_key(|a| a.1);

        // Remove oldest files until we're under the limit
        let mut current_size = total;
        for (path, _) in files {
            if current_size <= self.config.max_size_bytes {
                break;
            }

            if let Ok(metadata) = fs::metadata(&path) {
                let size = metadata.len();
                if fs::remove_file(&path).is_ok() {
                    log::debug!("Removed old cache entry: {:?}", path);
                    current_size = current_size.saturating_sub(size);
                }
            }
        }

        Ok(())
    }

    /// Attempts to auto-create a `.gitignore` entry for the cache directory.
    ///
    /// Adds the configured cache directory to the project's `.gitignore` if the file
    /// does not already contain an entry for the cache directory. The entry is written
    /// as a path relative to the git repository root (or the current working directory
    /// when no git repository is found).
    ///
    /// Returns `Ok(())` if the entry already exists, was successfully added, or if
    /// caching is disabled. Returns `Err` only on unexpected filesystem errors.
    pub fn ensure_gitignored(&self) -> Result<(), RsGuardError> {
        if !self.config.enabled || !self.config.auto_gitignore {
            return Ok(());
        }

        let git_root = find_git_root()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let gitignore_path = git_root.join(".gitignore");

        let entry = match Self::gitignore_entry(&git_root, &self.config.cache_dir) {
            Some(entry) => entry,
            None => {
                log::warn!(
                    "Cache dir {} is the same as the git root. Skipping .gitignore entry.",
                    self.config.cache_dir.display()
                );
                return Ok(());
            }
        };
        let entry_with_slash = format!("{}/", entry);

        match fs::read_to_string(&gitignore_path) {
            Ok(content) => {
                let has_entry = content.lines().any(|line| {
                    let normalized = Self::normalize_gitignore_line(line);
                    normalized == entry || normalized == entry_with_slash
                });
                if has_entry {
                    return Ok(());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(RsGuardError::Io(e));
            }
        }

        let mut f = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&gitignore_path)
            .map_err(RsGuardError::Io)?;

        f.write_all(format!("{}\n", entry_with_slash).as_bytes())
            .map_err(RsGuardError::Io)?;
        Ok(())
    }

    /// Computes the `.gitignore` entry for the cache directory relative to the git root.
    ///
    /// If the cache directory is inside the git root, returns the relative path with
    /// forward slashes. Otherwise, falls back to the configured path as-is.
    /// Returns `None` if the cache directory is the same as the git root.
    fn gitignore_entry(git_root: &Path, cache_dir: &Path) -> Option<String> {
        let cache_path = if cache_dir.is_absolute() {
            cache_dir.to_path_buf()
        } else {
            git_root.join(cache_dir)
        };

        // Prefer canonical paths so symlinked directories (e.g. /var -> /private/var)
        // resolve consistently before stripping the prefix.
        let relative =
            if let (Ok(root), Ok(cache)) = (git_root.canonicalize(), cache_path.canonicalize()) {
                cache.strip_prefix(&root).ok().map(|p| p.to_path_buf())
            } else {
                None
            };

        // Fall back to non-canonical paths if canonicalization fails.
        let relative = relative.or_else(|| {
            cache_path
                .strip_prefix(git_root)
                .ok()
                .map(|p| p.to_path_buf())
        });

        match relative {
            Some(path) => {
                let entry = path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .trim_end_matches('/')
                    .to_string();
                if entry.is_empty() {
                    None
                } else {
                    Some(entry)
                }
            }
            None => {
                log::warn!(
                    "Cache dir {} is outside git root {}. Using configured path in .gitignore.",
                    cache_path.display(),
                    git_root.display()
                );
                let entry = cache_dir
                    .to_string_lossy()
                    .replace('\\', "/")
                    .trim_end_matches('/')
                    .to_string();
                if entry.is_empty() {
                    None
                } else {
                    Some(entry)
                }
            }
        }
    }

    /// Normalizes a `.gitignore` line for duplicate detection.
    ///
    /// Trims whitespace and removes a trailing directory separator.
    fn normalize_gitignore_line(line: &str) -> String {
        line.trim().trim_end_matches('/').to_string()
    }

    /// Clears all cache entries.
    ///
    /// # Errors
    ///
    /// Returns [`RsGuardError::Io`] if the cache directory cannot be read
    /// or files cannot be removed.
    pub fn clear(&self) -> Result<(), RsGuardError> {
        let entries = fs::read_dir(&self.config.cache_dir).map_err(|e| {
            RsGuardError::Io(std::io::Error::other(format!(
                "Failed to read cache dir: {}",
                e
            )))
        })?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some(CACHE_FILE_EXT) {
                if let Err(e) = fs::remove_file(&path) {
                    log::warn!("Failed to remove cache entry {:?}: {}", path, e);
                }
            }
        }

        Ok(())
    }

    /// Returns statistics about the cache.
    pub fn stats(&self) -> Result<CacheStats, RsGuardError> {
        let mut file_count = 0u64;
        let mut total_size = 0u64;

        let entries = fs::read_dir(&self.config.cache_dir).map_err(|e| {
            RsGuardError::Io(std::io::Error::other(format!(
                "Failed to read cache dir: {}",
                e
            )))
        })?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some(CACHE_FILE_EXT) {
                if let Ok(metadata) = entry.metadata() {
                    file_count += 1;
                    total_size += metadata.len();
                }
            }
        }

        Ok(CacheStats {
            file_count,
            total_size_bytes: total_size,
            max_size_bytes: self.config.max_size_bytes,
        })
    }
}

/// Statistics about the cache state.
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of cache files.
    pub file_count: u64,
    /// Total size of cache in bytes.
    pub total_size_bytes: u64,
    /// Maximum allowed cache size in bytes.
    pub max_size_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Restores the process working directory when dropped.
    ///
    /// Tests that mutate `std::env::current_dir()` must use this guard so that
    /// a panic does not leave subsequent tests pointing at a deleted temp dir.
    struct RestoreCurrentDir(PathBuf);

    impl Drop for RestoreCurrentDir {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    #[test]
    fn test_diff_hash_consistent() {
        let content = "diff --git a/f.rs b/f.rs";
        let h1 = diff_hash(content);
        let h2 = diff_hash(content);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_diff_hash_different() {
        let h1 = diff_hash("content a");
        let h2 = diff_hash("content b");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_cache_key_includes_all_parameters() {
        let key1 = CacheKey::new(
            "diff",
            "prompt",
            "deepseek",
            "model",
            None,
            0.1,
            "https://default.example.com",
            None,
        );
        let key2 = CacheKey::new(
            "diff",
            "prompt",
            "deepseek",
            "model",
            None,
            0.1,
            "https://default.example.com",
            None,
        );
        let key3 = CacheKey::new(
            "diff",
            "prompt",
            "deepseek",
            "model",
            None,
            0.2,
            "https://default.example.com",
            None,
        );
        let key4 = CacheKey::new(
            "diff",
            "prompt",
            "openai",
            "model",
            None,
            0.1,
            "https://default.example.com",
            None,
        );
        let key5 = CacheKey::new(
            "diff",
            "prompt",
            "deepseek",
            "model",
            Some("flash"),
            0.1,
            "https://default.example.com",
            None,
        );

        assert_eq!(key1.as_string(), key2.as_string());
        assert_ne!(key1.as_string(), key3.as_string());
        assert_ne!(key1.as_string(), key4.as_string());
        assert_ne!(key1.as_string(), key5.as_string());
    }

    #[test]
    fn test_cache_key_isolates_base_url_override() {
        // Regression (F4): two runs with the same diff/prompt/provider/model
        // but DIFFERENT base_url must NOT share a cache entry. Prevents a
        // custom endpoint (e.g. a local mock) from poisoning the entry for
        // the real provider endpoint.
        let base = CacheKey::new(
            "diff",
            "prompt",
            "deepseek",
            "model",
            None,
            0.1,
            "https://api.deepseek.com",
            None,
        );
        let overridden = CacheKey::new(
            "diff",
            "prompt",
            "deepseek",
            "model",
            None,
            0.1,
            "http://localhost:11434",
            None,
        );
        assert_ne!(
            base.as_string(),
            overridden.as_string(),
            "different base_url must produce different cache keys (poisoning risk)"
        );
    }

    #[test]
    fn test_cache_key_isolates_max_tokens() {
        // Regression (F5): a run with max_tokens=Some(100) caches a truncated
        // response; a run with max_tokens=None must NOT hit that entry.
        let capped = CacheKey::new(
            "diff",
            "prompt",
            "deepseek",
            "model",
            None,
            0.1,
            "https://api.deepseek.com",
            Some(100),
        );
        let uncapped = CacheKey::new(
            "diff",
            "prompt",
            "deepseek",
            "model",
            None,
            0.1,
            "https://api.deepseek.com",
            None,
        );
        assert_ne!(
            capped.as_string(),
            uncapped.as_string(),
            "different max_tokens must produce different cache keys (truncation staleness)"
        );
    }

    #[test]
    fn test_cache_key_separator_prevents_field_collision() {
        // Regression (F6): provider="a", model="bc" must NOT collide with
        // provider="ab", model="c". The 0x00 separator between variable-length
        // fields guarantees this.
        let k1 = CacheKey::new("diff", "prompt", "a", "bc", None, 0.1, "url", None);
        let k2 = CacheKey::new("diff", "prompt", "ab", "c", None, 0.1, "url", None);
        assert_ne!(
            k1.as_string(),
            k2.as_string(),
            "field-split collision: separator not working"
        );
    }

    #[test]
    #[serial_test::serial]
    fn test_gitignore_auto_creation() {
        let dir = tempdir().unwrap();
        let cache_dir = Path::new(DEFAULT_CACHE_DIR);
        let config = CacheConfig {
            cache_dir: cache_dir.to_path_buf(),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        // Change to the temp directory for the test
        let original_dir = std::env::current_dir().unwrap();
        let _restore = RestoreCurrentDir(original_dir);
        std::env::set_current_dir(dir.path()).unwrap();

        // First call should create .gitignore
        cache.ensure_gitignored().unwrap();
        let gitignore_path = dir.path().join(".gitignore");
        assert!(gitignore_path.exists());
        let content = std::fs::read_to_string(&gitignore_path).unwrap();
        assert!(
            content
                .lines()
                .any(|line| line.trim() == ".rs-guard/cache/"),
            ".gitignore should contain exact relative entry: {}",
            content
        );
        let line_count_before = content.lines().count();

        // Second call should not duplicate the entry
        cache.ensure_gitignored().unwrap();
        let content_after = std::fs::read_to_string(&gitignore_path).unwrap();
        let line_count_after = content_after.lines().count();
        // Line count should be the same (no new lines added)
        assert_eq!(line_count_before, line_count_after);
    }

    #[test]
    #[serial_test::serial]
    fn test_gitignore_exact_line_matching() {
        let dir = tempdir().unwrap();
        let cache_dir = Path::new(DEFAULT_CACHE_DIR);
        let config = CacheConfig {
            cache_dir: cache_dir.to_path_buf(),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        // Change to the temp directory for the test
        let original_dir = std::env::current_dir().unwrap();
        let _restore = RestoreCurrentDir(original_dir);
        std::env::set_current_dir(dir.path()).unwrap();

        // Create .gitignore with a similar but different path
        let gitignore_path = dir.path().join(".gitignore");
        std::fs::write(&gitignore_path, ".rs-guard/cache2/\n").unwrap();

        // Should add the entry since it's not an exact match
        cache.ensure_gitignored().unwrap();
        let content = std::fs::read_to_string(&gitignore_path).unwrap();
        assert!(
            content
                .lines()
                .any(|line| line.trim() == ".rs-guard/cache/"),
            ".gitignore should contain exact relative entry: {}",
            content
        );
    }

    #[test]
    #[serial_test::serial]
    fn test_gitignore_auto_gitignore_false_skips_write() {
        let dir = tempdir().unwrap();
        let cache_dir = Path::new(DEFAULT_CACHE_DIR);
        let config = CacheConfig {
            cache_dir: cache_dir.to_path_buf(),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: false,
        };
        let cache = DiffCache::new(config).unwrap();

        let original_dir = std::env::current_dir().unwrap();
        let _restore = RestoreCurrentDir(original_dir);
        std::env::set_current_dir(dir.path()).unwrap();

        // Should not create .gitignore when auto_gitignore is false
        cache.ensure_gitignored().unwrap();
        let gitignore_path = dir.path().join(".gitignore");
        assert!(
            !gitignore_path.exists(),
            ".gitignore should not be created when auto_gitignore=false"
        );
    }

    #[test]
    #[serial_test::serial]
    fn test_gitignore_disabled_cache_skips_write() {
        let dir = tempdir().unwrap();
        let cache_dir = Path::new(DEFAULT_CACHE_DIR);
        let config = CacheConfig {
            cache_dir: cache_dir.to_path_buf(),
            ttl: Duration::from_secs(3600),
            enabled: false,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        let original_dir = std::env::current_dir().unwrap();
        let _restore = RestoreCurrentDir(original_dir);
        std::env::set_current_dir(dir.path()).unwrap();

        // Should not create .gitignore when cache is disabled
        cache.ensure_gitignored().unwrap();
        let gitignore_path = dir.path().join(".gitignore");
        assert!(
            !gitignore_path.exists(),
            ".gitignore should not be created when cache is disabled"
        );
    }

    #[test]
    #[serial_test::serial]
    fn test_gitignore_uses_relative_path_when_cache_dir_is_absolute() {
        let dir = tempdir().unwrap();
        let cache_dir = dir.path().join(DEFAULT_CACHE_DIR);
        let config = CacheConfig {
            cache_dir,
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        let original_dir = std::env::current_dir().unwrap();
        let _restore = RestoreCurrentDir(original_dir);
        std::env::set_current_dir(dir.path()).unwrap();

        cache.ensure_gitignored().unwrap();
        let gitignore_path = dir.path().join(".gitignore");
        let content = std::fs::read_to_string(&gitignore_path).unwrap();
        assert!(
            content
                .lines()
                .any(|line| line.trim() == ".rs-guard/cache/"),
            ".gitignore should contain exact relative entry: {}",
            content
        );
        assert!(
            !content
                .lines()
                .any(|line| line.contains(dir.path().to_string_lossy().as_ref())),
            ".gitignore should not contain absolute path: {}",
            content
        );
    }

    #[test]
    #[serial_test::serial]
    fn test_gitignore_does_not_duplicate_relative_entry() {
        let dir = tempdir().unwrap();
        let gitignore_path = dir.path().join(".gitignore");
        std::fs::write(&gitignore_path, ".rs-guard/cache\n").unwrap();

        let cache_dir = dir.path().join(DEFAULT_CACHE_DIR);
        let config = CacheConfig {
            cache_dir,
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        let original_dir = std::env::current_dir().unwrap();
        let _restore = RestoreCurrentDir(original_dir);
        std::env::set_current_dir(dir.path()).unwrap();

        cache.ensure_gitignored().unwrap();
        let content = std::fs::read_to_string(&gitignore_path).unwrap();
        let count = content
            .lines()
            .filter(|l| l.trim() == ".rs-guard/cache/" || l.trim() == ".rs-guard/cache")
            .count();
        assert_eq!(count, 1, "entry should not be duplicated: {}", content);
    }

    #[test]
    #[serial_test::serial]
    fn test_gitignore_does_not_duplicate_entry_with_trailing_slash() {
        let dir = tempdir().unwrap();
        let gitignore_path = dir.path().join(".gitignore");
        std::fs::write(&gitignore_path, ".rs-guard/cache/\n").unwrap();

        let cache_dir = dir.path().join(DEFAULT_CACHE_DIR);
        let config = CacheConfig {
            cache_dir,
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        let original_dir = std::env::current_dir().unwrap();
        let _restore = RestoreCurrentDir(original_dir);
        std::env::set_current_dir(dir.path()).unwrap();

        let line_count_before = std::fs::read_to_string(&gitignore_path)
            .unwrap()
            .lines()
            .count();

        cache.ensure_gitignored().unwrap();
        let content = std::fs::read_to_string(&gitignore_path).unwrap();
        let line_count_after = content.lines().count();
        assert_eq!(
            line_count_before, line_count_after,
            "entry with trailing slash should not be duplicated: {}",
            content
        );
    }

    #[test]
    fn test_cache_disabled_never_hits() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".rs-guard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: false,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        cache
            .set(
                "test content",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None,
                "cached response",
            )
            .unwrap();
        assert!(cache
            .get(
                "test content",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None
            )
            .is_none());
    }

    #[test]
    fn test_cache_set_get_roundtrip() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".rs-guard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        cache
            .set(
                "diff content",
                "system prompt",
                "deepseek",
                "deepseek-v4-flash",
                None,
                0.1,
                "https://default.example.com",
                None,
                "llm response",
            )
            .unwrap();
        let result = cache.get(
            "diff content",
            "system prompt",
            "deepseek",
            "deepseek-v4-flash",
            None,
            0.1,
            "https://default.example.com",
            None,
        );
        assert_eq!(result, Some("llm response".to_string()));
    }

    #[test]
    fn test_cache_miss_returns_none() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".rs-guard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        assert!(cache
            .get(
                "nonexistent content",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None
            )
            .is_none());
    }

    #[test]
    fn test_cache_entry_expires() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".rs-guard/cache"),
            ttl: Duration::from_secs(0), // Zero TTL = immediately expired
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        cache
            .set(
                "expiring content",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None,
                "will expire",
            )
            .unwrap();

        // Should be expired and return None
        let result = cache.get(
            "expiring content",
            "prompt",
            "deepseek",
            "model",
            None,
            0.1,
            "https://default.example.com",
            None,
        );
        assert!(result.is_none());

        // File should have been deleted
        let key = CacheKey::new(
            "expiring content",
            "prompt",
            "deepseek",
            "model",
            None,
            0.1,
            "https://default.example.com",
            None,
        )
        .as_string();
        assert!(!cache.cache_path(&key).exists());
    }

    #[test]
    fn test_cache_directory_created() {
        let dir = tempdir().unwrap();
        let cache_dir = dir.path().join("custom/cache/path");
        let config = CacheConfig {
            cache_dir: cache_dir.clone(),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };

        DiffCache::new(config).unwrap();
        assert!(cache_dir.exists());
    }

    #[test]
    fn test_cache_set_overwrites_existing() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".rs-guard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        cache
            .set(
                "key",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None,
                "version 1",
            )
            .unwrap();
        cache
            .set(
                "key",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None,
                "version 2",
            )
            .unwrap();

        assert_eq!(
            cache.get(
                "key",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None
            ),
            Some("version 2".to_string())
        );
    }

    #[test]
    fn test_cache_size_limit_enforcement() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".rs-guard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: 100, // Very small limit
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        // Add several entries
        for i in 0..10 {
            cache
                .set(
                    &format!("content {}", i),
                    "prompt",
                    "deepseek",
                    "model",
                    None,
                    0.1,
                    "https://default.example.com",
                    None,
                    &format!("response {}", i),
                )
                .unwrap();
        }

        // Check that we're under the limit
        let stats = cache.stats().unwrap();
        assert!(stats.total_size_bytes <= 100);
    }

    #[test]
    fn test_cache_clear() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".rs-guard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        cache
            .set(
                "key1",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None,
                "value1",
            )
            .unwrap();
        cache
            .set(
                "key2",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None,
                "value2",
            )
            .unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.file_count, 2);

        cache.clear().unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.file_count, 0);
    }

    #[test]
    fn test_cache_stats() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".rs-guard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: 1000,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        cache
            .set(
                "key1",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None,
                "value1",
            )
            .unwrap();
        cache
            .set(
                "key2",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None,
                "value2",
            )
            .unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.file_count, 2);
        assert!(stats.total_size_bytes > 0);
        assert_eq!(stats.max_size_bytes, 1000);
    }

    #[test]
    fn test_cache_multiline_response() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".rs-guard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        let multiline = "line1\nline2\nline3\nline4";
        cache
            .set(
                "key",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None,
                multiline,
            )
            .unwrap();

        assert_eq!(
            cache.get(
                "key",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None
            ),
            Some(multiline.to_string())
        );
    }

    #[test]
    fn test_cache_corrupted_file_no_timestamp() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".rs-guard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        // Write a valid entry first
        cache
            .set(
                "key",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None,
                "response",
            )
            .unwrap();

        // Corrupt the cache file by overwriting with garbage
        let key = CacheKey::new(
            "key",
            "prompt",
            "deepseek",
            "model",
            None,
            0.1,
            "https://default.example.com",
            None,
        )
        .as_string();
        let path = cache.cache_path(&key);
        std::fs::write(&path, "not a valid cache entry").unwrap();

        // Should return None (corrupted file treated as miss)
        assert!(cache
            .get(
                "key",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None
            )
            .is_none());
    }

    #[test]
    fn test_cache_corrupted_file_invalid_timestamp() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".rs-guard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        // Write a valid entry first
        cache
            .set(
                "key2",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None,
                "response",
            )
            .unwrap();

        // Corrupt the timestamp
        let key = CacheKey::new(
            "key2",
            "prompt",
            "deepseek",
            "model",
            None,
            0.1,
            "https://default.example.com",
            None,
        )
        .as_string();
        let path = cache.cache_path(&key);
        std::fs::write(&path, "not-a-number\nresponse body").unwrap();

        // Should return None
        assert!(cache
            .get(
                "key2",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None
            )
            .is_none());
    }

    #[test]
    fn test_cache_corrupted_file_empty() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".rs-guard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        // Write a valid entry first
        cache
            .set(
                "key3",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None,
                "response",
            )
            .unwrap();

        // Empty the cache file
        let key = CacheKey::new(
            "key3",
            "prompt",
            "deepseek",
            "model",
            None,
            0.1,
            "https://default.example.com",
            None,
        )
        .as_string();
        let path = cache.cache_path(&key);
        std::fs::write(&path, "").unwrap();

        // Should return None
        assert!(cache
            .get(
                "key3",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None
            )
            .is_none());
    }

    #[test]
    fn test_cache_corrupted_file_binary_data() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".rs-guard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        // Write a valid entry first
        cache
            .set(
                "key4",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None,
                "response",
            )
            .unwrap();

        // Write binary data
        let key = CacheKey::new(
            "key4",
            "prompt",
            "deepseek",
            "model",
            None,
            0.1,
            "https://default.example.com",
            None,
        )
        .as_string();
        let path = cache.cache_path(&key);
        std::fs::write(&path, [0xFF, 0xFE, 0x00, 0x01, 0x02]).unwrap();

        // Should return None (binary data can't be parsed as UTF-8 timestamp)
        assert!(cache
            .get(
                "key4",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None
            )
            .is_none());
    }

    #[test]
    #[serial_test::serial]
    fn test_find_git_root_in_git_repo() {
        // This test runs inside the rs-guard repo, so git root should be found
        let root = find_git_root();
        assert!(root.is_some(), "should find git root in a git repository");
        let root = root.unwrap();
        assert!(root.exists(), "git root should exist");
        // Should contain .git directory
        assert!(root.join(".git").exists() || root.join(".git").is_symlink());
    }

    #[test]
    #[serial_test::serial]
    fn test_default_cache_dir_uses_git_root() {
        let cache_dir = default_cache_dir();
        // Should end with .rs-guard/cache
        assert!(
            cache_dir.to_string_lossy().ends_with(".rs-guard/cache"),
            "cache dir should end with .rs-guard/cache, got: {:?}",
            cache_dir
        );
    }

    #[test]
    fn test_cache_config_with_custom_dir() {
        let dir = tempdir().unwrap();
        let custom_dir = dir.path().join("custom/cache");
        let config = CacheConfig {
            cache_dir: custom_dir.clone(),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();
        assert!(custom_dir.exists(), "custom cache dir should be created");

        cache
            .set(
                "key",
                "prompt",
                "deepseek",
                "model",
                None,
                0.1,
                "https://default.example.com",
                None,
                "value",
            )
            .unwrap();
        let result = cache.get(
            "key",
            "prompt",
            "deepseek",
            "model",
            None,
            0.1,
            "https://default.example.com",
            None,
        );
        assert_eq!(result, Some("value".to_string()));
    }

    #[test]
    #[serial_test::serial]
    #[cfg(unix)]
    fn test_gitignore_readonly_returns_error() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let cache_dir = dir.path().join(DEFAULT_CACHE_DIR);
        let config = CacheConfig {
            cache_dir: cache_dir.to_path_buf(),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            auto_gitignore: true,
        };
        let cache = DiffCache::new(config).unwrap();

        let original_dir = std::env::current_dir().unwrap();
        let _restore = RestoreCurrentDir(original_dir);
        std::env::set_current_dir(dir.path()).unwrap();

        let gitignore_path = dir.path().join(".gitignore");
        std::fs::write(&gitignore_path, "existing\n").unwrap();
        std::fs::set_permissions(&gitignore_path, std::fs::Permissions::from_mode(0o444)).unwrap();

        let result = cache.ensure_gitignored();
        assert!(result.is_err(), "should fail on read-only .gitignore");

        std::fs::set_permissions(&gitignore_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
}
