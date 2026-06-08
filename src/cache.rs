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
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
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
        self.config.cache_dir.join(format!("{}.{}", key, CACHE_FILE_EXT))
    }

    /// Ensures the cache directory exists, creating it if necessary.
    fn ensure_cache_dir(&self) -> Result<(), DiffguardError> {
        fs::create_dir_all(&self.config.cache_dir).map_err(|e| {
            DiffguardError::Config(format!("Failed to create cache dir: {}", e))
        })
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
    /// * `content` — Diff content to hash and look up.
    pub fn get(&self, content: &str) -> Option<String> {
        if !self.config.enabled {
            return None;
        }

        let key = diff_hash(content);
        let path = self.cache_path(&key);

        if !path.exists() {
            return None;
        }

        match self.read_entry(&path) {
            Some(response) => {
                log::debug!("Cache hit for diff hash: {}", key);
                Some(response)
            }
            None => {
                log::debug!("Cache miss or expired entry for diff hash: {}", key);
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
    /// * `content` — Diff content to hash and key by.
    /// * `response` — The LLM response text to cache.
    ///
    /// # Errors
    ///
    /// Returns [`DiffguardError::Io`] if the file cannot be written.
    pub fn set(&self, content: &str, response: &str) -> Result<(), DiffguardError> {
        if !self.config.enabled {
            return Ok(());
        }

        let key = diff_hash(content);
        let path = self.cache_path(&key);

        // Write to temp file in same directory, then atomically rename
        let tmp_path = path.with_extension("tmp");
        {
            let mut tmp = fs::File::create(&tmp_path)?;

            // Write timestamp as first line
            writeln!(tmp, "{}", Self::now_secs())?;

            // Write response
            tmp.write_all(response.as_bytes())?;
            tmp.sync_all()?;
        }

        fs::rename(&tmp_path, &path)?;

        log::debug!("Cached response for diff hash: {}", key);

        // Check size limit and cleanup if needed
        self.enforce_size_limit()?;

        Ok(())
    }

    /// Calculates the total size of all cache files.
    fn total_size(&self) -> Result<u64, DiffguardError> {
        let mut total = 0u64;

        let entries = fs::read_dir(&self.config.cache_dir).map_err(|e| {
            DiffguardError::Io(std::io::Error::other(format!("Failed to read cache dir: {}", e)))
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

        // Collect all cache files with their modification times
        let mut files: Vec<(PathBuf, SystemTime)> = Vec::new();

        let entries = fs::read_dir(&self.config.cache_dir).map_err(|e| {
            DiffguardError::Io(std::io::Error::other(format!("Failed to read cache dir: {}", e)))
        })?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some(CACHE_FILE_EXT) {
                if let Ok(metadata) = entry.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        files.push((path, modified));
                    }
                }
            }
        }

        // Sort by modification time (oldest first)
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

        // Check if entry already exists
        match fs::read_to_string(gitignore_path) {
            Ok(content) => {
                if content.contains(&entry) || content.contains(DEFAULT_CACHE_DIR) {
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
            DiffguardError::Io(std::io::Error::other(format!("Failed to read cache dir: {}", e)))
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
            DiffguardError::Io(std::io::Error::other(format!("Failed to read cache dir: {}", e)))
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
    fn test_cache_disabled_never_hits() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().join(".diffguard/cache"),
            ttl: Duration::from_secs(3600),
            enabled: false,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
        };
        let cache = DiffCache::new(config).unwrap();

        cache.set("test content", "cached response").unwrap();
        assert!(cache.get("test content").is_none());
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

        cache.set("diff content", "llm response").unwrap();
        let result = cache.get("diff content");
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

        assert!(cache.get("nonexistent content").is_none());
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

        cache.set("expiring content", "will expire").unwrap();

        // Should be expired and return None
        let result = cache.get("expiring content");
        assert!(result.is_none());

        // File should have been deleted
        let key = diff_hash("expiring content");
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

        cache.set("key", "version 1").unwrap();
        cache.set("key", "version 2").unwrap();

        assert_eq!(cache.get("key"), Some("version 2".to_string()));
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
                .set(&format!("content {}", i), &format!("response {}", i))
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

        cache.set("key1", "value1").unwrap();
        cache.set("key2", "value2").unwrap();

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

        cache.set("key1", "value1").unwrap();
        cache.set("key2", "value2").unwrap();

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
        cache.set("key", multiline).unwrap();

        assert_eq!(cache.get("key"), Some(multiline.to_string()));
    }
}
