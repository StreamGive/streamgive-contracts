# StreamGive — Contracts

Soroban smart contracts powering StreamGive, a recurring/streaming donation
platform for verified NGOs on Stellar.

For how these contracts fit with the backend and frontend — and how a
donation flows end to end — see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
For the stream lifecycle diagrams, see
[docs/STREAM_LIFECYCLE.md](docs/STREAM_LIFECYCLE.md); for simulating entry
points and estimating resource costs, see [docs/GAS.md](docs/GAS.md).
For who these contracts defend against, what the admin can and cannot do, and
which risks are knowingly accepted, see
[docs/THREAT_MODEL.md](docs/THREAT_MODEL.md).
For the complete callable surface, including arguments, authorization,
errors, and emitted events, see [docs/API.md](docs/API.md). Fee behavior and
rounding examples are in [docs/FEES.md](docs/FEES.md).

All of the guides in `docs/`, each with a one-line summary, are listed in
[docs/README.md](docs/README.md). If you are here to give rather than to build,
start with the plain-language [donor's guide](docs/DONORS.md).

## Contracts

- `ngo-registry` — on-chain NGO application, verification, and registry
- `donation-vault` — streaming donation vault (create / withdraw / cancel / modify streams)

### Admin transfer (donation-vault and ngo-registry)

Both contracts use the same two-step handshake. The registry has no pause
switch, but its admin transfer is otherwise identical.

| Step | Entry point | Who may call it | Effect |
| --- | --- | --- | --- |
| 1 | `propose_admin(new_admin)` | The **current admin** (admin auth) | Records `new_admin` as pending. The active admin does not change. |
| 2 | `accept_admin()` | The **pending admin only** (pending address auth) | Promotes the pending address to active admin and clears the proposal. |
| 3 | `cancel_admin_proposal()` | The **current admin only** (admin auth) | Clears the pending proposal. The active admin does not change. |

Only the current admin can propose or cancel. The pending address cannot
cancel; it can only accept. Until `accept_admin()` succeeds, the current admin
continues to control every admin-gated operation. In `donation-vault`, if the
current admin renounces, both the active and pending slots are cleared; the
registry has no renounce entry point.

## Release profile

The workspace `Cargo.toml`'s `[profile.release]` sets several non-default
flags. Soroban's resource-fee model charges per byte of the deployed wasm
and per CPU instruction executed, so a smaller, more predictable binary
isn't just nice-to-have — it directly lowers what every invocation of
these contracts costs:

| Setting             | Value       | Why                                                                                                   |
| -------------------- | ----------- | ------------------------------------------------------------------------------------------------------ |
| `opt-level`          | `"z"`       | Optimizes for binary size over speed — wasm size drives upload and storage fees.                       |
| `lto`                | `true`      | Whole-program link-time optimization, trimming dead code and shrinking the binary further.             |
| `codegen-units`      | `1`         | A single codegen unit gives the optimizer the whole crate to work with, trading build time for smaller output. |
| `panic`              | `"abort"`   | Drops unwinding tables and landing pads; Soroban traps on panic and can't unwind across the host boundary anyway. |
| `strip`              | `"symbols"` | Strips symbol/debug info from the deployed artifact — of no use on-chain, pure size cost otherwise.     |
| `debug`              | `0`         | No debug info emitted for release builds, same rationale as `strip`.                                    |
| `debug-assertions`   | `false`     | Standard release behavior — keeps hot paths free of debug-only checks.                                  |
| `overflow-checks`    | `true`      | Kept **on** in release, contrary to the Rust default — these contracts move token balances, and a silently wrapped `i128` is far worse than the small extra cost of a checked op. |

Change these with care: relaxing `opt-level`, `lto`, or `strip` grows the
deployed wasm and raises fees, while turning `overflow-checks` off would
let balance arithmetic wrap silently.

## Pausing

`donation-vault` has an admin-gated `pause` / `unpause` pair — an emergency
brake for when something is wrong. `pause` only flips the `Paused` flag in
instance storage: no funds are moved by the call itself, so balances stay where
they were and there is nothing to unwind when the pause is lifted.

While paused, the vault is not fully frozen. The state-changing entry points
that move funds or mutate stream state reject with `Error::ContractPaused`;
donor cancellation and the exceptional admin rescue path stay available. Every
other entry point, including all read-only views and the permissionless
keep-alive, continues to work. The table below lists every public entry point.

| Entry point | While paused |
| --- | --- |
| `create_stream`, `create_stream_with_start`, `create_stream_with_memo` | **Rejected** — `ContractPaused` |
| `withdraw` | **Rejected** — `ContractPaused` |
| `withdraw_batch` | **Rejected** — `ContractPaused` |
| `withdraw_all` | **Rejected** — `ContractPaused` |
| `top_up` | **Rejected** — `ContractPaused` |
| `modify_rate` | **Rejected** — `ContractPaused` |
| `transfer_stream` | **Rejected** — `ContractPaused` |
| `sweep_surplus`, `sweep_surplus_to` | **Rejected** — admin-gated; `ContractPaused` |
| `cancel_stream` | **Works** — donor auth; settles accrued payouts and refunds the unspent balance |
| `cancel_streams` | **Works** — donor auth once for the batch; cancels the listed streams all-or-nothing |
| `rescue_stream` | **Only works while paused** — admin-only emergency path; returns `NotPaused` when the vault is active |
| `pause` | **Rejected** when already paused — `AlreadyPaused` |
| `unpause` | **Works** — admin auth; lifts the pause |
| `renounce_admin` | **Works** — admin auth; no pause gate |
| `extend_stream` | **Works** — permissionless TTL bump; no pause gate |
| `init`, `__constructor` | **Works** during setup; neither has a pause gate (the vault cannot be paused before it is initialised) |
| `propose_admin`, `accept_admin`, `cancel_admin_proposal` | **Work** — admin handshake is not paused |
| `set_treasury`, `clear_treasury`, `set_fee_bps`, `set_token_fee_bps`, `clear_token_fee_bps` | **Work** — admin-gated configuration; no pause gate |
| `set_min_deposit`, `set_cancel_grace_ledgers`, `set_max_streams_per_donor`, `set_max_rate`, `clear_max_rate`, `set_min_duration`, `set_registry`, `clear_registry`, `upgrade`, `set_upgrade_delay`, `propose_upgrade`, `execute_upgrade`, `cancel_upgrade` | **Work** — admin-gated configuration; no pause gate |
| `total_donated`, `admin`, `pending_admin`, `get_stream`, `streams_by_donor`, `stream_count`, `pending_accrual`, `pending_payout`, `pending_refund`, `depletion_time`, `preview_stream`, `paused`, `get_config`, `treasury`, `fee_bps`, `max_fee_bps`, `token_fee_bps`, `allowed_tokens`, `min_deposit`, `cancel_grace_ledgers`, `max_streams_per_donor`, `max_rate`, `min_duration`, `registry`, `payout_address`, `upgrade_delay`, `pending_upgrade`, `committed_balance`, `surplus` | **Work** — read-only views are never paused |

Pausing does **not** stop time-based accrual. A stream's `pending_accrual`
keeps growing while the vault is paused, so a stream paused for a week still
owes a week of accrual once the pause is lifted. That accrual is claimable via
`withdraw` as soon as the vault is unpaused.

`withdraw`, `withdraw_batch`, and `withdraw_all` being rejected is the point of
the brake: they are the paths that pay tokens straight out of the vault, so a
pause triggered
by a suspected vulnerability has to close them or an attacker could drain funds
while the rest of the contract is frozen.

`cancel_stream` is deliberately left open. It is the one ordinary path that
returns money to a donor, so keeping it available means a pause can never trap
a donor's unspent deposit. `cancel_streams` is its bounded batch form and is
left open for the same reason. `rescue_stream` is the opposite: it exists only for
incident response, is admin-only, and deliberately refuses to run while the
vault is active.

## Storage TTL and keeping entries alive

Soroban contracts use bounded Time-To-Live (TTL) for on-chain state retention:

- **Instance storage** (contract admins, configuration, pause flags) is bumped
  to 30 days (`518,400` ledgers) on every state-changing call.
- **Persistent storage** (each individual `Stream` record in `donation-vault` and
  each `Ngo` record in `ngo-registry`) has an independent 90-day TTL (`1,555,200`
  ledgers) that must be maintained per entry.

### Expiry risk for idle entries

If a stream has no activity (withdrawals, top-ups, rate modifications) or an NGO
entry receives no updates for 90 consecutive days, its TTL expires and the network
**archives** the entry.

Archived entries cannot be read or modified by normal contract calls (`get_stream`,
`withdraw`, verification lookups, etc. will fail) until a Soroban state restoration
transaction is submitted and network restoration fees are paid. Long-running streams
with low drip rates or infrequent withdrawals are especially at risk if left untouched.

### Keep-alive entry points (`extend_stream` & `touch_ngo`)

To protect idle entries from archival without moving funds, modifying balances, or
requiring admin credentials, both contracts provide permissionless keep-alive entry
points that anyone (donors, NGOs, keeper bots, or indexers) can invoke:

- **`donation-vault::extend_stream(stream_id)`**: Extends the persistent storage
  TTL of `DataKey::Stream(stream_id)` back to 90 days. Can be called at any time,
  requires no authorization, and remains accessible even while the vault is paused.
- **`ngo-registry::touch_ngo(owner)`**: Refreshes both the registry instance TTL
  (to 30 days) and the NGO's persistent storage TTL (`DataKey::Ngo(owner)`) back
  to 90 days without altering registration status. Requires no authorization.

For full key specifications, bump thresholds, and archival lifecycle details, see
[docs/STORAGE.md](docs/STORAGE.md).

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
`cargo fmt --check`, `cargo clippy`, a `wasm32v1-none` release
build, a wasm binary size check (see
[`scripts/check-wasm-size.sh`](scripts/check-wasm-size.sh)), and
`cargo test --workspace` on every push and pull request.

## Deploying

[`scripts/deploy-testnet.sh`](scripts/deploy-testnet.sh) builds both
contracts, deploys `ngo-registry` and `donation-vault` to Stellar
testnet (initializing each atomically via `__constructor`), applies any
optional vault configuration, verifies `admin()`, and writes the resulting
contract IDs and Wasm SHA-256 hashes to [`deployments.json`](deployments.json).

### Prerequisites

- The [`stellar` CLI](https://developers.stellar.org/docs/tools/cli).
- `rustup` with the `wasm32v1-none` target:
  `rustup target add wasm32v1-none`.
- Node.js (`node`), used to check for existing entries, compute Wasm SHA-256
  hashes, and atomically update `deployments.json` while preserving any
  existing `mainnet` entry.
- A funded testnet identity, for example:
  `stellar keys generate my-testnet-identity --network testnet --fund`.

Both deploy scripts check for `stellar`, `node`, `rustup`, and the
`wasm32v1-none` target up front and exit before deploying anything if one is
missing.

### Environment variables

- `STELLAR_SOURCE_ACCOUNT` (required): the name of the funded Stellar CLI
  identity that signs and pays for the deploy.
- `STELLAR_ADMIN_ADDRESS` (optional on testnet; required `G...` address on
  mainnet): the address set as admin of both contracts. Use a wallet a human
  can sign with, because admin actions such as approving NGOs are driven from
  the browser admin panel, which a CLI-only deployer key cannot do. On testnet
  this falls back to the address of `STELLAR_SOURCE_ACCOUNT`, which is fine
  for a throwaway deploy only.
- `STELLAR_REGISTRY_ADDRESS` (optional on testnet; required on mainnet unless
  `--allow-no-registry` is passed): address of `ngo-registry` (`C...` or
  `G...`), or `"auto"` to wire the vault to the newly deployed registry via
  `set_registry`.
- `STELLAR_TREASURY_ADDRESS` (optional): treasury address (`C...` or `G...`)
  passed to `set_treasury`. Required if `STELLAR_FEE_BPS > 0`.
- `STELLAR_FEE_BPS` (optional): global protocol fee in basis points
  (`0`–`1000`), passed to `set_fee_bps`.
- `STELLAR_MIN_DEPOSIT` (optional): minimum stream deposit in stroops
  (non-negative integer), passed to `set_min_deposit`.

### Running the script

From the repo root:

```sh
STELLAR_SOURCE_ACCOUNT=my-testnet-identity \
STELLAR_ADMIN_ADDRESS=G... \
STELLAR_REGISTRY_ADDRESS=auto \
./scripts/deploy-testnet.sh
```

`deploy-testnet.sh` refuses to overwrite an existing testnet entry in
`deployments.json`; pass `--force` to overwrite it deliberately.

### `deployments.json`

`deployments.json` holds the live testnet deployment at the top level: the
network, network passphrase, deployment timestamp, admin address, contract
IDs, and Wasm SHA-256 hashes. The backend and frontend read these IDs from
their env files, so update them there after a redeploy. Avoid committing a
`deployments.json` rewritten by a personal test deploy.

A mainnet deployment is recorded under the `mainnet` key of the same file by
[`scripts/deploy-mainnet.sh`](scripts/deploy-mainnet.sh), which requires
`--confirm`, `STELLAR_SOURCE_ACCOUNT`, `STELLAR_ADMIN_ADDRESS`,
`STELLAR_RPC_URL`, and `STELLAR_REGISTRY_ADDRESS` (unless
`--allow-no-registry` is passed).

## FAQ

### Why is this project licensed under Apache-2.0?

Apache-2.0 permits reuse and modification while providing an explicit patent
license and clear contributor protections. That makes it a practical default
for contracts intended to be integrated by wallets, applications, and other
open-source projects.

### Why are release overflow checks enabled?

The contracts move token balances and calculate payouts with `i128`. A wrapped
balance could silently corrupt funds, so release builds keep `overflow-checks`
enabled and return explicit arithmetic errors where the contract can handle
the failure.

### Why are the contracts `no_std`?

Soroban contracts run in a constrained WebAssembly environment. `no_std`
keeps the deployed artifact small and avoids bringing operating-system
facilities that are unavailable on-chain.

### Why does each stream have its own TTL?

Persistent storage is retained per key. A stream that is never touched can
expire independently of the vault instance, so state-changing calls and the
permissionless `extend_stream` entry point refresh the specific stream that
needs to remain available.

### What is the cancelled-stream grace period?

The admin can configure `cancel_grace_ledgers` so indexers have additional
time to observe and process a cancellation. Cancelling a stream retains its
record for the normal stream TTL plus that configured grace period; a value of
zero keeps the default retention period.

## Error codes

Each contract exposes its failures as a `#[contracterror] enum Error`,
returned as `Result<_, Error>` from every fallible entry point. Clients see
the numeric code below (e.g. a failed `try_withdraw` surfacing `Error(5)`).

### `donation-vault`

Codes are append-only and never renumbered; the `donation-vault` enum is
currently allocated up to code 41. See [docs/UPGRADE_POLICY.md](docs/UPGRADE_POLICY.md). "Admin-gated" entry points first call
`require_admin`, so they can also return `NotInitialized` (no admin) or
`AdminRenounced`.

| Code | Error | Meaning | Entry points |
| --- | --- | --- | --- |
| 1 | `AlreadyInitialized` | `init` already created an admin. | `init` |
| 2 | `NotInitialized` | No admin is stored. | `admin`, `propose_admin`, `cancel_admin_proposal`, `pause`, `unpause`, `set_treasury`, `clear_treasury`, `set_fee_bps`, `set_token_fee_bps`, `clear_token_fee_bps`, `set_min_deposit`, `set_cancel_grace_ledgers`, `set_max_streams_per_donor`, `set_min_duration`, `set_registry`, `renounce_admin`, `rescue_stream`, `upgrade`, `set_upgrade_delay`, `propose_upgrade`, `execute_upgrade`, `cancel_upgrade`, `sweep_surplus` |
| 3 | `StreamNotFound` | No stream exists for the id. | `get_stream`, `pending_accrual`, `pending_payout`, `pending_refund`, `depletion_time`, `extend_stream`, `withdraw`, `withdraw_batch`, `withdraw_all`, `cancel_stream`, `cancel_streams`, `rescue_stream`, `top_up`, `modify_rate`, `transfer_stream` |
| 4 | `InvalidAmount` | A deposit, rate, top-up, rate change, minimum deposit, or sweep amount was zero/negative (or a fee would swallow the first payout). | `create_stream`, `create_stream_with_start`, `create_stream_with_memo`, `top_up`, `modify_rate`, `set_min_deposit`, `sweep_surplus` |
| 5 | `NothingToWithdraw` | The stream accrued nothing since its last checkpoint. | `withdraw` |
| 6 | `ContractPaused` | The admin paused the vault; see [Pausing](#pausing). | `create_stream`, `create_stream_with_start`, `create_stream_with_memo`, `withdraw`, `withdraw_batch`, `withdraw_all`, `top_up`, `modify_rate`, `transfer_stream`, `sweep_surplus`, `sweep_surplus_to` |
| 7 | `FeeTooHigh` | Fee above the 1,000 bps (10%) cap. | `set_fee_bps`, `set_token_fee_bps` |
| 8 | `NoPendingAdmin` | No admin proposal is pending. | `accept_admin`, `cancel_admin_proposal` |
| 9 | `ArithmeticOverflow` | Checked balance, total, counter, or grace-ledger arithmetic left `i128`/`u64` range. | `create_stream`, `create_stream_with_start`, `create_stream_with_memo`, `withdraw`, `withdraw_batch`, `withdraw_all`, `cancel_stream`, `cancel_streams`, `top_up`, `modify_rate`, `transfer_stream`, `set_cancel_grace_ledgers` |
| 10 | `SelfStream` | Donor and NGO are the same address. | `create_stream`, `create_stream_with_start`, `create_stream_with_memo` |
| 11 | `InvalidTreasury` | Treasury address is the vault itself. | `set_treasury` |
| 12 | `AlreadyPaused` | `pause` called while already paused. | `pause` |
| 13 | `AlreadyUnpaused` | `unpause` called while already active. | `unpause` |
| 14 | `DepositTooLow` | Deposit is below the configured `min_deposit`. | `create_stream`, `create_stream_with_start`, `create_stream_with_memo` |
| 15 | `StreamCancelled` | The stream was already closed out by `cancel_stream`/`rescue_stream`. | `pause_stream`, `resume_stream`, `top_up`, `modify_rate`, `transfer_stream`, `rescue_stream` |
| 16 | `InvalidAdmin` | The proposed admin equals the current admin. | `propose_admin` |
| 17 | `StreamLimitExceeded` | Donor reached `max_streams_per_donor`. | `create_stream`, `create_stream_with_start`, `create_stream_with_memo` |
| 18 | `NgoNotVerified` | A configured registry has no verified entry for the NGO. | `create_stream`, `create_stream_with_start`, `create_stream_with_memo`, `top_up`, `transfer_stream`, `modify_rate` (rate increases only) |
| 19 | `StreamCounterMissing` | The stream-id counter is absent from instance storage. | `create_stream`, `create_stream_with_start`, `create_stream_with_memo` |
| 20 | `AdminRenounced` | Admin control was permanently renounced, or `init` ran after a renounce. | `init`, `admin`, `propose_admin`, `cancel_admin_proposal`, `pause`, `unpause`, `set_treasury`, `clear_treasury`, `set_fee_bps`, `set_token_fee_bps`, `clear_token_fee_bps`, `set_min_deposit`, `set_cancel_grace_ledgers`, `set_max_streams_per_donor`, `set_min_duration`, `set_registry`, `renounce_admin`, `rescue_stream`, `upgrade`, `set_upgrade_delay`, `propose_upgrade`, `execute_upgrade`, `cancel_upgrade`, `sweep_surplus`, `sweep_surplus_to` |
| 21 | `FeeRequiresTreasury` | A non-zero fee was set with no treasury, or the treasury was cleared while a fee is set. | `set_fee_bps`, `set_token_fee_bps`, `clear_treasury` |
| 22 | `NotPaused` | `rescue_stream` called while the vault is active. | `rescue_stream` |
| 23 | `MixedNgo` | A batch withdrawal mixed streams belonging to different NGOs. | `withdraw_batch` |
| 24 | `BatchLimitExceeded` | Too many ids in one batch call. | `get_streams`, `withdraw_batch`, `cancel_streams` |
| 25 | `DuplicateStreamId` | The same stream id appears twice in a batch call. | `withdraw_batch`, `cancel_streams` |
| 26 | `InvalidNgo` | The NGO is the vault's own address. | `create_stream`, `create_stream_with_start`, `create_stream_with_memo` |
| 27 | `InvalidToken` | The token is the vault's own address. | `create_stream`, `create_stream_with_start`, `create_stream_with_memo` |
| 28 | `StreamPaused` | `withdraw`/`withdraw_batch`/`withdraw_all` called on a stream whose donor paused it with `pause_stream`. | `withdraw`, `withdraw_batch`, `withdraw_all` |
| 29 | `RateTooHigh` | Rate exceeds the admin-configured `max_rate`. | `create_stream`, `create_stream_with_start`, `create_stream_with_memo`, `modify_rate` |
| 30 | `StartTooFar` | `create_stream_with_start` was given a `start` more than one year (`MAX_START_DELAY_SECONDS`) ahead of the current ledger time. | `create_stream_with_start` |
| 31 | `MemoTooLong` | `create_stream_with_memo` or `set_memo` was given a `memo` longer than `MAX_MEMO_BYTES` (32) bytes. | `create_stream_with_memo`, `set_memo` |
| 32 | `StreamAlreadyPaused` | `pause_stream` called on a stream that is already paused. | `pause_stream` |
| 33 | `StreamNotPaused` | `resume_stream` called on a stream that is not paused. | `resume_stream` |
| 34 | `NoPendingUpgrade` | `execute_upgrade` or `cancel_upgrade` was called with no upgrade proposed. | `execute_upgrade`, `cancel_upgrade` |
| 35 | `UpgradeDelayNotElapsed` | `execute_upgrade` was called before the proposed upgrade's timelock elapsed. | `execute_upgrade` |
| 36 | `UpgradeTimelockEnabled` | The immediate `upgrade` entry point was called while a non-zero `upgrade_delay` is configured. | `upgrade` |
| 37 | `NoTreasury` | `sweep_surplus` was called before a treasury was configured, so the surplus has nowhere to go. | `sweep_surplus` |
| 38 | `SweepExceedsSurplus` | `sweep_surplus` or `sweep_surplus_to` was asked to move more than the surplus above the token's committed balance, which would dip into stream funds. | `sweep_surplus`, `sweep_surplus_to` |
| 39 | `StreamTooShort` | A stream's deposit would deplete in fewer seconds than the admin-configured `min_duration`. Distinct from `InvalidAmount`, which only covers zero or negative amounts. | `create_stream`, `create_stream_with_start`, `create_stream_with_memo`, `modify_rate` |
| 43 | `StreamDrained` | `pause_stream` called on a stream that already paid out in full (status `Drained`). It was not cancelled. | `pause_stream` |
| 44 | `StreamEmpty` | `pause_stream` called on a stream whose balance is zero but that is neither cancelled, drained nor paused. | `pause_stream` |
| 45 | `InvalidRegistry` | `set_registry` was given an address that does not answer the registry's `ngo_count` probe: a plain account, the vault itself, or an unrelated contract. | `set_registry` |

### `ngo-registry`

Codes 1–15 are the full `Error` enum. The same append-only rule applies. Admin-gated entry points first call
`require_admin`, and the review entry points first call `require_approver`, so they can also return
`NotInitialized` or `NotApprover`. The admin is an implicit approver: the
registry keeps an admin-managed list of additional addresses permitted to
review NGOs (`add_approver` / `remove_approver` / `is_approver` /
`list_approvers`) so review work can be delegated without handing out the admin
key.

| Code | Error | Meaning | Entry points |
| --- | --- | --- | --- |
| 1 | `AlreadyInitialized` | `init` already created an admin. | `init` |
| 2 | `NotInitialized` | No admin is stored. | `admin`, `propose_admin`, `cancel_admin_proposal`, `approve_ngo`, `batch_approve`, `reject_ngo`, `revoke_ngo`, `add_approver`, `remove_approver`, `set_verify_period`, `renew_ngo`, `upgrade` |
| 3 | `AlreadyRegistered` | The owner already has a registry entry. | `register` |
| 4 | `NotRegistered` | No entry exists for the owner. | `unregister`, `update_name`, `set_metadata_uri`, `get_ngo`, `approve_ngo`, `batch_approve`, `reject_ngo`, `revoke_ngo`, `renew_ngo`, `touch_ngo` |
| 5 | `AlreadyVerified` | The action is not allowed once an NGO is verified: unregister, rename, reject, or approve again. | `unregister`, `update_name`, `approve_ngo`, `batch_approve`, `reject_ngo` |
| 6 | `NameTooLong` | Name exceeds 200 bytes. | `register`, `update_name` |
| 7 | `NotVerified` | `revoke_ngo` or `renew_ngo` called on an NGO that is not verified. | `revoke_ngo`, `renew_ngo` |
| 8 | `NoPendingAdmin` | No admin proposal is pending. | `accept_admin`, `cancel_admin_proposal` |
| 9 | `InvalidAdmin` | The proposed admin equals the current admin. | `propose_admin` |
| 10 | `ArithmeticOverflow` | The owner index or total count left `u32`/`u64` range. | `register` |
| 11 | `InvalidName` | The name is empty or whitespace-only. | `register` |
| 12 | `NotApprover` | The acting address is neither the admin nor on the approver list. | `approve_ngo`, `batch_approve`, `reject_ngo`, `revoke_ngo`, `remove_approver` |
| 13 | `AlreadyApprover` | The address is already on the approver list. | `add_approver` |
| 14 | `MetadataUriTooLong` | The metadata URI exceeds 512 bytes. | `set_metadata_uri` |
| 15 | `NoVerifyPeriod` | `renew_ngo` was called while no verification period is configured. | `renew_ngo` |
| 16 | `RegistrationsPaused` | `register` was called while the admin has paused new registrations. | `register` |

## Status

Early development.

## License

Apache-2.0 — see [LICENSE](./LICENSE).
