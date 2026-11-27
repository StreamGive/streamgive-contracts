# Changelog

All notable changes to this repository's contracts will be documented in
this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project does not yet follow a formal versioning scheme — each contract's
`Cargo.toml` still reads `0.1.0` — so entries are grouped under
[Unreleased] until the first tagged release.

## [Unreleased]

### Fixed

- `ngo-registry`: restore the build. Merges had dropped the definitions behind the
  timelocked upgrade, owner transfer, category tags and unique names; they are back,
  with the error codes that collided on 16 renumbered after `RegistrationsPaused`:
  `NoPendingUpgrade` (17), `UpgradeDelayNotElapsed` (18), `UpgradeTimelockEnabled`
  (19), `InvalidCategories` (20) and `NameTaken` (21). `accept_owner_transfer` now
  moves the name claims and any revocation to the new owner, and a duplicated
  `verified_count` decrement left in `delete_ngo_entry` is gone. Repairs the registry tests
  the same merges truncated, and adds the missing entry points to `docs/ROLES.md`.

### Changed

- `ngo-registry`: split the 4,500-line `src/test.rs` into `src/tests/` modules for
  registration, verification, admin and paging, with shared fixtures in
  `tests/helpers.rs`. Same 244 tests; no behaviour change. See issue #395.
- `donation-vault`: split `lib.rs` into `events.rs`, `types.rs`, and `storage.rs` modules. No behaviour change. See issue #393.

### Added

- `donation-vault`: read-only `preview_stream` view returning the estimated
  stream duration, the effective fee, and the first validation error before a
  stream is created. Shares its token/amount and configured-limit checks with
  `create_stream`, so the preview cannot drift (#438).
- `ngo-registry`: `VerificationStatus` enum and `verification_status(owner)` view that distinguishes Unknown/Pending/Verified/Expired/Revoked. Closes #448.

### Added

- `ngo-registry`: admin-gated `pause_registrations` / `resume_registrations`
  switch and `registrations_paused` view to block new `register` calls with
  `Error::RegistrationsPaused` (code 16) while keeping all other entry points
  operational. Emits `regrpause` and `regresume` events.
- `donation-vault`: bounded `cancel_streams` entry point so a donor can cancel
  several streams in one transaction, with one authorization and one refund
  per stream. Rejects foreign or already-cancelled streams and is
  all-or-nothing. Adds `Error::NotStreamOwner` (code 40).
- `donation-vault`: `set_registry` now probes the candidate with `ngo_count`
  and rejects addresses that are not an NGO registry with the new append-only
  `Error::InvalidRegistry` (code 45), leaving the previously configured
  registry in place (#406).
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
  cancelled streams, with TTL coverage for indexing after cancellation.
- `donation-vault`: read-only `pending_accrual` view.
- `donation-vault`: tests that nothing accrues while a stream is paused and that
  accrual restarts from the resume time. `pending_accrual` and `pending_payout`
  now return zero for a paused stream, matching what `settle` pays.
- `donation-vault`: `pause_stream` now returns `Error::StreamDrained` (43) for a
  drained stream and `Error::StreamEmpty` (44) for an empty one instead of
  `StreamCancelled`, which is kept for real cancellations only.
- `donation-vault`: regression coverage for one-stroop-per-second streams and
  their zero-rounded protocol fee.
- `DataKey` enums now derive `Debug` in both contracts for clearer storage-key
  diagnostics.
- `ngo-registry`: contract skeleton with storage types and `init`.
- `ngo-registry`: NGO application/registration via `register`.
- `ngo-registry`: admin-gated `approve_ngo`.
- `ngo-registry`: admin-gated `revoke_ngo`.
- `ngo-registry`: two-step admin transfer via `propose_admin` /
  `accept_admin`.
- `ngo-registry`: owner-gated `update_name` for fixing an application's
  name before approval; rejected with `Error::AlreadyVerified` after.
- `ngo-registry`: read-only `ngo_count` getter for total registered NGOs.
- `ngo-registry`: optional owner-controlled metadata URI on an NGO entry.
  `set_metadata_uri` (owner-auth-gated) sets or clears it, `metadata_uri`
  reads it back, and it is included in `get_ngo`'s `Ngo`. Bounded to
  `MAX_METADATA_URI_LEN` (512 bytes); a longer value is rejected with
  `Error::MetadataUriTooLong` (14). Emits a `metadata` event. Unlike the
  name, it stays editable after approval, so an NGO's website/logo reference
  can be kept current (#347).
- `ngo-registry`: optional verification expiry. `set_verify_period` (admin)
  sets a period in seconds that `approve_ngo` / `batch_approve` add to the
  current time to stamp `Ngo.verified_until`; `verify_period` reads it back.
  `renew_ngo` (admin) moves an entry's expiry forward from now. `is_verified`
  and `get_ngo` treat a lapsed verification as unverified, while the stored
  flag is kept so the entry can be renewed. A `renew` event carries the new
  expiry and a `verperiod` event the new period; renewing with no period
  configured fails with `Error::NoVerifyPeriod` (15) (#348).
- `donation-vault`: optional minimum stream duration. `set_min_duration` (admin)
  sets the shortest time to depletion, in seconds, that `create_stream` and
  `modify_rate` accept; `min_duration` reads it back and it is included in
  `get_config`. A deposit that would drain sooner is rejected with
  `Error::StreamTooShort` (39), distinct from `InvalidAmount`. Emits a `mindur`
  event. Off by default (`0`) (#340).
- `donation-vault`: `withdraw_all(ngo, start, limit)` settles an NGO's streams
  a bounded page at a time straight from the NGO's stream index, so a caller
  never has to list stream ids off-chain or know when it has seen them all. It
  returns a `WithdrawAllPage { amounts, next }` — the net payout per stream in
  page order and the cursor to pass as `start` next time; iteration is complete
  once `next` reaches the NGO's stream count. `limit` is clamped to
  `MAX_BATCH_WITHDRAW_STREAMS`, and a page that has accrued nothing yields one
  `0` per stream and still advances the cursor rather than failing (#342).
- `donation-vault`: read-only `pending_refund(stream_id)` view for donors. It
  returns the refund a `cancel_stream` would make right now — the stream's
  remaining balance minus what has accrued to the NGO — applying the same rules
  as `cancel_stream` (cliff and paused streams accrue nothing). A drained or
  already-cancelled stream reports `0`. It is the donor's mirror of
  `pending_payout` and never moves funds (#345).
- `donation-vault`: the `drained` event. When settling a stream empties its
  balance and flips its status to `Drained`, the vault emits
  `DrainedEvent { stream_id, total_paid }` — the cumulative amount paid to the
  NGO over the stream's life. Covers `withdraw`, `withdraw_batch` (once per
  emptied stream), and the settle-driven `modify_rate`, `transfer_stream`, and
  `pause_stream` paths. `cancel_stream` and `top_up` settle only to reclassify
  a stream and stay silent, so an indexer sees the terminal transition once,
  without recomputing it from `withdraw` events (#344).
- Contract events for registry and vault state changes (see
  [`docs/EVENTS.md`](docs/EVENTS.md)).
- Document storage TTL policy, inactivity expiry risks, and keep-alive entry
  points (`extend_stream` and `touch_ngo`) in `README.md` and `docs/STORAGE.md`.
- `scripts/deploy-testnet.sh` for deploying both contracts to testnet.
- `scripts/deploy-mainnet.sh` for deploying to mainnet. It requires
  `--confirm`, pins the Public network passphrase, requires an explicit
  admin address, refuses to overwrite an existing entry, and records the
  contract IDs under a separate `mainnet` key in `deployments.json`.
  `deploy-testnet.sh` now preserves that key when it rewrites the file.
- CI workflow running `cargo fmt --check`, `cargo clippy`, a
  `wasm32v1-none` release build, and `cargo test --workspace`.
- README FAQ covering the license, release overflow checks, `no_std`, storage
  TTLs, and cancelled-stream retention.
- `docs/THREAT_MODEL.md`: assets, actors and trust boundaries, security
  invariants, and per-contract threat tables (authorization and accounting,
  availability and storage economics, registry identity, privilege/deployment
  and off-chain), plus a residual-risk register of open items.
- `docs/UPGRADE_POLICY.md`: error codes are append-only, how storage-layout
  changes are handled across an `upgrade`, and the process for announcing
  breaking changes (#314).
- `donation-vault`: `Error::InvalidNgo` (26) and `Error::InvalidToken` (27).
- `donation-vault`: per-NGO payout address via `set_payout_address` /
  `payout_address`, so an NGO can route `withdraw` / `withdraw_batch` payouts
  to a treasury wallet while its owner key only authorises. Owner-auth-gated,
  emits `payoutset`, and reported on the `withdraw` event.
- `donation-vault`: optional per-second rate ceiling via `set_max_rate` /
  `clear_max_rate` / `max_rate`. Off by default; when set, `create_stream`
  and `modify_rate` reject rates above it with `Error::RateTooHigh` (#339).
  Admin-gated, emits `maxrate` / `maxrtclr`, and reported by `get_config`.
- `donation-vault`: `create_stream_with_start` for a stream that stays idle
  behind a cliff until its `start` timestamp, so scheduled or trial-period
  pledges don't accrue from the ledger of creation. The deposit is still
  pulled up front; `pending_accrual` stays 0 before `start`, `depletion_time`
  is measured from `start`, and cancelling before `start` refunds the whole
  deposit. A `start` more than `MAX_START_DELAY_SECONDS` (one year) ahead is
  rejected with `Error::StartTooFar` (#337).
- `ngo-registry`: admin-managed approver list so review work can be delegated
  without sharing the admin key. `add_approver` / `remove_approver` (admin-gated)
  manage the list, `is_approver` / `list_approvers` expose it, and `approve_ngo`,
  `batch_approve`, and `revoke_ngo` now accept the admin or any listed approver as
  the acting address (`Error::NotApprover` otherwise). Adds `Error::NotApprover`
  (12) and `Error::AlreadyApprover` (13), emits `apadd` / `aprem`, and backs the
  new `DataKey::Approvers` instance key (#349).
- `ngo-registry`: `reject_ngo(approver, ngo_owner, reason_code)` so a reviewer can
  decline a pending application and tell the applicant why: it removes the entry
  (freeing the owner to register again) and emits `rejected` carrying the opaque,
  caller-defined `reason_code`. Like the other review entry points it is callable
  by the admin or a listed approver (`Error::NotApprover` otherwise), and it
  rejects an already-verified NGO with `Error::AlreadyVerified` — an approval is
  undone with `revoke_ngo` first (#350).
- `ngo-registry`: `list_ngos_page(start, limit)` returns a page of owner
  addresses alongside `next`, the index to resume from, so clients can page
  through the registry without recomputing the cursor and can tell the final
  page from a full one. `list_ngos` is unchanged (#351).
- `ngo-registry`: `owner_by_name(name)` looks up an NGO's owner address by its
  exact name through an index kept in sync on `register`, `update_name`,
  `unregister`, `remove_ngo` and `reject_ngo`. Entries registered before this
  change are not indexed until they rename (#447).
- `ngo-registry`: `verified_count()` getter so dashboards can read the number of
  verified NGOs without listing the registry. The count is backed by a
  `DataKey::VerifiedCount` instance key kept in step by every transition:
  `approve_ngo` / `batch_approve` increment it, `revoke_ngo` decrements it, and
  removing an entry decrements it when the entry was verified. `ngo_count` is
  unchanged and still reports the total number of entries (#352).
- `donation-vault`: two-step, timelocked upgrade. `set_upgrade_delay(delay)`
  configures the warning period (admin-gated, emits `updelay`); `propose_upgrade`
  records a pending Wasm hash plus its `executable_at` timestamp (admin-gated,
  emits `uppropose`); `execute_upgrade` carries it out, emitting `upgrade`
  (admin-gated, fails with `Error::NoPendingUpgrade` when nothing is proposed and
  `Error::UpgradeDelayNotElapsed` before the delay); and `cancel_upgrade`
  withdraws the proposal (admin-gated, emits `upcancel`, fails with
  `Error::NoPendingUpgrade` when nothing is proposed). Read-only `upgrade_delay()`
  and `pending_upgrade()` expose the configured delay and the pending proposal.
  Donors holding open streams now get a warning window before the bytecode
  changes (#343).
- `donation-vault`: the vault now tracks a committed balance per token — the sum
  of stream balances it holds on behalf of streams — kept in step by every
  fund-moving path (`create_stream` / `top_up` add, `withdraw`,
  `withdraw_batch`, `cancel_stream`, `rescue_stream`, `modify_rate`,
  `pause_stream`, `resume_stream` and `transfer_stream` subtract). Admin-gated
  `sweep_surplus(token, amount)` moves only the surplus above that committed
  balance to the treasury, so tokens sent to the vault by mistake can be
  recovered without ever touching stream funds; it emits `sweep` and fails with
  `Error::NoTreasury` (no treasury configured) or
  `Error::SweepExceedsSurplus` (more than the surplus). Read-only
  `committed_balance()` and `surplus()` expose the two figures (#346).
  Storage note: `DataKey::CommittedBalance` is a new key, so it is absent (zero)
  on any pre-existing deployment. Upgrading a vault that already holds open
  streams therefore requires a one-time backfill of each token's committed
  balance (the sum of its streams' `balance`) before `sweep_surplus` is used, or
  the whole token balance would read as sweepable surplus. No such deployment
  exists yet, so no migration entry point is added.
- `donation-vault`: admin-gated `sweep_surplus_to(token, amount, destination)`
  recovers misplaced tokens to an explicit address without requiring a
  configured treasury. It applies the same committed-balance ceiling as
  `sweep_surplus`, rejects the vault itself with
  `Error::InvalidSweepDestination` (code 40), and emits `sweep` naming the
  destination (#439).
- `donation-vault`: read-only `active_streams_by_ngo(ngo, start, limit)` pages
  through an NGO's live streams only, skipping `Cancelled` and `Drained` ones.
  It returns an `ActiveStreamsPage` with the ids plus the `next` position to
  continue from, so callers can enumerate open positions without fetching
  finished streams (#440).

### Changed

- `donation-vault`: the `sweep` event's final data field is now named
  `destination` instead of `treasury`, since `sweep_surplus_to` can send the
  surplus anywhere. Positional XDR encoding is unchanged, so existing indexers
  keep decoding it (#439).
- **BREAKING** `ngo-registry`: `approve_ngo`, `batch_approve`, and `revoke_ngo`
  now take the acting address as a leading argument
  (`approve_ngo(approver: Address, ngo_owner: Address)`,
  `batch_approve(approver: Address, owners: Vec<Address>)`,
  `revoke_ngo(approver: Address, ngo_owner: Address)`) and require the caller to
  be the admin or a listed approver. Integrators calling these entry points must
  pass the authorising address and update their invocation arguments (#349).
- **BREAKING** `ngo-registry`: the `Ngo` struct gains a
  `metadata_uri: Option<String>` field, so the value returned by `get_ngo`
  changes shape. Off-chain decoders of `Ngo` must add the new field (#347).
- **BREAKING** `ngo-registry`: the `Ngo` struct also gains a
  `verified_until: Option<u64>` field, so `get_ngo`'s return shape changes
  again. Off-chain decoders of `Ngo` must add the new field. Verification is
  now time-bounded when a period is configured, and a lapsed verification is
  reported by `get_ngo`/`is_verified` as unverified (#348).
- **BREAKING** `donation-vault`: the `withdraw` event data payload now includes
  the payout destination, changing from `(accrued: i128, fee: i128, net: i128)`
  to `(accrued: i128, fee: i128, net: i128, payout: Address)`. Off-chain
  indexers and subscribers must update their `withdraw` event decoders.
- **BREAKING** `donation-vault`: updated event data payloads to include net, fee,
  and settled amounts. `withdraw` now emits `(accrued: i128, fee: i128, net: i128)`
  instead of single-value `accrued`; `topup` now emits `(amount: i128, settled: i128)`
  instead of single-value `amount`; `ratemod` now emits `(old_rate: i128, new_rate: i128, settled: i128)`
  instead of `(old_rate, new_rate)`; and `transfer` emits `(old_ngo: Address, new_ngo: Address, settled: i128)`
  instead of `(old_ngo, new_ngo)`. Off-chain indexers and subscribers must update
  their event decoders for these new data shapes.
- `donation-vault`: `get_config` returns an extended `Config` struct that includes
  `min_deposit`, `max_streams_per_donor`, `registry`, `cancel_grace_ledgers`,
  `total_donated`, `pending_admin`, and `renounced`. Clients decoding the old
  struct shape will need updating.
- **BREAKING** `donation-vault`: the `Config` returned by `get_config` gains a
  `max_rate: Option<i128>` field (the per-second rate ceiling, `None` when
  unset). Clients decoding the `Config` struct must add the new field (#339).
- **BREAKING** `donation-vault`: the `Stream` struct gains a `start: u64` field
  (the cliff timestamp, equal to `created_at` for a plain `create_stream`).
  Off-chain decoders of the stored `Stream` record must add the new field
  (#337).
- **BREAKING** `donation-vault`: the `Stream` struct gains a `memo: Bytes`
  field (the donor's optional tag, empty when none was supplied with
  `create_stream`), and the `created` event data payload gains a trailing
  `memo: Bytes`. Off-chain decoders of the stored `Stream` record and of the
  `created` event must add the new field (#341).
- `donation-vault`: stream `balance`/`withdrawn` updates and the stream-id
  counter use checked arithmetic, returning `Error::ArithmeticOverflow`
  instead of panicking on overflow.
- `donation-vault`: `create_stream` rejects a stream whose `donor` and `ngo`
  are the same address with `Error::SelfStream`, so a deposit can't be
  counted as a committed donation while streaming straight back to its donor.
- `donation-vault`: the immediate `upgrade` entry point now refuses to run while
  a non-zero `upgrade_delay` is configured, returning
  `Error::UpgradeTimelockEnabled` instead. Upgrades must then go through
  `propose_upgrade` / `execute_upgrade`; setting the delay back to zero via
  `set_upgrade_delay(0)` explicitly re-enables the immediate path (#343).
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

- `donation-vault`: renumbered `Error::StreamAlreadyPaused` (was 26) and
  `Error::StreamNotPaused` (was 27) to 32 and 33. Their old discriminants
  collided with `Error::InvalidNgo` (26) and `Error::InvalidToken` (27),
  which failed to compile (`E0081: discriminant value assigned more than
  once`). Per the append-only policy the newer variants move; `InvalidNgo`
  and `InvalidToken` keep their numbers. Any client matching on the old
  numbers for the two pause variants must update.
- `donation-vault`: `modify_rate` re-verifies the NGO with the configured
  registry when the rate is raised, matching `top_up` and `transfer_stream`,
  and fails with `Error::NgoNotVerified` for a revoked NGO. Lowering the rate
  is still allowed (#330).
- `donation-vault`: `create_stream` rejects the vault's own address as the
  NGO (`Error::InvalidNgo`) or the token (`Error::InvalidToken`) before any
  state is written (#331).
- `donation-vault`: `withdraw` and `withdraw_batch` return
  `Error::ArithmeticOverflow` if the fee/net split cannot be computed,
  instead of reporting a fee of zero (#332).
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
