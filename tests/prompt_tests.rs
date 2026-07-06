//! Unit tests for prompt composition with project rules layering.

#[test]
fn test_compose_prompt_with_project_rules_includes_conventions_section() {
    let base_prompt = "You are a code reviewer.";
    let project_rules: Option<&str> = Some("# Project Rules\nUse Rust patterns.");
    let rules_file_path = "AGENTS.md";

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
    let rules_file_path = "";

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
