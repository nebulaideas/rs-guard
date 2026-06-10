# Framework-Specific Review Prompts

This directory contains custom review prompts tailored to specific frameworks and use cases.

## Available Prompts

- **[react-vite.md](react-vite.md)** — React + Vite projects
  - Focuses on React best practices, TypeScript safety, performance, and Vite configuration
  - Covers hooks, dependency arrays, memoization, bundle size, and security

- **[rails.md](rails.md)** — Ruby on Rails applications
  - Focuses on Rails conventions, database safety, security, and performance
  - Covers ActiveRecord, migrations, background jobs, and testing

- **[general-code-review.md](general-code-review.md)** — General-purpose code review
  - Suitable for any programming language or framework
  - Focuses on correctness, security, error handling, and code quality

## Usage

Copy the appropriate prompt to your repository:

```bash
# For React + Vite
cp examples/prompts/react-vite.md .github/review-prompt.md

# For Rails
cp examples/prompts/rails.md .github/review-prompt.md

# For general use
cp examples/prompts/general-code-review.md .github/review-prompt.md
```

Then customize it to match your project's specific rules and patterns.

## Customizing Prompts

Edit the copied prompt to add:
- Project-specific conventions
- Custom severity guidelines
- Framework-specific focus areas
- Team-specific coding standards

## Integration

The prompt will be automatically used by rs-guard when:
- Running in CI mode (GitHub Actions)
- Running in local mode with pre-commit hooks
- Running with `--prompt-file .github/review-prompt.md`

For pre-commit hook setup with Husky or Lefthook, see [`../local-review/husky-setup.md`](../local-review/husky-setup.md).
