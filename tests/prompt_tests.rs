//! Unit tests for prompt composition with project rules layering.

#[test]
fn test_compose_prompt_with_project_rules_includes_conventions_section() {
    let base_prompt = "You are a code reviewer.";
    let project_rules: Option<&str> = Some("# Project Rules\nUse Rust patterns.");
    let rules_file_path: Option<&str> = Some("AGENTS.md");

    let composed = rs_guard::pipeline::compose_prompt(base_prompt, project_rules, rules_file_path);

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

    let composed = rs_guard::pipeline::compose_prompt(base_prompt, project_rules, rules_file_path);

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

    let composed = rs_guard::pipeline::compose_prompt(base_prompt, project_rules, rules_file_path);

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

    let composed = rs_guard::pipeline::compose_prompt(base_prompt, project_rules, rules_file_path);

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

    let composed = rs_guard::pipeline::compose_prompt(base_prompt, project_rules, rules_file_path);

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
        rs_guard::pipeline::compose_prompt(custom_prompt, project_rules, rules_file_path);

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
        rs_guard::pipeline::compose_prompt(custom_prompt, project_rules, rules_file_path);

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
