# StreamGive — Contracts

Soroban smart contracts powering StreamGive, a recurring/streaming donation
platform for verified NGOs on Stellar.

## Contracts

- `ngo-registry` — on-chain NGO application, verification, and registry
- `donation-vault` — streaming donation vault (create / withdraw / cancel / modify streams)

## Related repositories

- [streamgive-backend](https://github.com/streamgive/streamgive-backend) — indexer & API
- [streamgive-frontend](https://github.com/streamgive/streamgive-frontend) — donor & NGO web app
- [streamgive-docs](https://github.com/streamgive/streamgive-docs) — documentation

## Testing

Run the full test suite for all contracts from the workspace root:

```sh
cargo test --workspace
```

To run tests for a single contract:

```sh
cargo test -p donation-vault
cargo test -p ngo-registry
```

Notable coverage:

- `donation-vault`'s `math` module unit-tests the streaming accrual
  calculation (`accrued`) directly: zero/negative rate, zero balance,
  zero elapsed time, capping at the remaining balance, and saturating
  instead of overflowing/panicking near `i128::MAX`.
- It also includes a deterministic grid-based invariant sweep
  (`invariants_hold_across_a_grid_of_inputs`) that checks, across a
  matrix of rates, balances, and elapsed durations, that accrual is
  always non-negative, never exceeds the remaining balance, and is
  monotonically non-decreasing as elapsed time (or rate) grows — a
  stand-in for property-based testing over the streaming math's edge
  cases.

CI (see [`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs
`cargo fmt --check`, `cargo clippy`, a `wasm32-unknown-unknown` release
build, a wasm binary size check (see
[`scripts/check-wasm-size.sh`](scripts/check-wasm-size.sh)), and
`cargo test --workspace` on every push and pull request.

## Status

Early development.

## License

Apache-2.0 — see [LICENSE](./LICENSE).
