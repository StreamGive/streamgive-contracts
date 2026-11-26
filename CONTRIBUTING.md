# Contributing

The full contribution guide — coding conventions, commit/PR expectations,
how the StreamGive repos relate to each other, and so on — lives in
[streamgive-docs](https://github.com/streamgive/streamgive-docs). Read that
first; this file only covers what's specific to working in
`streamgive-contracts`.

Please note that this project is released with a
[Contributor Code of Conduct](./CODE_OF_CONDUCT.md). By participating you
agree to abide by its terms.

## Local checks

Before opening a PR, run what CI runs (see
[`.github/workflows/ci.yml`](.github/workflows/ci.yml)):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --target wasm32v1-none --release
cargo test --workspace
```

### Pre-commit hook

To catch formatting and lint failures before committing, install the repository's git hook (see [`scripts/hooks/README.md`](./scripts/hooks/README.md)):

```sh
git config core.hooksPath scripts/hooks
chmod +x scripts/hooks/pre-commit
```

The hook automatically checks `cargo fmt --check` and runs `cargo clippy` on any changed crates whenever you commit.

See the [README](./README.md#testing) for more on the test suite, and
[docs/EVENTS.md](./docs/EVENTS.md) if your change adds or changes a
published contract event.

## Writing contract tests

Contract tests live beside each contract in
[`contracts/donation-vault/src/test.rs`](./contracts/donation-vault/src/test.rs)
and [`contracts/ngo-registry/src/test.rs`](./contracts/ngo-registry/src/test.rs).
They run against Soroban's in-memory test environment; you do not need a
network, wallet, or Stellar CLI to run them.

### Set up and run

Install the stable Rust toolchain with `rustup`. From the repository root,
Cargo resolves the pinned dependencies from `Cargo.lock`. Run the tests for
the contract you changed:

```sh
cargo test -p donation-vault
cargo test -p ngo-registry
```

Or run both contracts:

```sh
cargo test --workspace
```

CI also builds the release Wasm, so install its target and run the complete
local checks before submitting:

```sh
rustup target add wasm32v1-none
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --target wasm32v1-none --release
cargo test --workspace
```

### Test helpers and environment

Both test modules have a `setup()` helper. It creates an `Env`, registers the
contract with its constructor, and returns a generated client; the default
setup also calls `env.mock_all_auths()`. The registry setup returns
`(Env, NgoRegistryClient, admin)`. The vault's `Setup` bundles its `Env`,
`DonationVaultClient`, token client and token admin, donor, and NGO. Prefer
these helpers for ordinary tests so setup stays consistent.

For vault tests, `create_token()` registers a Stellar Asset Contract and
returns a token client and its admin client. Mint funds with
`token_admin.mint(&address, &amount)` before testing deposits or payouts.
When a test needs a special configuration, follow the nearby tests' pattern:
extend the fixture or add a small helper rather than duplicating a long setup.

`mock_all_auths()` is convenient for testing behavior after successful
authorization, but it accepts any address for any `require_auth` call. Do not
rely on it to verify authorization. For an auth-negative test, clear the
mocked authorizations with `env.mock_auths(&[])` and assert the call fails
(many existing tests use `#[should_panic]` for the auth failure). To test who
must authorize a call, install a scoped `MockAuth` for that contract,
function, and argument tuple. For example, the vault tests use
`assert_auth_required_from()` to verify the recorded authorizer. A narrowly
scoped mock looks like this:

```rust
env.mock_auths(&[MockAuth {
    address: &admin,
    invoke: &MockAuthInvoke {
        contract: &client.address,
        fn_name: "set_treasury",
        args: (treasury.clone(),).into_val(&env),
        sub_invokes: &[],
    },
}]);
```

The test modules already import `MockAuth`, `MockAuthInvoke`, and `IntoVal`.
Match `fn_name` and `args` to the actual generated-client call. For methods
with nested authorized contract calls, describe the expected nested calls in
`sub_invokes`.

Use `env.ledger().with_mut(...)` to control ledger time or TTL limits in tests
that depend on them; do not wait on wall-clock time. Vault event helpers such
as `last_event()`, `assert_last_event()`, and `count_events_with_topic()`
compare emitted events, including topic/data payloads. Event buffers are
cleared between top-level invocations, so assert an event before making
another contract call.

### Adding a test for a new function

1. Add the contract function and its generated-client call in the contract's
   `lib.rs`; add the test in that contract's `src/test.rs`.
2. Start from that module's `setup()` fixture. For the vault, use the existing
   `Setup` fields and `create_token()` where the function needs a token.
3. Add a success case that calls the generated client and checks the observable
   result: returned value, relevant state, token balances, or event. Add
   focused failure cases for the function's important boundaries and expected
   contract errors.
4. If the function requires authorization, test the required signer explicitly
   and include a missing or wrong signer case. Do not let the fixture's
   `mock_all_auths()` make an auth test pass without checking the authorizer.
5. For expected contract errors, use the generated client's `try_` method and
   compare the error, rather than expecting a panic. For example:

   ```rust
   let result = client.try_register(&owner, &whitespace_name);
   assert_eq!(result, Err(Ok(Error::InvalidName)));
   ```

   Check nearby tests for the exact error type and setup pattern for the
   function you are covering.
6. Name the test for the behavior it protects, then run the focused package
   tests and formatting check:

   ```sh
   cargo test -p ngo-registry register_whitespace_only_name_fails
   cargo fmt --all -- --check
   ```

If a new public entry point is added, also update its role row in
[`docs/ROLES.md`](./docs/ROLES.md); the role-documentation tests require the
table to list every public entry point.

## Reporting issues

Open an issue in this repo for bugs or proposals scoped to the contracts
themselves. Cross-repo questions (indexer, frontend, docs site) belong in
their respective repos, linked from the [README](./README.md#related-repositories).
