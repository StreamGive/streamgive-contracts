# Pre-Commit Hook

This repository provides a pre-commit git hook in `scripts/hooks/pre-commit` to prevent formatting and lint issues from reaching CI.

## What it does

When committing staged changes, the hook:
1. Detects which crates have staged changes (`contracts/donation-vault`, `contracts/ngo-registry`, or root manifests).
2. If no contract code was changed (e.g. only documentation or scripts), it exits early.
3. If contract code was changed:
   - Runs `cargo fmt --all -- --check` to ensure code is properly formatted.
   - Runs `cargo clippy` on the affected crate(s) (or `--workspace` if both/manifests changed) with `-D warnings` to enforce lint checks.

## Installation

### Option 1: Configure Git hooks directory (Recommended)

Point your repository's `core.hooksPath` to the `scripts/hooks` directory:

```sh
git config core.hooksPath scripts/hooks
chmod +x scripts/hooks/pre-commit
```

### Option 2: Copy to `.git/hooks`

Copy or symlink the script directly to your local `.git/hooks`:

```sh
cp scripts/hooks/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

## Bypassing

If you need to bypass the hook in an emergency (not recommended), use `--no-verify`:

```sh
git commit --no-verify -m "commit message"
```
