# Installation Guide

Complete guide for installing and running rs-guard locally or in GitHub Actions.

---

## Table of Contents

- [Quick Start](#quick-start)
- [Installation Methods](#installation-methods)
  - [Option 1: Cargo Install (Recommended)](#option-1-cargo-install-recommended)
  - [Option 2: Build from Source](#option-2-build-from-source)
- [Platform-Specific Instructions](#platform-specific-instructions)
  - [Linux](#linux)
  - [macOS](#macos)
  - [Windows](#windows)
- [Verification](#verification)
- [GitHub Actions Setup](#github-actions-setup)
- [Local Testing Setup](#local-testing-setup)
- [Troubleshooting Installation](#troubleshooting-installation)

---

## Quick Start

**For immediate testing:**

```bash
# Install via cargo (requires Rust toolchain)
cargo install rs-guard --locked

# Set your API key
export DEEPSEEK_API_KEY="your-api-key"

# Test locally
rs-guard --help
```

---

## Installation Methods

### Option 1: Cargo Install (Recommended)

Simplest method — downloads and builds from crates.io.

```bash
cargo install rs-guard --locked
```

This installs the `rs-guard` binary to `~/.cargo/bin/rs-guard`. Requires Rust
1.92+ and a C compiler (for aws-lc-rs, the TLS library). First build takes
2-5 minutes; subsequent installs are cached.

#### Prerequisites

Install Rust (requires version 1.92+):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

On Linux, you also need a C compiler and cmake (for aws-lc-rs):

```bash
sudo apt-get install -y build-essential cmake  # Debian/Ubuntu
sudo yum install -y gcc cmake                   # RHEL/CentOS
```

On macOS, Xcode Command Line Tools provide everything needed:

```bash
xcode-select --install
```

#### Verify

```bash
rs-guard --version
```

---

### Option 2: Build from Source

Requires Rust toolchain. Use this if you need a custom build or want to contribute.

#### Prerequisites

Install Rust (requires version 1.92+):

```bash
# Linux/macOS
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Verify installation
rustc --version  # Should be 1.92 or higher
cargo --version
```

#### Build Steps

```bash
# Clone the repository
git clone https://github.com/nebulaideas/rs-guard.git
cd rs-guard

# Checkout the desired branch/tag
git checkout main  # or specific version tag

# Build release binary (optimized)
cargo build --release

# Binary location:
# Linux/macOS: ./target/release/rs-guard
# Windows: .\target\release\rs-guard.exe

# Optional: Install to system path
cargo install --path .
```

#### Build Time Expectations

- **First build:** 2-5 minutes (downloads and compiles all dependencies)
- **Subsequent builds:** 30-60 seconds (incremental compilation)
- **Release build:** Additional 1-2 minutes for optimizations

---

## Platform-Specific Instructions

### Linux

#### Add to PATH (if not using sudo)

```bash
# Add to your shell config (~/.bashrc, ~/.zshrc, etc.)
export PATH="$HOME/.cargo/bin:$PATH"

# Or if you downloaded the binary manually
export PATH="$HOME/bin:$PATH"
```

#### System Dependencies

No additional dependencies required. The binary is statically linked.

### macOS

#### Gatekeeper Warning

If you see "rs-guard cannot be opened because the developer cannot be verified":

```bash
# Option 1: Remove quarantine attribute
xattr -d com.apple.quarantine /usr/local/bin/rs-guard

# Option 2: Use sudo to move to a trusted location
sudo mv ~/Downloads/rs-guard /usr/local/bin/
```

#### Apple Silicon Performance

Native ARM64 binaries run 20-30% faster on M1/M2/M3/M4/M5 chips compared to Rosetta translation.

### Windows

#### Add to PATH

1. Open **System Properties** → **Environment Variables**
2. Under **User variables**, select `Path` → **Edit**
3. Add: `C:\Users\<YourUsername>\.cargo\bin`
4. Click **OK** and restart your terminal

Alternatively, add to PowerShell profile (`$PROFILE`):

```powershell
$env:Path += ";$env:USERPROFILE\.cargo\bin"
```

#### Windows Defender

If flagged as unknown software:

- Right-click → **Properties** → Check **Unblock**
- Or add an exclusion in Windows Security settings

---

## Verification

After installation, verify the binary works:

```bash
# Check version
rs-guard --version

# Display help
rs-guard --help

# Test with a simple command (should show help, not error)
rs-guard
```

Expected output:

```
rs-guard 1.0.2
AI-powered code review CLI for GitHub PRs

Usage: rs-guard [OPTIONS]
...
```

---

## GitHub Actions Setup

### Minimal Workflow

Create `.github/workflows/ai-review.yml`:

```yaml
name: AI Code Review

on:
  pull_request:
    types: [opened, synchronize]

permissions:
  pull-requests: write
  contents: read

jobs:
  review:
    runs-on: ubuntu-latest
    if: ${{ !github.event.pull_request.head.repo.fork }}

    steps:
      # Pinned from actions/checkout@v5 (93cb6efe) to avoid Node.js 20 deprecation.
      - uses: actions/checkout@93cb6efe18208431cddfb8368fd83d5badbf9bfd

      - uses: dtolnay/rust-toolchain@e97e2d8cc328f1b50210efc529dca0028893a2d9
        with:
          toolchain: stable

      - name: Cache cargo build
        uses: Swatinem/rust-cache@49a0bdc70d2e1b713ca9e2869b211fcce03d3c1c

      - name: Install rs-guard
        run: cargo install rs-guard --locked --version "1.8.2"

      - name: AI Code Review
        run: rs-guard
        env:
          DEEPSEEK_API_KEY: ${{ secrets.DEEPSEEK_API_KEY }}
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          PR_NUMBER: ${{ github.event.pull_request.number }}
          REPO_FULL_NAME: ${{ github.repository }}
```

### With Configuration File

If using `.reviewer.toml`:

```yaml
- name: AI Code Review
  run: rs-guard --config .reviewer.toml
  env:
    GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
    PR_NUMBER: ${{ github.event.pull_request.number }}
    REPO_FULL_NAME: ${{ github.repository }}
```

### Upload Review Artifacts

```yaml
- name: Upload review result
  # Pinned from actions/upload-artifact@v5 (330a01c4) to avoid Node.js 20 deprecation.
  uses: actions/upload-artifact@330a01c490aca151604b8cf639adc76d48f6c5d4
  if: always()
  with:
    name: review-result
    path: |
      review-result.txt
      rs-guard-metrics.json
```

### Required Secrets

Configure these in your repository settings (**Settings** → **Secrets and variables** → **Actions**):

| Secret             | Description                                       |
| ------------------ | ------------------------------------------------- |
| `DEEPSEEK_API_KEY` | Your DeepSeek API key (or other provider)         |
| `GITHUB_TOKEN`     | Auto-provided by GitHub Actions (no setup needed) |

---

## Local Testing Setup

### Step 1: Install the Binary

Choose one of the installation methods above. For development, "Build from Source" is recommended.

### Step 2: Configure API Keys

```bash
# Linux/macOS
export DEEPSEEK_API_KEY="your-api-key"

# Windows (PowerShell)
$env:DEEPSEEK_API_KEY="your-api-key"
# Or: $env:KIMI_API_KEY="..." / $env:OPENAI_API_KEY="..."

# Windows (Command Prompt)
set DEEPSEEK_API_KEY=your-api-key
REM Or: set KIMI_API_KEY=... / set OPENAI_API_KEY=...
```

**Optional:** Add to your shell profile for persistence:

```bash
# ~/.bashrc or ~/.zshrc
export DEEPSEEK_API_KEY="your-api-key"
# Or: export KIMI_API_KEY="..." / export OPENAI_API_KEY="..."
```

### Step 3: Test in a Repository

```bash
# Navigate to a git repository
cd /path/to/your/project

# Stage some changes
git add .

# Run rs-guard
rs-guard

# Or with explicit provider
rs-guard --provider kimi --model kimi-k2.5
```

### Step 4: Set Up Pre-commit Hook (Optional)

```bash
# Copy the example hook
cp examples/local-review/pre-commit-hook.sh .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit

# Now commits will be reviewed automatically
git commit -m "feat: add new feature"
```

To bypass the hook when needed:

```bash
git commit --no-verify -m "quick fix"
```

### Step 5: Review a Diff File

```bash
# Generate a diff
git diff > my-changes.diff

# Review the diff file
rs-guard --diff-file my-changes.diff
```

---

## Troubleshooting Installation

### "Command not found: rs-guard"

**Cause:** Binary not in PATH.

**Solution:**

```bash
# Find where rs-guard was installed
which rs-guard  # Linux/macOS
where rs-guard  # Windows

# If not found, check default locations:
ls ~/.cargo/bin/rs-guard
ls /usr/local/bin/rs-guard

# Add to PATH
export PATH="$HOME/.cargo/bin:$PATH"
```

### "Permission denied"

**Cause:** Binary lacks execute permission.

**Solution:** Ensure `~/.cargo/bin` is in your PATH:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

### Build fails with "Rust version too old"

**Cause:** Rust version < 1.92.

**Solution:**

```bash
rustup update stable
rustc --version  # Verify >= 1.92
```

### "SSL certificate error" during download

**Cause:** Outdated CA certificates or network issue.

**Solution:**

```bash
# Update CA certificates
sudo apt-get update && sudo apt-get install --reinstall ca-certificates  # Debian/Ubuntu
sudo yum reinstall ca-certificates  # RHEL/CentOS

# Or use --insecure flag (not recommended for production)
curl -k -L -o rs-guard <URL>
```

### Windows: "The system cannot find the file specified"

**Cause:** PATH not updated or terminal needs restart.

**Solution:**

1. Close and reopen your terminal/PowerShell
2. Verify PATH: `$env:Path -split ';' | Select-String rs-guard`
3. Re-add to PATH if needed (see Windows section above)

### macOS: "Cannot be opened because the developer cannot be verified"

**Solution:**

```bash
xattr -d com.apple.quarantine $(which rs-guard)
```

Or go to **System Preferences** → **Security & Privacy** → Click **Open Anyway**.

---

## Next Steps

- **[docs/USAGE.md](USAGE.md)** — Complete CLI reference
- **[docs/CONFIGURATION.md](CONFIGURATION.md)** — `.reviewer.toml` setup
- **[docs/PROVIDERS.md](PROVIDERS.md)** — API key acquisition for all providers
- **[docs/LOCAL_MODE.md](LOCAL_MODE.md)** — Pre-commit hook configuration
- **[examples/](../examples/)** — Example workflows and configurations

---

## Getting Help

- **Documentation:** See [docs/](docs/) directory
- **Issues:** <https://github.com/nebulaideas/rs-guard/issues>
- **Discussions:** <https://github.com/nebulaideas/rs-guard/discussions>
