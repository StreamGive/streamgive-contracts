# Contributing

The full contribution guide — coding conventions, commit/PR expectations,
how the StreamGive repos relate to each other, and so on — lives in
[streamgive-docs](https://github.com/streamgive/streamgive-docs). Read that
first; this file only covers what's specific to working in
`streamgive-contracts`.

## Local checks

Before opening a PR, run what CI runs (see
[`.github/workflows/ci.yml`](.github/workflows/ci.yml)):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --target wasm32-unknown-unknown --release
cargo test --workspace
```

See the [README](./README.md#testing) for more on the test suite, and
[docs/EVENTS.md](./docs/EVENTS.md) if your change adds or changes a
published contract event.

## Reporting issues

Open an issue in this repo for bugs or proposals scoped to the contracts
themselves. Cross-repo questions (indexer, frontend, docs site) belong in
their respective repos, linked from the [README](./README.md#related-repositories).
