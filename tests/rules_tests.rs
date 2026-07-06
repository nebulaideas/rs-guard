//! Integration tests for the project rules detection module (`src/rules.rs`).
//!
//! Covers: priority ordering, first-match-wins, missing files, soft-cap
//! truncation with warning banner, `.cursor/rules/` glob behavior, and the
//! builder API.

use rs_guard::rules::{
    detect_project_rules, RulesDetector, RulesFilePath, RulesFileSize, DEFAULT_RULES_CAP_BYTES,
};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helper: create a temp repo root with a single rules file
// ---------------------------------------------------------------------------

fn make_repo_with_file(relative_path: &str, content: &str) -> TempDir {
    let dir = TempDir::new().expect("failed to create temp dir");
    let file_path = dir.path().join(relative_path);
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).expect("failed to create parent dirs");
    }
    fs::write(&file_path, content).expect("failed to write rules file");
    dir
}

// ---------------------------------------------------------------------------
// Priority ordering — each slot detected when only that file exists
// ---------------------------------------------------------------------------

#[test]
fn detects_agents_md_when_only_agents_exists() {
    let dir = make_repo_with_file("AGENTS.md", "# Agent rules\n");
    let result = detect_project_rules(dir.path()).expect("detection should succeed");
    let detected = result.expect("AGENTS.md should be detected");
    assert_eq!(detected.path(), Path::new("AGENTS.md"));
    assert_eq!(detected.content(), "# Agent rules\n");
    assert!(!detected.is_truncated());
}

#[test]
fn detects_claude_md_when_only_claude_exists() {
    let dir = make_repo_with_file("CLAUDE.md", "# Claude rules\n");
    let result = detect_project_rules(dir.path()).expect("detection should succeed");
    let detected = result.expect("CLAUDE.md should be detected");
    assert_eq!(detected.path(), Path::new("CLAUDE.md"));
    assert_eq!(detected.content(), "# Claude rules\n");
}

#[test]
fn detects_copilot_instructions_when_only_it_exists() {
    let dir = make_repo_with_file(".github/copilot-instructions.md", "# Copilot rules\n");
    let result = detect_project_rules(dir.path()).expect("detection should succeed");
    let detected = result.expect("copilot-instructions.md should be detected");
    assert_eq!(
        detected.path(),
        Path::new(".github/copilot-instructions.md")
    );
    assert_eq!(detected.content(), "# Copilot rules\n");
}

#[test]
fn detects_gemini_styleguide_when_only_it_exists() {
    let dir = make_repo_with_file(".gemini/styleguide.md", "# Gemini rules\n");
    let result = detect_project_rules(dir.path()).expect("detection should succeed");
    let detected = result.expect("styleguide.md should be detected");
    assert_eq!(detected.path(), Path::new(".gemini/styleguide.md"));
    assert_eq!(detected.content(), "# Gemini rules\n");
}

#[test]
fn detects_windsurfrules_when_only_it_exists() {
    let dir = make_repo_with_file(".windsurfrules", "# Windsurf rules\n");
    let result = detect_project_rules(dir.path()).expect("detection should succeed");
    let detected = result.expect(".windsurfrules should be detected");
    assert_eq!(detected.path(), Path::new(".windsurfrules"));
    assert_eq!(detected.content(), "# Windsurf rules\n");
}

// ---------------------------------------------------------------------------
// .cursor/rules/*.md glob — first file alphabetically
// ---------------------------------------------------------------------------

#[test]
fn detects_cursor_rules_glob_first_alphabetically() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let cursor_dir = dir.path().join(".cursor/rules");
    fs::create_dir_all(&cursor_dir).expect("failed to create .cursor/rules");
    // Create multiple .md files — detection should pick the first alphabetically
    fs::write(cursor_dir.join("zebra.md"), "# Zebra\n").expect("write zebra");
    fs::write(cursor_dir.join("alpha.md"), "# Alpha\n").expect("write alpha");
    fs::write(cursor_dir.join("middle.md"), "# Middle\n").expect("write middle");

    let result = detect_project_rules(dir.path()).expect("detection should succeed");
    let detected = result.expect("a .cursor/rules/*.md file should be detected");
    assert_eq!(
        detected.path(),
        Path::new(".cursor/rules/alpha.md"),
        "should pick first file alphabetically"
    );
    assert_eq!(detected.content(), "# Alpha\n");
}

#[test]
fn cursor_rules_glob_ignores_non_md_files() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let cursor_dir = dir.path().join(".cursor/rules");
    fs::create_dir_all(&cursor_dir).expect("failed to create .cursor/rules");
    // Only non-.md files — should NOT match
    fs::write(cursor_dir.join("rules.txt"), "# Rules\n").expect("write txt");

    let result = detect_project_rules(dir.path()).expect("detection should succeed");
    assert!(
        result.is_none(),
        "non-.md files in .cursor/rules/ should not match"
    );
}

// ---------------------------------------------------------------------------
// First-match-wins when multiple files coexist
// ---------------------------------------------------------------------------

#[test]
fn agents_md_wins_over_claude_md() {
    let dir = TempDir::new().expect("failed to create temp dir");
    fs::write(dir.path().join("AGENTS.md"), "# Agents\n").expect("write agents");
    fs::write(dir.path().join("CLAUDE.md"), "# Claude\n").expect("write claude");

    let result = detect_project_rules(dir.path()).expect("detection should succeed");
    let detected = result.expect("a rules file should be detected");
    assert_eq!(
        detected.path(),
        Path::new("AGENTS.md"),
        "AGENTS.md should take priority over CLAUDE.md"
    );
}

#[test]
fn claude_md_wins_over_copilot_instructions() {
    let dir = TempDir::new().expect("failed to create temp dir");
    fs::write(dir.path().join("CLAUDE.md"), "# Claude\n").expect("write claude");
    fs::create_dir_all(dir.path().join(".github")).expect("create .github");
    fs::write(
        dir.path().join(".github/copilot-instructions.md"),
        "# Copilot\n",
    )
    .expect("write copilot");

    let result = detect_project_rules(dir.path()).expect("detection should succeed");
    let detected = result.expect("a rules file should be detected");
    assert_eq!(
        detected.path(),
        Path::new("CLAUDE.md"),
        "CLAUDE.md should take priority over copilot-instructions.md"
    );
}

#[test]
fn full_priority_chain_agents_first() {
    let dir = TempDir::new().expect("failed to create temp dir");
    // Create ALL priority slots
    fs::write(dir.path().join("AGENTS.md"), "# Agents\n").expect("write agents");
    fs::write(dir.path().join("CLAUDE.md"), "# Claude\n").expect("write claude");
    fs::create_dir_all(dir.path().join(".github")).expect("create .github");
    fs::write(
        dir.path().join(".github/copilot-instructions.md"),
        "# Copilot\n",
    )
    .expect("write copilot");
    fs::create_dir_all(dir.path().join(".gemini")).expect("create .gemini");
    fs::write(dir.path().join(".gemini/styleguide.md"), "# Gemini\n").expect("write gemini");
    let cursor_dir = dir.path().join(".cursor/rules");
    fs::create_dir_all(&cursor_dir).expect("create .cursor/rules");
    fs::write(cursor_dir.join("rules.md"), "# Cursor\n").expect("write cursor");
    fs::write(dir.path().join(".windsurfrules"), "# Windsurf\n").expect("write windsurf");

    let result = detect_project_rules(dir.path()).expect("detection should succeed");
    let detected = result.expect("a rules file should be detected");
    assert_eq!(
        detected.path(),
        Path::new("AGENTS.md"),
        "AGENTS.md should win when all slots are present"
    );
}

// ---------------------------------------------------------------------------
// No rules files found → None
// ---------------------------------------------------------------------------

#[test]
fn returns_none_when_no_rules_files_exist() {
    let dir = TempDir::new().expect("failed to create temp dir");
    // Empty repo — no rules files
    let result = detect_project_rules(dir.path()).expect("detection should succeed");
    assert!(
        result.is_none(),
        "should return None when no rules files exist"
    );
}

// ---------------------------------------------------------------------------
// Soft cap truncation
// ---------------------------------------------------------------------------

#[test]
fn file_under_cap_is_not_truncated() {
    // Content well under 32 KB
    let content = "x".repeat(100);
    let dir = make_repo_with_file("AGENTS.md", &content);

    let result = detect_project_rules(dir.path()).expect("detection should succeed");
    let detected = result.expect("rules file should be detected");
    assert!(
        !detected.is_truncated(),
        "file under cap should not be truncated"
    );
    assert_eq!(detected.content(), &content);
    assert_eq!(detected.original_size(), 100);
}

#[test]
fn file_over_cap_is_truncated_with_warning_banner() {
    // Content over 32 KB
    let content = "x".repeat(DEFAULT_RULES_CAP_BYTES + 1000);
    let dir = make_repo_with_file("AGENTS.md", &content);

    let result = detect_project_rules(dir.path()).expect("detection should succeed");
    let detected = result.expect("rules file should be detected");
    assert!(detected.is_truncated(), "file over cap should be truncated");
    assert_eq!(
        detected.original_size(),
        DEFAULT_RULES_CAP_BYTES + 1000,
        "original_size should reflect the full file size"
    );
    assert!(
        detected.content().len() <= DEFAULT_RULES_CAP_BYTES,
        "truncated content should fit within the cap"
    );
    assert!(
        detected.content().contains("TRUNCATION WARNING"),
        "truncated content should contain a truncation warning banner"
    );
}

#[test]
fn file_exactly_at_cap_is_not_truncated() {
    let content = "x".repeat(DEFAULT_RULES_CAP_BYTES);
    let dir = make_repo_with_file("AGENTS.md", &content);

    let result = detect_project_rules(dir.path()).expect("detection should succeed");
    let detected = result.expect("rules file should be detected");
    assert!(
        !detected.is_truncated(),
        "file exactly at cap should not be truncated"
    );
    assert_eq!(detected.content(), &content);
}

// ---------------------------------------------------------------------------
// Builder API
// ---------------------------------------------------------------------------

#[test]
fn builder_detect_with_custom_cap() {
    let content = "y".repeat(200);
    let dir = make_repo_with_file("AGENTS.md", &content);

    let detector = RulesDetector::builder()
        .repo_root(dir.path().to_path_buf())
        .cap_bytes(100)
        .build()
        .expect("builder should succeed");

    let result = detector.detect().expect("detection should succeed");
    let detected = result.expect("rules file should be detected");
    assert!(
        detected.is_truncated(),
        "file over custom cap should be truncated"
    );
    assert_eq!(detected.original_size(), 200);
}

#[test]
fn builder_requires_repo_root() {
    let result = RulesDetector::builder().build();
    assert!(
        result.is_err(),
        "builder without repo_root should return an error"
    );
}

#[test]
fn builder_default_cap_is_32kb() {
    let detector = RulesDetector::builder()
        .repo_root(std::path::PathBuf::from("."))
        .build()
        .expect("builder should succeed");
    assert_eq!(
        detector.cap_bytes(),
        DEFAULT_RULES_CAP_BYTES,
        "default cap should be 32 KB"
    );
}

// ---------------------------------------------------------------------------
// detect_all_files — returns all matching files in priority order (Phase 2 prep)
// ---------------------------------------------------------------------------

#[test]
fn detect_all_files_returns_priority_order() {
    let dir = TempDir::new().expect("failed to create temp dir");
    fs::write(dir.path().join("AGENTS.md"), "# Agents\n").expect("write agents");
    fs::write(dir.path().join("CLAUDE.md"), "# Claude\n").expect("write claude");
    fs::create_dir_all(dir.path().join(".github")).expect("create .github");
    fs::write(
        dir.path().join(".github/copilot-instructions.md"),
        "# Copilot\n",
    )
    .expect("write copilot");

    let detector = RulesDetector::builder()
        .repo_root(dir.path().to_path_buf())
        .build()
        .expect("builder should succeed");

    let all_files = detector.detect_all_files();
    assert_eq!(all_files.len(), 3, "should find all 3 rules files");
    assert_eq!(all_files[0], RulesFilePath::from("AGENTS.md"));
    assert_eq!(all_files[1], RulesFilePath::from("CLAUDE.md"));
    assert_eq!(
        all_files[2],
        RulesFilePath::from(".github/copilot-instructions.md")
    );
}

#[test]
fn detect_all_files_empty_when_none_exist() {
    let dir = TempDir::new().expect("failed to create temp dir");

    let detector = RulesDetector::builder()
        .repo_root(dir.path().to_path_buf())
        .build()
        .expect("builder should succeed");

    let all_files = detector.detect_all_files();
    assert!(all_files.is_empty(), "should find no rules files");
}

// ---------------------------------------------------------------------------
// Newtype conversions
// ---------------------------------------------------------------------------

#[test]
fn rules_file_path_from_str() {
    let path: RulesFilePath = "AGENTS.md".into();
    assert_eq!(path.as_path(), Path::new("AGENTS.md"));
}

#[test]
fn rules_file_size_display() {
    let size = RulesFileSize::from(1024);
    assert_eq!(format!("{}", size), "1024");
}
