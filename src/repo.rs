//! Repository-root resolution utilities.
//!
//! Provides helpers for locating the repository root so that rs-guard behaves
//! consistently when invoked from a subdirectory of a Git working tree.

use std::path::PathBuf;

/// Finds the Git working-tree root by running `git rev-parse --show-toplevel`.
///
/// Returns `None` if Git is not installed, not on `PATH`, or if the current
/// directory is not inside a Git repository.
#[must_use]
pub fn find_git_root() -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let root = String::from_utf8(output.stdout).ok()?.trim().to_string();

    if root.is_empty() {
        return None;
    }

    Some(PathBuf::from(root))
}

/// Resolves the repository root to use for file detection and caching.
///
/// First attempts to locate the Git working-tree root via
/// `git rev-parse --show-toplevel`. If Git is unavailable, the current
/// directory is not inside a Git repo, or the command fails for any reason,
/// falls back to the current working directory.
///
/// # Returns
///
/// A [`PathBuf`] containing the resolved repo root.
#[must_use]
pub fn resolve_repo_root() -> PathBuf {
    find_git_root().unwrap_or_else(|| {
        log::debug!("No Git working tree found; falling back to current directory for repo root");
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    })
}

// ---------------------------------------------------------------------------
// Inline unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    /// Restores the process working directory when dropped.
    struct RestoreCurrentDir(PathBuf);

    impl Drop for RestoreCurrentDir {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    #[test]
    #[serial_test::serial]
    fn resolve_repo_root_finds_git_root() {
        let dir = TempDir::new().expect("temp dir");
        let git_dir = dir.path().join("repo");
        fs::create_dir(&git_dir).expect("create repo dir");

        // Initialize a git repo
        Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&git_dir)
            .output()
            .expect("git init should succeed");

        let sub_dir = git_dir.join("src");
        fs::create_dir(&sub_dir).expect("create sub dir");

        // Change into the subdirectory
        let original_dir = std::env::current_dir().unwrap();
        let _restore = RestoreCurrentDir(original_dir);
        std::env::set_current_dir(&sub_dir).unwrap();

        let root = resolve_repo_root();
        assert_eq!(
            root.canonicalize().unwrap_or(root),
            git_dir.canonicalize().unwrap_or(git_dir),
            "resolve_repo_root should return the git working-tree root"
        );
    }

    #[test]
    #[serial_test::serial]
    fn resolve_repo_root_falls_back_to_cwd_when_not_in_git_repo() {
        let dir = TempDir::new().expect("temp dir");
        let sub_dir = dir.path().join("not-a-repo");
        fs::create_dir(&sub_dir).expect("create sub dir");

        let original_dir = std::env::current_dir().unwrap();
        let _restore = RestoreCurrentDir(original_dir);
        std::env::set_current_dir(&sub_dir).unwrap();

        let root = resolve_repo_root();
        assert_eq!(
            root.canonicalize().unwrap_or(root),
            sub_dir.canonicalize().unwrap_or(sub_dir),
            "resolve_repo_root should fall back to current directory outside a git repo"
        );
    }

    #[test]
    #[serial_test::serial]
    fn resolve_repo_root_falls_back_when_git_fails() {
        // A git work-tree without a .git directory (or a corrupted git state)
        // will cause `git rev-parse --show-toplevel` to fail.
        let dir = TempDir::new().expect("temp dir");
        let bad_repo_dir = dir.path().join("bad-repo");
        fs::create_dir(&bad_repo_dir).expect("create bad repo dir");

        // Create a .git file (not directory) to confuse git
        fs::write(bad_repo_dir.join(".git"), "not-a-repo").expect("write fake .git");

        let original_dir = std::env::current_dir().unwrap();
        let _restore = RestoreCurrentDir(original_dir);
        std::env::set_current_dir(&bad_repo_dir).unwrap();

        let root = resolve_repo_root();
        assert_eq!(
            root.canonicalize().unwrap_or(root),
            bad_repo_dir.canonicalize().unwrap_or(bad_repo_dir),
            "resolve_repo_root should fall back to current directory when git fails"
        );
    }
}
