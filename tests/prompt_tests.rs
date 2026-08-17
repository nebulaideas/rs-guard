//! Unit tests for prompt composition with project rules layering.

#[test]
fn test_compose_prompt_with_project_rules_includes_conventions_section() {
    let base_prompt = "You are a code reviewer.";
    let project_rules: Option<&str> = Some("# Project Rules\nUse Rust patterns.");
    let rules_file_path: Option<&str> = Some("AGENTS.md");

    let composed =
        rs_guard::pipeline::compose_prompt(base_prompt, project_rules, rules_file_path, false);

    assert!(
        composed.contains("Project Conventions"),
        "composed prompt should contain 'Project Conventions' section"
    );
    assert!(
        composed.contains("AGENTS.md"),
        "composed prompt should mention the rules file path"
    );
    assert!(
        composed.contains("# Project Rules\nUse Rust patterns."),
        "composed prompt should include the rules content"
    );
    assert!(
        composed.contains("project rules take precedence"),
        "composed prompt should state that project rules take precedence"
    );
}

#[test]
fn test_compose_prompt_without_project_rules_unchanged() {
    let base_prompt = "You are a code reviewer.";
    let project_rules: Option<&str> = None;
    let rules_file_path: Option<&str> = None;

    let composed =
        rs_guard::pipeline::compose_prompt(base_prompt, project_rules, rules_file_path, false);

    assert_eq!(
        composed, base_prompt,
        "prompt should be unchanged when no project rules"
    );
    assert!(
        !composed.contains("Project Conventions"),
        "should not add Project Conventions section"
    );
}

#[test]
fn test_compose_prompt_with_empty_file_path_omits_header() {
    let base_prompt = "You are a code reviewer.";
    let project_rules: Option<&str> = Some("# Project Rules\nUse Rust patterns.");
    let rules_file_path: Option<&str> = Some("");

    let composed =
        rs_guard::pipeline::compose_prompt(base_prompt, project_rules, rules_file_path, false);

    assert!(
        composed.contains("Project Conventions"),
        "should add Project Conventions section"
    );
    assert!(
        !composed.contains("(from )"),
        "should not include empty file path in header"
    );
    assert!(
        composed.contains("# Project Rules\nUse Rust patterns."),
        "should include the rules content"
    );
}

#[test]
fn test_compose_prompt_with_empty_rules_content_returns_base_prompt() {
    let base_prompt = "You are a code reviewer.";
    let project_rules: Option<&str> = Some("");
    let rules_file_path: Option<&str> = Some("AGENTS.md");

    let composed =
        rs_guard::pipeline::compose_prompt(base_prompt, project_rules, rules_file_path, false);

    assert_eq!(
        composed, base_prompt,
        "empty rules content should return base prompt unchanged"
    );
    assert!(
        !composed.contains("Project Conventions"),
        "should not add Project Conventions section for empty rules"
    );
}

#[test]
fn test_compose_prompt_with_none_file_path_omits_header() {
    let base_prompt = "You are a code reviewer.";
    let project_rules: Option<&str> = Some("# Project Rules\nUse Rust patterns.");
    let rules_file_path: Option<&str> = None;

    let composed =
        rs_guard::pipeline::compose_prompt(base_prompt, project_rules, rules_file_path, false);

    assert!(
        composed.contains("Project Conventions"),
        "should add Project Conventions section"
    );
    assert!(
        !composed.contains("(from"),
        "should not include file path when None"
    );
    assert!(
        composed.contains("# Project Rules\nUse Rust patterns."),
        "should include the rules content"
    );
}

#[test]
fn test_compose_prompt_with_custom_prompt_file_and_no_rules_is_backwards_compatible() {
    // Regression: repos that only use `.github/review-prompt.md` (v1.4.0 style)
    // should see exactly that prompt, with no "Project Conventions" section added.
    let custom_prompt = "You are a Rust specialist reviewer focused on unsafe code.";
    let project_rules: Option<&str> = None;
    let rules_file_path: Option<&str> = None;

    let composed =
        rs_guard::pipeline::compose_prompt(custom_prompt, project_rules, rules_file_path, false);

    assert_eq!(
        composed, custom_prompt,
        "custom prompt should be returned unchanged when no project rules are detected"
    );
    assert!(
        !composed.contains("Project Conventions"),
        "should not add Project Conventions section for repos without project rules"
    );
}

#[test]
fn test_compose_prompt_layers_rules_on_top_of_custom_prompt() {
    // New behavior: when project rules are detected, they are appended to a
    // custom prompt file (v1.4.0 style) without replacing it.
    let custom_prompt = "You are a Rust specialist reviewer focused on unsafe code.";
    let project_rules: Option<&str> = Some("# Project Rules\nUse Rust patterns.");
    let rules_file_path: Option<&str> = Some("AGENTS.md");

    let composed =
        rs_guard::pipeline::compose_prompt(custom_prompt, project_rules, rules_file_path, false);

    assert!(
        composed.starts_with(custom_prompt),
        "custom prompt should be preserved at the start of the composed prompt"
    );
    assert!(
        composed.contains("Project Conventions"),
        "should add Project Conventions section when rules are present"
    );
    assert!(
        composed.contains("# Project Rules\nUse Rust patterns."),
        "should include the project rules content"
    );
    assert!(
        composed.contains("project rules take precedence"),
        "should include the precedence statement"
    );
}

// --- Language-aware prompt auto-selection tests ---

#[test]
fn test_auto_select_prompt_rust_diff() {
    let diff = "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1,2 @@\n+x\n";
    let prompt = rs_guard::prompt_select::auto_select_prompt(diff);
    assert!(
        !prompt.is_empty(),
        "auto-selected prompt should not be empty"
    );
    // CLI tooling prompt should differ from the default
    assert_ne!(
        prompt,
        rs_guard::config::DEFAULT_PROMPT,
        "Rust diff should select CLI tooling prompt, not the default"
    );
}

#[test]
fn test_auto_select_prompt_frontend_diff() {
    let diff = "diff --git a/src/App.tsx b/src/App.tsx\n--- a/src/App.tsx\n+++ b/src/App.tsx\n@@ -1 +1,2 @@\n+x\ndiff --git a/src/index.ts b/src/index.ts\n--- a/src/index.ts\n+++ b/src/index.ts\n@@ -1 +1,2 @@\n+y\n";
    let prompt = rs_guard::prompt_select::auto_select_prompt(diff);
    assert_ne!(
        prompt,
        rs_guard::config::DEFAULT_PROMPT,
        "Frontend diff should select frontend prompt, not the default"
    );
}

#[test]
fn test_auto_select_prompt_backend_diff() {
    let diff = "diff --git a/app/models/user.rb b/app/models/user.rb\n--- a/app/models/user.rb\n+++ b/app/models/user.rb\n@@ -1 +1,2 @@\n+x\ndiff --git a/db/schema.rb b/db/schema.rb\n--- a/db/schema.rb\n+++ b/db/schema.rb\n@@ -1 +1,2 @@\n+y\n";
    let prompt = rs_guard::prompt_select::auto_select_prompt(diff);
    assert_ne!(
        prompt,
        rs_guard::config::DEFAULT_PROMPT,
        "Backend diff should select backend prompt, not the default"
    );
}

#[test]
fn test_auto_select_prompt_empty_diff_returns_default() {
    let prompt = rs_guard::prompt_select::auto_select_prompt("");
    assert_eq!(
        prompt,
        rs_guard::config::DEFAULT_PROMPT,
        "Empty diff should fall back to default prompt"
    );
}

#[test]
fn test_auto_select_prompt_unknown_extensions_returns_default() {
    let diff =
        "diff --git a/data.xyz b/data.xyz\n--- a/data.xyz\n+++ b/data.xyz\n@@ -1 +1,2 @@\n+x\n";
    let prompt = rs_guard::prompt_select::auto_select_prompt(diff);
    assert_eq!(
        prompt,
        rs_guard::config::DEFAULT_PROMPT,
        "Unknown extensions should fall back to default prompt"
    );
}

#[test]
fn test_prompt_category_general_returns_default_prompt() {
    assert_eq!(
        rs_guard::prompt_select::PromptCategory::General.prompt(),
        rs_guard::config::DEFAULT_PROMPT,
        "General category should return the built-in DEFAULT_PROMPT"
    );
}

#[test]
fn test_default_extension_map_covers_rust_tsx_rb() {
    let map = rs_guard::prompt_select::default_extension_map();
    assert!(map.contains_key("rs"));
    assert!(map.contains_key("tsx"));
    assert!(map.contains_key("rb"));
    assert!(map.contains_key("go"));
    assert!(map.contains_key("py"));
}

#[test]
fn test_all_prompt_templates_contain_verdict_contract() {
    // Every built-in prompt template must contain the exact verdict metadata
    // block so the verdict parser can extract the review state.
    let categories = [
        rs_guard::prompt_select::PromptCategory::General,
        rs_guard::prompt_select::PromptCategory::BackendApi,
        rs_guard::prompt_select::PromptCategory::FrontendSpa,
        rs_guard::prompt_select::PromptCategory::CliTooling,
    ];
    for cat in categories {
        let prompt = cat.prompt();
        assert!(
            prompt.contains("[RS_GUARD_VERDICT_METADATA]"),
            "Prompt category {:?} must contain [RS_GUARD_VERDICT_METADATA] block",
            cat
        );
        assert!(
            prompt.contains("Verdict:"),
            "Prompt category {:?} must contain 'Verdict:' field",
            cat
        );
        assert!(
            prompt.contains("CriticalIssues:"),
            "Prompt category {:?} must contain 'CriticalIssues:' field",
            cat
        );
        assert!(
            prompt.contains("SecurityIssues:"),
            "Prompt category {:?} must contain 'SecurityIssues:' field",
            cat
        );
        assert!(
            prompt.contains("ImportantIssues:"),
            "Prompt category {:?} must contain 'ImportantIssues:' field",
            cat
        );
        assert!(
            prompt.contains("Suggestions:"),
            "Prompt category {:?} must contain 'Suggestions:' field",
            cat
        );
    }
}

#[test]
fn test_all_prompt_templates_contain_severity_taxonomy() {
    // Every built-in prompt template must reference the severity labels
    // so findings are categorized correctly.
    let categories = [
        rs_guard::prompt_select::PromptCategory::General,
        rs_guard::prompt_select::PromptCategory::BackendApi,
        rs_guard::prompt_select::PromptCategory::FrontendSpa,
        rs_guard::prompt_select::PromptCategory::CliTooling,
    ];
    for cat in categories {
        let prompt = cat.prompt();
        assert!(
            prompt.contains("Critical") || prompt.contains("critical"),
            "Prompt category {:?} must reference Critical severity",
            cat
        );
    }
}
