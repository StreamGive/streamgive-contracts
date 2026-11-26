## Summary

<!-- Describe what this PR changes and why. -->

Closes #

## Type of change

- [ ] Bug fix
- [ ] New feature
- [ ] Refactor / test improvement
- [ ] Documentation
- [ ] Chore / CI / tooling

## Checklist

### Tests & quality checks

- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] Added or updated unit tests covering new behavior, edge cases, and auth rules

### Documentation & changelog

- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` builds without warnings
- [ ] Updated `docs/ROLES.md` if any public contract entry point was added, renamed, or removed
- [ ] Updated `docs/EVENTS.md`, `docs/API.md`, or `README.md` if events, interfaces, or storage changed
- [ ] Added an entry under `[Unreleased]` in `CHANGELOG.md` if files under `contracts/` changed (or requested the `skip-changelog` label)

### Wasm binary size

- [ ] `cargo build --workspace --target wasm32v1-none --release` succeeds
- [ ] `bash scripts/check-wasm-size.sh` passes within the configured size budgets (`ngo_registry` <= 64 KiB, `donation_vault` <= 96 KiB)
- [ ] Included before/after wasm sizes below if contract code changed (or marked N/A)

## Wasm size report (if applicable)

| Contract | Before (bytes) | After (bytes) | Delta |
| --- | --- | --- | --- |
| `ngo_registry.wasm` | N/A | N/A | N/A |
| `donation_vault.wasm` | N/A | N/A | N/A |
