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
build, and `cargo test --workspace` on every push and pull request.

## Error codes

Each contract exposes its failures as a `#[contracterror] enum Error`,
returned as `Result<_, Error>` from every fallible entry point. Clients see
the numeric code below (e.g. a failed `try_withdraw` surfacing `Error(5)`).

### `donation-vault`

| Code | Error                | Meaning                                                                 |
| ---- | --------------------- | ------------------------------------------------------------------------ |
| 1    | `AlreadyInitialized`  | `init` was already called; the vault already has an admin.               |
| 2    | `NotInitialized`      | `init` has not been called yet, so there is no admin to act as.          |
| 3    | `StreamNotFound`      | No stream exists for the given stream id.                                |
| 4    | `InvalidAmount`       | `deposit` or `rate` passed to `create_stream` was zero or negative.      |
| 5    | `NothingToWithdraw`   | The stream has accrued nothing since its last checkpoint.                |
| 6    | `ContractPaused`      | The admin has paused the vault; only `cancel_stream` still works.        |
| 7    | `FeeTooHigh`          | `set_fee_bps` was called with a value above the 10% (1,000 bps) cap.     |
| 8    | `NoPendingAdmin`      | `accept_admin` was called without a prior (or already-completed) `propose_admin`. |

### `ngo-registry`

| Code | Error                | Meaning                                                          |
| ---- | --------------------- | ------------------------------------------------------------------ |
| 1    | `AlreadyInitialized`  | `init` was already called; the registry already has an admin.    |
| 2    | `NotInitialized`      | `init` has not been called yet, so there is no admin to act as.  |
| 3    | `AlreadyRegistered`   | `register` was called for an address that already has an entry. |
| 4    | `NotRegistered`       | No registry entry exists for the given owner address.            |

## Status

Early development.

## License

Apache-2.0 — see [LICENSE](./LICENSE).
