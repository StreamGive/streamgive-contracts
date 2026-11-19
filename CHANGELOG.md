# Changelog

All notable changes to this repository's contracts will be documented in
this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project does not yet follow a formal versioning scheme — each contract's
`Cargo.toml` still reads `0.1.0` — so entries are grouped under
[Unreleased] until the first tagged release.

## [Unreleased]

### Added

- `donation-vault`: contract skeleton with stream storage schema.
- `donation-vault`: `create_stream`, `withdraw`, `withdraw_batch`,
  `cancel_stream`, `rescue_stream`, `top_up`, `modify_rate`, and
  `transfer_stream` for the full streaming donation lifecycle.
- `donation-vault`: `withdraw_batch` to claim accrued payouts across up to
  `MAX_BATCH_WITHDRAW_STREAMS` (20) unique streams belonging to the same NGO
  in one transaction, aggregating net and fee transfers per token.
- `donation-vault`: admin-only `rescue_stream` emergency escape hatch while
  paused, refunding a stream's full remaining balance directly to the donor
  without settling accrual to the NGO and emitting `rescue`.
- `donation-vault`: donor-gated `transfer_stream` to reassign an active stream
  to a different verified NGO after settling prior accrual to the old NGO,
  emitting `transfer`.
- `donation-vault`: admin-gated `pause` / `unpause` for fund-moving actions.
- `donation-vault`: optional protocol fee (`set_fee_bps` / `fee_bps` /
  `max_fee_bps`) with treasury payout (`set_treasury` / `clear_treasury` /
  `treasury`).
- `donation-vault`: per-token protocol fee overrides (`set_token_fee_bps`,
  `clear_token_fee_bps`, `token_fee_bps`) stored in persistent storage,
  emitting `tokfeeset` and `tokfeeclr`.
- `donation-vault`: admin-configurable minimum deposit floor
  (`set_min_deposit` / `min_deposit`), enforced on `create_stream` and
  `top_up` (`Error::DepositTooLow`) and emitting `mindep`.
- `donation-vault`: per-donor stream cap (`set_max_streams_per_donor` /
  `max_streams_per_donor`, defaulting to `100`), enforced before token
  transfer (`Error::StreamLimitExceeded`) and emitting `maxstrm`.
- `donation-vault`: optional NGO registry verification (`set_registry` /
  `clear_registry` / `registry`), fail-closed and checked on `create_stream`,
  `top_up`, and `transfer_stream` (`Error::NgoNotVerified`), emitting
  `regset` and `regclr`.
- `donation-vault`: `total_donated` getter tracking cumulative deposits and
  top-ups across all streams.
- `donation-vault`: read-only views `get_streams` (batch lookup capped at
  `MAX_BATCH_GET_STREAMS = 40`), `streams_by_donor`, `stream_count`,
  `pending_accrual`, `pending_payout`, `depletion_time`, `get_config`, and
  `allowed_tokens`.
- `donation-vault`: admin-configurable `cancel_grace_ledgers` retention for
  cancelled streams (`set_cancel_grace_ledgers` / `cancel_grace_ledgers`,
  emitting `grceset`), with TTL coverage for indexing after cancellation.
- `donation-vault`: two-step admin transfer (`propose_admin`, `accept_admin`,
  `cancel_admin_proposal`, `pending_admin`), self-proposal rejection
  (`Error::InvalidAdmin`), and permanent `renounce_admin` (emitting
  `renounce`).
- `donation-vault`, `ngo-registry`: `__constructor` for atomic
  deployment-time admin initialization in both contracts, emitting `init`.
- `donation-vault`, `ngo-registry`: admin-gated in-place `upgrade` entry
  points emitting `upgrade`, with unit test coverage verifying authorization
  and state preservation.
- `donation-vault`, `ngo-registry`: typed events for all admin configuration
  and lifecycle changes (`init`, `upgrade`, `treasset`, `treclr`, `feeset`,
  `tokfeeset`, `tokfeeclr`, `mindep`, `grceset`, `regset`, `regclr`,
  `maxstrm`, `propadmin`, `acptadmin`, `canceladm`, `renounce`, `rescue`,
  `transfer`; see [`docs/EVENTS.md`](docs/EVENTS.md)).
- `donation-vault`: regression coverage for one-stroop-per-second streams and
  their zero-rounded protocol fee.
- `DataKey` enums now derive `Debug` in both contracts for clearer storage-key
  diagnostics.
- `ngo-registry`: contract skeleton with storage types and `init`.
- `ngo-registry`: NGO application/registration via `register`, owner-gated
  `update_name` and `unregister` (blocked once verified with
  `Error::AlreadyVerified`).
- `ngo-registry`: admin-gated `approve_ngo`, `batch_approve`, `revoke_ngo`,
  and `remove_ngo`.
- `ngo-registry`: two-step admin transfer (`propose_admin`, `accept_admin`,
  `cancel_admin_proposal`, `pending_admin`).
- `ngo-registry`: read-only getters `ngo_count` (with deprecated `total_ngos`
  alias), `is_verified`, and paginated `list_ngos` (capped at
  `MAX_LIST_LIMIT = 50`).
- Documentation: `SECURITY.md`, `docs/ARCHITECTURE.md`, `docs/FEES.md`,
  `docs/STORAGE.md`, `docs/EVENTS.md`, `docs/THREAT_MODEL.md`, and README
  sections covering release profile, pausing, storage TTLs/keep-alive entry
  points (`extend_stream` and `touch_ngo`), error codes, and FAQ.
- `scripts/deploy-testnet.sh` and `scripts/deploy-mainnet.sh` for building,
  deploying, initializing, configuring, and recording both contracts (with
  network passphrases and Wasm SHA-256 hashes) in `deployments.json`.
- CI workflow running `cargo fmt --check`, `cargo clippy`, `cargo doc`, a
  `wasm32v1-none` release build, Wasm binary size check, and
  `cargo test --workspace`.

### Changed

- **BREAKING** `donation-vault`: `Stream` struct layout changed — added
  `created_at: u64`, `status: StreamStatus`, and `cancelled: bool` fields.
  Existing persistent `Stream` entries written by earlier builds cannot be
  decoded after an in-place Wasm upgrade without migration.
- **BREAKING** `donation-vault`, `ngo-registry`: `Error` enum variants were
  expanded and renumbered (`donation-vault` codes `1`–`25`, `ngo-registry`
  codes `1`–`11`). Clients and indexers matching numeric Soroban error codes
  must update to the current enum values.
- **BREAKING** `donation-vault`: updated event data payloads to include net,
  fee, and settled amounts. `withdraw` now emits
  `(accrued: i128, fee: i128, net: i128)` instead of single-value `accrued`;
  `topup` now emits `(amount: i128, settled: i128)` instead of single-value
  `amount`; `ratemod` now emits `(old_rate: i128, new_rate: i128, settled: i128)`
  instead of `(old_rate, new_rate)`; and `transfer` emits
  `(old_ngo: Address, new_ngo: Address, settled: i128)` instead of
  `(old_ngo, new_ngo)`. Off-chain indexers and subscribers must update their
  event decoders for these new data shapes.
- **BREAKING** `donation-vault`: `get_config` returns an extended `Config`
  struct that includes `min_deposit`, `max_streams_per_donor`, `registry`,
  `cancel_grace_ledgers`, `total_donated`, `pending_admin`, and `renounced`.
  Clients decoding the old struct shape will need updating.
- `donation-vault`, `ngo-registry`: `init` now requires `admin.require_auth()`
  and is superseded by `__constructor` for atomic deployment-time
  initialization.
- `donation-vault`: `set_fee_bps` and `set_token_fee_bps` reject non-zero
  fees when no treasury is configured (`Error::FeeRequiresTreasury`), and
  `clear_treasury` rejects clearing while a non-zero global fee is set.
- `donation-vault`: `min_deposit` is enforced on `top_up` as well as
  `create_stream`, and registry verification (`require_verified_ngo`) is
  re-checked on `top_up` and `transfer_stream`.
- `donation-vault`: stream `balance`/`withdrawn` updates and the stream-id
  counter use checked arithmetic, returning `Error::ArithmeticOverflow`
  instead of panicking on overflow.
- `donation-vault`: `create_stream` and `transfer_stream` reject a stream
  whose `donor` and `ngo` are the same address with `Error::SelfStream`, so a
  deposit can't be counted as a committed donation while streaming straight
  back to its donor.
- `ngo-registry`: `register` and `update_name` validate names — rejecting
  empty names, Unicode whitespace-only or invisible-format-only names,
  control characters, and invalid UTF-8 (`Error::InvalidName`), as well as
  names exceeding 200 bytes (`Error::NameTooLong`).
- Both contracts migrated event declarations from deprecated
  `Events::publish` to explicit `#[contractevent]` types. Topic names and
  positional payload formats are declared in code and documented in
  `docs/EVENTS.md`; consumers should update to the generated event types
  before the next contract release.
- `scripts/deploy-testnet.sh`, `scripts/deploy-mainnet.sh`: switched to
  atomic constructor deploys, added optional post-deploy vault configuration
  (`STELLAR_REGISTRY_ADDRESS`, `STELLAR_TREASURY_ADDRESS`, `STELLAR_FEE_BPS`,
  `STELLAR_MIN_DEPOSIT`), testnet overwrite guard (`--force`), mainnet
  mandatory registry check (`--allow-no-registry` opt-in), and portable
  self-locating paths.

### Fixed

- `donation-vault`: `set_treasury` rejects the vault's own address with
  `Error::InvalidTreasury`, so protocol fees can't be locked in the contract.
- `donation-vault`: `create_stream`, `top_up`, `cancel_stream`, and `settle`
  update stream state before external token transfers.
- `donation-vault`: `cancel_stream` returns `Error::StreamCancelled` early
  instead of rewriting already-cancelled streams, and suppresses duplicate
  `cancel` events when nothing moved.
- `donation-vault`: `settle`, `withdraw`, `withdraw_batch`, and
  `transfer_stream` mark streams `StreamStatus::Drained` when balance reaches
  zero, and `top_up` reactivates `Drained` streams back to
  `StreamStatus::Active`.
- `donation-vault`: `withdraw_batch` bounds batch size
  (`Error::BatchLimitExceeded`), rejects duplicate stream IDs
  (`Error::DuplicateStreamId`), returns `Error::NothingToWithdraw` when no
  stream pays out, and computes per-stream fees consistently with `withdraw`.
- `donation-vault`: `modify_rate` and `transfer_stream` return early as
  no-ops without emitting events when the new value matches the current one.
- `ngo-registry`: `NgoCount` is incremented on `register` and decremented on
  `unregister` and `remove_ngo` with checked arithmetic; `total_ngos` reads
  the same counter as `ngo_count`.
- `ngo-registry`: `update_name` only writes storage and emits `renamed` when
  the name actually changes.

[Unreleased]: https://github.com/StreamGive/streamgive-contracts/compare/main...HEAD
