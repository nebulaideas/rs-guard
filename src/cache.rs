//! Response caching for LLM results.
//!
//! Caches LLM responses by SHA-256 diff hash to avoid redundant API calls.
//! Cache location: `.diffguard/cache/` (project-local).
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

use crate::error::DiffguardError;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Default cache directory relative to project root.
const DEFAULT_CACHE_DIR: &str = ".diffguard/cache";

/// Default cache TTL: 24 hours.
const DEFAULT_TTL_SECS: u64 = 86400;

/// Default cache size limit: 100MB.
const DEFAULT_MAX_SIZE_BYTES: u64 = 100 * 1024 * 1024;

/// Cache entry file extension.
const CACHE_FILE_EXT: &str = "cache";

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
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::from(DEFAULT_CACHE_DIR),
            ttl: Duration::from_secs(DEFAULT_TTL_SECS),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
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
    /// Sampling temperature.
    temperature: f32,
}

impl CacheKey {
    /// Creates a new cache key from the given components.
    fn new(
        diff_content: &str,
        prompt: &str,
        provider: &str,
        model: &str,
        temperature: f32,
    ) -> Self {
        let diff_hash = hash_content(diff_content);
        let prompt_hash = hash_content(prompt);
        Self {
            diff_hash,
            prompt_hash,
            provider: provider.to_string(),
            model: model.to_string(),
            temperature,
        }
    }

    /// Returns the cache key as a hex string.
    fn as_string(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.diff_hash.as_bytes());
        hasher.update(self.prompt_hash.as_bytes());
        hasher.update(self.provider.as_bytes());
        hasher.update(self.model.as_bytes());
        hasher.update(self.temperature.to_le_bytes());
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
/// use diffguard::cache::diff_hash;
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
    /// Returns [`DiffguardError::Config`] if the cache directory cannot be created.
    pub fn new(config: CacheConfig) -> Result<Self, DiffguardError> {
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
    fn ensure_cache_dir(&self) -> Result<(), DiffguardError> {
        fs::create_dir_all(&self.config.cache_dir)
            .map_err(|e| DiffguardError::Config(format!("Failed to create cache dir: {}", e)))
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
        let mut lines = content.lines();

        // First line is the timestamp
        let timestamp_str = lines.next()?;
        let timestamp: u64 = timestamp_str.parse().ok()?;

        // Check TTL (use >= so TTL=0 means immediately expired)
        let now = Self::now_secs();
        let age = now.saturating_sub(timestamp);
        if age >= self.config.ttl.as_secs() {
            // Entry expired - remove it
            let _ = fs::remove_file(path);
            return None;
        }

        // Rest is the response
        let response: String = lines.collect::<Vec<_>>().join("\n");
        if response.is_empty() {
            return None;
        }

        Some(response)
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
    /// * `temperature` — Sampling temperature.
    pub fn get(
        &self,
        diff_content: &str,
        prompt: &str,
        provider: &str,
        model: &str,
        temperature: f32,
    ) -> Option<String> {
        if !self.config.enabled {
            return None;
        }

        let key = CacheKey::new(diff_content, prompt, provider, model, temperature);
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
    /// * `temperature` — Sampling temperature.
    /// * `response` — The LLM response text to cache.
    ///
    /// # Errors
    ///
    /// Returns [`DiffguardError::Io`] if the file cannot be written.
    pub fn set(
        &self,
        diff_content: &str,
        prompt: &str,
        provider: &str,
        model: &str,
        temperature: f32,
        response: &str,
    ) -> Result<(), DiffguardError> {
        if !self.config.enabled {
            return Ok(());
        }

        let key = CacheKey::new(diff_content, prompt, provider, model, temperature);
        let key_str = key.as_string();
        let path = self.cache_path(&key_str);

        // Write to temp file with unique name in same directory, then atomically rename
        // Use timestamp + random component for uniqueness to prevent symlink attacks
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let tmp_filename = format!("{}.{}.tmp", key_str, timestamp);
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
    fn total_size(&self) -> Result<u64, DiffguardError> {
        let mut total = 0u64;

        let entries = fs::read_dir(&self.config.cache_dir).map_err(|e| {
            DiffguardError::Io(std::io::Error::other(format!(
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
    fn enforce_size_limit(&self) -> Result<(), DiffguardError> {
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
            DiffguardError::Io(std::io::Error::other(format!(
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
    /// Adds `.diffguard/cache/` to the project's `.gitignore` if the file
    /// does not already contain an entry for the cache directory.
    ///
    /// Logs a warning if the operation fails.
    pub fn ensure_gitignored(&self) {
        if !self.config.enabled {
            return;
        }

        let gitignore_path = Path::new(".gitignore");
        let entry = format!("{}/\n", DEFAULT_CACHE_DIR);

        // Check if entry already exists using exact line matching
        match fs::read_to_string(gitignore_path) {
            Ok(content) => {
                // Check for exact line match (with or without trailing slash)
                let has_entry = content.lines().any(|line| {
                    line == DEFAULT_CACHE_DIR || line == format!("{}/", DEFAULT_CACHE_DIR)
                });
                if has_entry {
                    return;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // .gitignore doesn't exist, will create
            }
            Err(e) => {
                log::warn!("Failed to read .gitignore: {}", e);
                return;
            }
        }

        // Append entry
        match fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(gitignore_path)
        {
            Ok(mut f) => {
                if let Err(e) = f.write_all(entry.as_bytes()) {
                    log::warn!("Failed to write to .gitignore: {}", e);
                }
            }
            Err(e) => {
                log::warn!("Failed to open .gitignore for writing: {}", e);
            }
        }
    }

    /// Clears all cache entries.
    ///
    /// # Errors
    ///
    /// Returns [`DiffguardError::Io`] if the cache directory cannot be read
    /// or files cannot be removed.
    pub fn clear(&self) -> Result<(), DiffguardError> {
        let entries = fs::read_dir(&self.config.cache_dir).map_err(|e| {
            DiffguardError::Io(std::io::Error::other(format!(
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
    pub fn stats(&self) -> Result<CacheStats, DiffguardError> {
        let mut file_count = 0u64;
        let mut total_size = 0u64;

        let entries = fs::read_dir(&self.config.cache_dir).map_err(|e| {
            DiffguardError::Io(std::io::Error::other(format!(
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
        let key1 = CacheKey::new("diff", "prompt", "deepseek", "model", 0.1);
        let key2 = CacheKey::new("diff", "prompt", "deepseek", "model", 0.1);
        let key3 = CacheKey::new("diff", "prompt", "deepseek", "model", 0.2);
        let key4 = CacheKey::new("diff", "prompt", "openai", "model", 0.1);

        assert_eq!(key1.as_string(), key2.as_string());
        assert_ne!(key1.as_string(), key3.as_string());
        assert_ne!(key1.as_string(), key4.as_string());
    }

    #[test]
    fn test_gitignore_auto_creation() {
        let dir = tempdir().unwrap();
        let cache_dir = dir.path().join(".diffguard/cache");
        let config = CacheConfig {
            cache_dir: cache_dir.clone(),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
        };
        let cache = DiffCache::new(config).unwrap();

        // Change to the temp directory for the test
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        // First call should create .gitignore
        cache.ensure_gitignored();
        let gitignore_path = dir.path().join(".gitignore");
        assert!(gitignore_path.exists());
        let content = std::fs::read_to_string(&gitignore_path).unwrap();
        assert!(content.contains(DEFAULT_CACHE_DIR));
        let line_count_before = content.lines().count();

        // Second call should not duplicate the entry
        cache.ensure_gitignored();
        let content_after = std::fs::read_to_string(&gitignore_path).unwrap();
        let line_count_after = content_after.lines().count();
        // Line count should be the same (no new lines added)
        assert_eq!(line_count_before, line_count_after);

        // Restore original directory
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_gitignore_exact_line_matching() {
        let dir = tempdir().unwrap();
        let cache_dir = dir.path().join(".diffguard/cache");
        let config = CacheConfig {
            cache_dir: cache_dir.clone(),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
        };
        let cache = DiffCache::new(config).unwrap();

        // Change to the temp directory for the test
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        // Create .gitignore with a similar but different path
        let gitignore_path = dir.path().join(".gitignore");
        std::fs::write(&gitignore_path, ".diffguard/cache2/").unwrap();

        // Should add the entry since it's not an exact match
        cache.ensure_gitignored();
        let content = std::fs::read_to_string(&gitignore_path).unwrap();
        assert!(content.contains(DEFAULT_CACHE_DIR));

        // Restore original directory
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_cache_disabled_never_hits() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".diffguard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: false,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
        };
        let cache = DiffCache::new(config).unwrap();

        cache
            .set(
                "test content",
                "prompt",
                "deepseek",
                "model",
                0.1,
                "cached response",
            )
            .unwrap();
        assert!(cache
            .get("test content", "prompt", "deepseek", "model", 0.1)
            .is_none());
    }

    #[test]
    fn test_cache_set_get_roundtrip() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".diffguard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
        };
        let cache = DiffCache::new(config).unwrap();

        cache
            .set(
                "diff content",
                "system prompt",
                "deepseek",
                "deepseek-v4-flash",
                0.1,
                "llm response",
            )
            .unwrap();
        let result = cache.get(
            "diff content",
            "system prompt",
            "deepseek",
            "deepseek-v4-flash",
            0.1,
        );
        assert_eq!(result, Some("llm response".to_string()));
    }

    #[test]
    fn test_cache_miss_returns_none() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".diffguard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
        };
        let cache = DiffCache::new(config).unwrap();

        assert!(cache
            .get("nonexistent content", "prompt", "deepseek", "model", 0.1)
            .is_none());
    }

    #[test]
    fn test_cache_entry_expires() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".diffguard/cache"),
            ttl: Duration::from_secs(0), // Zero TTL = immediately expired
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
        };
        let cache = DiffCache::new(config).unwrap();

        cache
            .set(
                "expiring content",
                "prompt",
                "deepseek",
                "model",
                0.1,
                "will expire",
            )
            .unwrap();

        // Should be expired and return None
        let result = cache.get("expiring content", "prompt", "deepseek", "model", 0.1);
        assert!(result.is_none());

        // File should have been deleted
        let key = CacheKey::new("expiring content", "prompt", "deepseek", "model", 0.1).as_string();
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
        };

        DiffCache::new(config).unwrap();
        assert!(cache_dir.exists());
    }

    #[test]
    fn test_cache_set_overwrites_existing() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".diffguard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
        };
        let cache = DiffCache::new(config).unwrap();

        cache
            .set("key", "prompt", "deepseek", "model", 0.1, "version 1")
            .unwrap();
        cache
            .set("key", "prompt", "deepseek", "model", 0.1, "version 2")
            .unwrap();

        assert_eq!(
            cache.get("key", "prompt", "deepseek", "model", 0.1),
            Some("version 2".to_string())
        );
    }

    #[test]
    fn test_cache_size_limit_enforcement() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".diffguard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: 100, // Very small limit
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
                    0.1,
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
            cache_dir: dir.path().join(".diffguard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
        };
        let cache = DiffCache::new(config).unwrap();

        cache
            .set("key1", "prompt", "deepseek", "model", 0.1, "value1")
            .unwrap();
        cache
            .set("key2", "prompt", "deepseek", "model", 0.1, "value2")
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
            cache_dir: dir.path().join(".diffguard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: 1000,
        };
        let cache = DiffCache::new(config).unwrap();

        cache
            .set("key1", "prompt", "deepseek", "model", 0.1, "value1")
            .unwrap();
        cache
            .set("key2", "prompt", "deepseek", "model", 0.1, "value2")
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
            cache_dir: dir.path().join(".diffguard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: true,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
        };
        let cache = DiffCache::new(config).unwrap();

        let multiline = "line1\nline2\nline3\nline4";
        cache
            .set("key", "prompt", "deepseek", "model", 0.1, multiline)
            .unwrap();

        assert_eq!(
            cache.get("key", "prompt", "deepseek", "model", 0.1),
            Some(multiline.to_string())
        );
    }
}
