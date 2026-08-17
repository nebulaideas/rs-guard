//! Language-aware prompt auto-selection.
//!
//! When no explicit `--prompt-file` is provided, rs-guard can inspect the
//! changed file extensions in a diff and select a review prompt template
//! that matches the dominant language or domain (backend, frontend, CLI, etc.).
//!
//! The selection is a *hint* — an explicit prompt file always wins. The
//! mapping from file extensions to [`PromptCategory`] is configurable via
//! `.reviewer.toml`; the built-in defaults cover common languages.

use std::collections::HashMap;

/// A review prompt category backed by a built-in template.
///
/// Each variant maps to a prompt template embedded from `examples/prompts/`.
/// The [`General`](Self::General) variant mirrors the crate's `DEFAULT_PROMPT`
/// and is the fallback when no language signal is strong enough.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PromptCategory {
    /// General-purpose code review (language-agnostic baseline).
    General,
    /// Backend services and APIs (database, migrations, HTTP semantics).
    BackendApi,
    /// Frontend single-page applications (React, Vue, Svelte, Angular).
    FrontendSpa,
    /// CLI tools and systems programs (Rust, Go, C/C++, Python CLIs).
    CliTooling,
}

impl PromptCategory {
    /// Returns the built-in prompt template text for this category.
    pub fn prompt(self) -> &'static str {
        match self {
            Self::General => crate::config::DEFAULT_PROMPT,
            Self::BackendApi => include_str!("../examples/prompts/backend-api.md"),
            Self::FrontendSpa => include_str!("../examples/prompts/frontend-spa.md"),
            Self::CliTooling => include_str!("../examples/prompts/cli-tooling.md"),
        }
    }
}

/// Built-in default mapping from file extension (without leading dot) to
/// [`PromptCategory`].
///
/// Extensions are matched case-insensitively. This map is used when the user
/// has not provided a custom `prompt_language_map` in `.reviewer.toml`.
pub fn default_extension_map() -> HashMap<&'static str, PromptCategory> {
    let mut m = HashMap::new();
    // Frontend
    for ext in [
        "ts", "tsx", "jsx", "vue", "svelte", "astro", "html", "css", "scss", "sass",
    ] {
        m.insert(ext, PromptCategory::FrontendSpa);
    }
    // Backend
    for ext in [
        "rb", "py", "go", "java", "kt", "scala", "php", "sql", "prisma", "graphql",
    ] {
        m.insert(ext, PromptCategory::BackendApi);
    }
    // CLI / systems
    for ext in [
        "rs", "c", "h", "cpp", "cc", "cxx", "hpp", "hxx", "zig", "nim", "sh", "bash",
    ] {
        m.insert(ext, PromptCategory::CliTooling);
    }
    m
}

/// Extracts file paths from `diff --git a/... b/...` headers in a unified diff.
///
/// Returns the `b/` path (the new path) for each file section.
fn extract_diff_paths(diff: &str) -> Vec<String> {
    diff.lines()
        .filter(|line| line.starts_with("diff --git "))
        .filter_map(|line| {
            let rest = line.strip_prefix("diff --git ")?;
            // Parse "a/path b/path" — prefer b/ path
            let mut b_path = None;
            for tok in rest.split_whitespace() {
                if let Some(p) = tok.strip_prefix("b/") {
                    b_path = Some(p.to_string());
                }
            }
            // Fallback: if no b/ found, try a/ path
            if b_path.is_none() {
                for tok in rest.split_whitespace() {
                    if let Some(p) = tok.strip_prefix("a/") {
                        b_path = Some(p.to_string());
                        break;
                    }
                }
            }
            b_path
        })
        .collect()
}

/// Extracts the file extension (lowercase, without leading dot) from a path.
fn file_extension(path: &str) -> Option<String> {
    let basename = path.rsplit('/').next()?;
    let dot_idx = basename.rfind('.')?;
    if dot_idx == 0 {
        // Hidden file like ".gitignore" — no extension
        return None;
    }
    let ext = &basename[dot_idx + 1..];
    if ext.is_empty() {
        return None;
    }
    Some(ext.to_lowercase())
}

/// Detects the dominant [`PromptCategory`] from diff content by counting
/// file extensions and mapping them via the provided extension map.
///
/// Returns [`PromptCategory::General`] when the diff is empty, no recognized
/// extensions are found, or no single category dominates.
///
/// # Arguments
///
/// * `diff` — The unified diff content to analyze.
/// * `ext_map` — A mapping from lowercase file extension to category.
pub fn detect_prompt_category(
    diff: &str,
    ext_map: &HashMap<&str, PromptCategory>,
) -> PromptCategory {
    let paths = extract_diff_paths(diff);
    if paths.is_empty() {
        return PromptCategory::General;
    }

    let mut counts: HashMap<PromptCategory, usize> = HashMap::new();
    for path in &paths {
        if let Some(ext) = file_extension(path) {
            if let Some(&cat) = ext_map.get(ext.as_str()) {
                *counts.entry(cat).or_insert(0) += 1;
            }
        }
    }

    if counts.is_empty() {
        return PromptCategory::General;
    }

    // Pick the category with the highest count; ties broken by priority order
    counts
        .into_iter()
        .max_by_key(|(cat, count)| {
            (
                *count,
                match cat {
                    PromptCategory::CliTooling => 3,
                    PromptCategory::BackendApi => 2,
                    PromptCategory::FrontendSpa => 1,
                    PromptCategory::General => 0,
                },
            )
        })
        .map(|(cat, _)| cat)
        .unwrap_or(PromptCategory::General)
}

/// Selects a prompt for the given diff, using the default extension map.
///
/// Convenience wrapper around [`detect_prompt_category`] with the built-in
/// default extension mapping. Returns the prompt text for the detected
/// category.
pub fn auto_select_prompt(diff: &str) -> &'static str {
    let map = default_extension_map();
    detect_prompt_category(diff, &map).prompt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn cat(c: PromptCategory) -> PromptCategory {
        c
    }

    #[test]
    fn test_file_extension_basic() {
        assert_eq!(file_extension("src/main.rs"), Some("rs".to_string()));
        assert_eq!(file_extension("app/index.tsx"), Some("tsx".to_string()));
        assert_eq!(file_extension("README.md"), Some("md".to_string()));
    }

    #[test]
    fn test_file_extension_no_extension() {
        assert_eq!(file_extension("Makefile"), None);
        assert_eq!(file_extension(".gitignore"), None);
        assert_eq!(file_extension("src/lib"), None);
    }

    #[test]
    fn test_file_extension_case_insensitive() {
        assert_eq!(file_extension("App.TSX"), Some("tsx".to_string()));
        assert_eq!(file_extension("main.RS"), Some("rs".to_string()));
    }

    #[test]
    fn test_extract_diff_paths_basic() {
        let diff = "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1,2 @@\n+x\ndiff --git a/lib.ts b/lib.ts\n--- a/lib.ts\n+++ b/lib.ts\n@@ -1 +1,2 @@\n+y\n";
        let paths = extract_diff_paths(diff);
        assert_eq!(paths, vec!["src/main.rs", "lib.ts"]);
    }

    #[test]
    fn test_extract_diff_paths_empty_diff() {
        assert!(extract_diff_paths("").is_empty());
        assert!(extract_diff_paths("no diff here\njust text").is_empty());
    }

    #[test]
    fn test_extract_diff_paths_renamed_file() {
        let diff = "diff --git a/old.rs b/new.rs\n--- a/old.rs\n+++ b/new.rs\n@@ -1 +1,2 @@\n+x\n";
        let paths = extract_diff_paths(diff);
        assert_eq!(paths, vec!["new.rs"]);
    }

    #[test]
    fn test_detect_prompt_category_rust_dominates() {
        let diff = "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1,2 @@\n+x\ndiff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1,2 @@\n+y\n";
        let map = default_extension_map();
        assert_eq!(
            detect_prompt_category(diff, &map),
            cat(PromptCategory::CliTooling)
        );
    }

    #[test]
    fn test_detect_prompt_category_frontend_dominates() {
        let diff = "diff --git a/src/App.tsx b/src/App.tsx\n--- a/src/App.tsx\n+++ b/src/App.tsx\n@@ -1 +1,2 @@\n+x\ndiff --git a/src/index.ts b/src/index.ts\n--- a/src/index.ts\n+++ b/src/index.ts\n@@ -1 +1,2 @@\n+y\ndiff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1,2 @@\n+z\n";
        let map = default_extension_map();
        assert_eq!(
            detect_prompt_category(diff, &map),
            cat(PromptCategory::FrontendSpa)
        );
    }

    #[test]
    fn test_detect_prompt_category_backend_dominates() {
        let diff = "diff --git a/app/models/user.rb b/app/models/user.rb\n--- a/app/models/user.rb\n+++ b/app/models/user.rb\n@@ -1 +1,2 @@\n+x\ndiff --git a/db/migrate/001.rb b/db/migrate/001.rb\n--- a/db/migrate/001.rb\n+++ b/db/migrate/001.rb\n@@ -1 +1,2 @@\n+y\n";
        let map = default_extension_map();
        assert_eq!(
            detect_prompt_category(diff, &map),
            cat(PromptCategory::BackendApi)
        );
    }

    #[test]
    fn test_detect_prompt_category_empty_diff_returns_general() {
        let map = default_extension_map();
        assert_eq!(
            detect_prompt_category("", &map),
            cat(PromptCategory::General)
        );
    }

    #[test]
    fn test_detect_prompt_category_unknown_extensions_returns_general() {
        let diff =
            "diff --git a/data.xyz b/data.xyz\n--- a/data.xyz\n+++ b/data.xyz\n@@ -1 +1,2 @@\n+x\n";
        let map = default_extension_map();
        assert_eq!(
            detect_prompt_category(diff, &map),
            cat(PromptCategory::General)
        );
    }

    #[test]
    fn test_detect_prompt_category_tie_breaks_by_priority() {
        // One frontend + one backend + one CLI — CLI wins on tie-break
        let diff = "diff --git a/app.tsx b/app.tsx\n--- a/app.tsx\n+++ b/app.tsx\n@@ -1 +1,2 @@\n+x\ndiff --git a/api.rb b/api.rb\n--- a/api.rb\n+++ b/api.rb\n@@ -1 +1,2 @@\n+y\ndiff --git a/main.rs b/main.rs\n--- a/main.rs\n+++ b/main.rs\n@@ -1 +1,2 @@\n+z\n";
        let map = default_extension_map();
        assert_eq!(
            detect_prompt_category(diff, &map),
            cat(PromptCategory::CliTooling)
        );
    }

    #[test]
    fn test_detect_prompt_category_majority_wins_over_tie_break() {
        // 2 frontend + 1 CLI — frontend wins by count
        let diff = "diff --git a/app.tsx b/app.tsx\n--- a/app.tsx\n+++ b/app.tsx\n@@ -1 +1,2 @@\n+x\ndiff --git a/index.ts b/index.ts\n--- a/index.ts\n+++ b/index.ts\n@@ -1 +1,2 @@\n+y\ndiff --git a/main.rs b/main.rs\n--- a/main.rs\n+++ b/main.rs\n@@ -1 +1,2 @@\n+z\n";
        let map = default_extension_map();
        assert_eq!(
            detect_prompt_category(diff, &map),
            cat(PromptCategory::FrontendSpa)
        );
    }

    #[test]
    fn test_prompt_for_category_returns_nonempty() {
        assert!(!PromptCategory::General.prompt().is_empty());
        assert!(!PromptCategory::BackendApi.prompt().is_empty());
        assert!(!PromptCategory::FrontendSpa.prompt().is_empty());
        assert!(!PromptCategory::CliTooling.prompt().is_empty());
    }

    #[test]
    fn test_auto_select_prompt_rust() {
        let diff = "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1,2 @@\n+x\n";
        let prompt = auto_select_prompt(diff);
        assert!(!prompt.is_empty());
        // CLI tooling prompt should mention CLI or systems or panic/unwrap
        assert!(
            prompt.to_lowercase().contains("cli")
                || prompt.to_lowercase().contains("system")
                || prompt.to_lowercase().contains("panic")
                || prompt.to_lowercase().contains("unwrap"),
            "CLI prompt should reference CLI/systems concepts"
        );
    }

    #[test]
    fn test_auto_select_prompt_empty_returns_general() {
        let prompt = auto_select_prompt("");
        assert!(!prompt.is_empty());
    }

    #[test]
    fn test_default_extension_map_covers_common_languages() {
        let map = default_extension_map();
        assert_eq!(map.get("rs"), Some(&PromptCategory::CliTooling));
        assert_eq!(map.get("tsx"), Some(&PromptCategory::FrontendSpa));
        assert_eq!(map.get("rb"), Some(&PromptCategory::BackendApi));
        assert_eq!(map.get("py"), Some(&PromptCategory::BackendApi));
        assert_eq!(map.get("go"), Some(&PromptCategory::BackendApi));
    }

    #[test]
    fn test_detect_prompt_category_custom_map() {
        let mut custom = HashMap::new();
        custom.insert("xyz", PromptCategory::BackendApi);
        let diff =
            "diff --git a/data.xyz b/data.xyz\n--- a/data.xyz\n+++ b/data.xyz\n@@ -1 +1,2 @@\n+x\n";
        assert_eq!(
            detect_prompt_category(diff, &custom),
            cat(PromptCategory::BackendApi)
        );
    }
}
