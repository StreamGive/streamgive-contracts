// SPDX-License-Identifier: Apache-2.0
//! Donation vault contract for the StreamGive protocol.
//!
//! Accepts token deposits from donors and releases them to NGOs as a
//! continuous per-second stream. Supports stream creation, withdrawal,
//! cancellation, top-up, rate modification, transfer, and an admin-gated
//! emergency rescue path.
#![no_std]
#![allow(missing_docs)]
use soroban_sdk::{
    contract, contractclient, contracterror, contractevent, contractimpl, contracttype,
    symbol_short, token, Address, Bytes, BytesN, Env, IntoVal, Map, String, Vec,
};

use common::{
    INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD, PERSISTENT_BUMP_AMOUNT,
    PERSISTENT_LIFETIME_THRESHOLD,
};

/// Event emitted when a new admin is proposed.
#[contractevent(topics = ["propadmin"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct ProposedAdminEvent {
    pub admin: Address,
}

/// Event emitted when a proposed admin accepts the role.
#[contractevent(topics = ["acptadmin"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct AcceptedAdminEvent {
    pub admin: Address,
}

/// Event emitted when a pending admin proposal is cancelled.
#[contractevent(topics = ["canceladm"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct CancelledAdminEvent {
    pub data: (),
}

/// Event emitted when the admin permanently renounces control.
#[contractevent(topics = ["renounce"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct RenouncedAdminEvent {
    pub data: (),
}

/// Event emitted when the vault is paused.
#[contractevent(topics = ["pause"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct PausedEvent {
    pub data: (),
}

/// Event emitted when the vault is unpaused.
#[contractevent(topics = ["unpause"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct UnpausedEvent {
    pub data: (),
}

/// Event emitted when a new stream is created.
#[contractevent(topics = ["created"], data_format = "vec")]
#[allow(missing_docs)]
pub struct CreatedEvent {
    #[topic]
    pub stream_id: u64,
    pub donor: Address,
    pub ngo: Address,
    pub token: Address,
    pub deposit: i128,
    pub rate: i128,
    /// The donor's optional tag for this stream (issue #341), empty when none
    /// was supplied.
    pub memo: Bytes,
}

/// Event emitted on each withdrawal from a stream.
#[contractevent(topics = ["withdraw"], data_format = "vec")]
#[allow(missing_docs)]
pub struct WithdrawnEvent {
    #[topic]
    pub stream_id: u64,
    pub accrued: i128,
    pub fee: i128,
    pub net: i128,
    /// Where the net payout was actually sent: the NGO's configured payout
    /// address if one is set, otherwise the NGO owner address itself.
    pub payout: Address,
}

/// Emitted once when a stream's balance reaches zero and its status becomes
/// `Drained` (issue #344). Carries the stream id and the cumulative amount
/// paid to the NGO over the stream's life, so an indexer can mark the stream
/// finished without recomputing the transition from `withdraw` events.
#[contractevent(topics = ["drained"], data_format = "vec")]
#[allow(missing_docs)]
pub struct DrainedEvent {
    #[topic]
    pub stream_id: u64,
    /// The stream's `withdrawn` total at the moment it emptied.
    pub total_paid: i128,
}

/// Emitted by `set_memo` when a donor replaces a stream's tag (issue #441).
/// Carries both values so an indexer can update its copy without keeping the
/// old memo itself.
#[contractevent(topics = ["memoset"], data_format = "vec")]
pub struct MemoSetEvent {
    #[topic]
    pub stream_id: u64,
    pub old_memo: Bytes,
    pub new_memo: Bytes,
}

/// Event emitted when a stream is cancelled.
#[contractevent(topics = ["cancel"], data_format = "vec")]
#[allow(missing_docs)]
pub struct CancelledStreamEvent {
    #[topic]
    pub stream_id: u64,
    pub accrued: i128,
    pub refund: i128,
}

/// Event emitted when a stream is topped up with additional funds.
#[contractevent(topics = ["topup"], data_format = "vec")]
#[allow(missing_docs)]
pub struct ToppedUpEvent {
    #[topic]
    pub stream_id: u64,
    pub amount: i128,
    pub settled: i128,
}

/// Event emitted when a stream's per-second rate is modified.
#[contractevent(topics = ["ratemod"], data_format = "vec")]
#[allow(missing_docs)]
pub struct RateModifiedEvent {
    #[topic]
    pub stream_id: u64,
    pub old_rate: i128,
    pub new_rate: i128,
    pub settled: i128,
}

/// Event emitted when a stream is transferred to a new NGO.
#[contractevent(topics = ["transfer"], data_format = "vec")]
#[allow(missing_docs)]
pub struct TransferStreamEvent {
    #[topic]
    pub stream_id: u64,
    pub old_ngo: Address,
    pub new_ngo: Address,
    pub settled: i128,
}

/// Event emitted when a stream is paused by the donor.
#[contractevent(topics = ["strmpause"], data_format = "vec")]
#[allow(missing_docs)]
pub struct PausedStreamEvent {
    #[topic]
    pub stream_id: u64,
    pub settled: i128,
}

/// Event emitted when a paused stream is resumed by the donor.
#[contractevent(topics = ["strmresume"], data_format = "vec")]
#[allow(missing_docs)]
pub struct ResumedStreamEvent {
    #[topic]
    pub stream_id: u64,
}

/// Event emitted when the treasury address is set.
#[contractevent(topics = ["treasset"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct TreasurySetEvent {
    pub treasury: Address,
}

/// Event emitted when the global fee basis points are configured.
#[contractevent(topics = ["feeset"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct FeeBpsSetEvent {
    pub fee_bps: u32,
}

/// Event emitted when a per-token fee override is set.
#[contractevent(topics = ["tokfeeset"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct TokenFeeBpsSetEvent {
    #[topic]
    pub token: Address,
    pub fee_bps: u32,
}

/// Event emitted when a per-token fee override is cleared.
#[contractevent(topics = ["tokfeeclr"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct TokenFeeBpsClearedEvent {
    #[topic]
    pub token: Address,
    pub data: (),
}

/// Event emitted when the per-donor stream cap is configured.
#[contractevent(topics = ["maxstrm"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct MaxStreamsPerDonorSetEvent {
    pub limit: u64,
}

/// Event emitted when the maximum per-second rate is set.
#[contractevent(topics = ["maxrate"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct MaxRateSetEvent {
    pub max_rate: i128,
}

/// Event emitted when the maximum per-second rate ceiling is cleared.
#[contractevent(topics = ["maxrtclr"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct MaxRateClearedEvent {
    pub data: (),
}

/// Event emitted when the maximum deposit per stream is configured.
#[contractevent(topics = ["maxdep"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct MaxDepositPerStreamSetEvent {
    pub max_deposit: i128,
}

/// Event emitted when the maximum deposit per stream is cleared.
#[contractevent(topics = ["maxdepclr"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct MaxDepositPerStreamClearedEvent {
    pub data: (),
}

/// Event emitted when the minimum stream duration is configured.
#[contractevent(topics = ["mindur"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct MinDurationSetEvent {
    pub min_duration: u64,
}

/// Event emitted when a stream is rescued by the admin during a pause.
#[contractevent(topics = ["rescue"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct RescuedStreamEvent {
    #[topic]
    pub stream_id: u64,
    pub refund: i128,
}

/// Event emitted when the treasury address is cleared.
#[contractevent(topics = ["treclr"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct TreasuryClearedEvent {
    pub data: (),
}

/// Event emitted when the minimum deposit is configured.
#[contractevent(topics = ["mindep"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct MinDepositSetEvent {
    pub min_deposit: i128,
}

/// Event emitted when the cancelled-stream retention window is configured.
#[contractevent(topics = ["grceset"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct CancelGraceLedgersSetEvent {
    pub grace_ledgers: u32,
}

/// Event emitted when the NGO registry address is set.
#[contractevent(topics = ["regset"], data_format = "vec")]
#[allow(missing_docs)]
pub struct RegistrySetEvent {
    pub registry: Address,
    pub previous_registry: Option<Address>,
}

/// Event emitted when the NGO registry address is cleared.
#[contractevent(topics = ["regclr"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct RegistryClearedEvent {
    pub data: (),
}

/// Event emitted when an NGO configures a payout address.
#[contractevent(topics = ["payoutset"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct PayoutAddressSetEvent {
    #[topic]
    pub ngo: Address,
    pub payout: Address,
}

/// Event emitted when the contract is initialized.
#[contractevent(topics = ["init"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct InitializedEvent {
    pub admin: Address,
}

/// Event emitted when the contract is upgraded.
#[contractevent(topics = ["upgrade"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct UpgradedEvent {
    pub new_wasm_hash: BytesN<32>,
}

/// Emitted by `execute_upgrade` with the full configuration in force as the
/// upgraded code takes over (issue #443). New code may introduce settings that
/// take their defaults silently; this gives indexers an explicit baseline
/// for the new contract version instead of leaving them to infer it.
#[contractevent(topics = ["config_snapshot"], data_format = "single-value")]
pub struct ConfigSnapshotEvent {
    pub config: Config,
}

/// Event emitted when a timelocked upgrade is proposed.
#[contractevent(topics = ["uppropose"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct UpgradeProposedEvent {
    #[topic]
    pub new_wasm_hash: BytesN<32>,
    pub executable_at: u64,
}

/// Event emitted when a pending upgrade is cancelled.
#[contractevent(topics = ["upcancel"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct UpgradeCancelledEvent {
    pub new_wasm_hash: BytesN<32>,
}

/// Event emitted when the upgrade timelock delay is configured.
#[contractevent(topics = ["updelay"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct UpgradeDelaySetEvent {
    pub delay: u64,
}

/// Emitted by `sweep_surplus` / `sweep_surplus_to` when the admin moves
/// tokens that were sent to the vault by mistake (above the committed balance
/// for that token) to an explicit destination. See issues #346 and #439.
#[contractevent(topics = ["sweep"], data_format = "vec")]
#[allow(missing_docs)]
pub struct SurplusSweptEvent {
    #[topic]
    pub token: Address,
    pub amount: i128,
    pub destination: Address,
}

/// A contract upgrade recorded by `propose_upgrade`, awaiting the timelock
/// before `execute_upgrade` may carry it out. See issue #343.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PendingUpgrade {
    /// The Wasm hash the contract will be upgraded to on execution.
    pub new_wasm_hash: BytesN<32>,
    /// Ledger timestamp at or after which `execute_upgrade` may run.
    pub executable_at: u64,
}

mod math;

/// Mirrors ngo-registry's `Ngo` record for cross-contract calls. Declared
/// locally rather than imported from the `ngo-registry` crate: depending on
/// its source directly would pull that crate's own `#[contractimpl]` exports
/// into this contract's Wasm link unit, colliding with this contract's
/// identically-named entry points (`admin`, `init`, `upgrade`, ...).
#[contracttype]
#[derive(Clone, Debug)]
pub struct NgoRecord {
    /// The address that registered and owns this NGO entry.
    pub owner: Address,
    /// The display name of the NGO.
    pub name: String,
    /// Whether the NGO has been verified in the registry.
    pub verified: bool,
    pub metadata_uri: Option<String>,
    pub verified_until: Option<u64>,
}

/// Thin cross-contract interface onto the configured ngo-registry contract.
/// Only the one method `create_stream` needs. See `NgoRecord` for why this
/// isn't just imported from the `ngo-registry` crate.
#[contractclient(name = "NgoRegistryClient")]
#[allow(dead_code)]
trait NgoRegistryInterface {
    fn get_ngo(env: Env, owner: Address) -> NgoRecord;
    fn ngo_count(env: Env) -> u64;
}

/// A single donor -> NGO streaming donation.
///
/// `balance` is the undrawn amount still deposited in the vault; `rate` is
/// how much of it accrues to the NGO per second. `created_at` is set once,
/// by `create_stream`, and never changes; `last_update` moves forward on
/// every checkpoint (withdraw, cancel, top-up, or rate change).
#[contracttype]
// Debug and PartialEq let tests assert_eq! on a try_* callΓÇÖs full
// Result<Result<Stream, _>, _> rather than unwrapping it by hand first,
// and compare a whole stream at once instead of field by field.
#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    /// The address that opened this stream and may cancel or top it up.
    pub donor: Address,
    /// The address receiving the accruing payout.
    pub ngo: Address,
    /// The token being streamed.
    pub token: Address,
    /// Accrual amount per second, in token units.
    pub rate: i128,
    /// Undrawn amount remaining in the vault for this stream.
    pub balance: i128,
    /// Cumulative gross amount paid out from this stream so far.
    pub withdrawn: i128,
    /// Ledger timestamp at which this stream was created.
    pub created_at: u64,
    /// When this stream begins paying out. Equal to `created_at` for a plain
    /// `create_stream`; later than it for `create_stream_with_start`. No time
    /// before this timestamp accrues, so a cliff stream stays idle until it
    /// arrives. See issue #337.
    pub start: u64,
    /// Ledger timestamp of the last checkpoint for this stream.
    pub last_update: u64,
    /// Optional free-form tag chosen by the donor at creation, e.g. a campaign
    /// id, capped at `MAX_MEMO_BYTES` bytes. Empty means no memo was set.
    /// See issue #341.
    pub memo: Bytes,
    /// Explicit lifecycle state. Set to Active at creation, Cancelled on
    /// cancel_stream, and Drained when withdraw brings balance to zero.
    pub status: StreamStatus,
    /// Set once by `cancel_stream`, never unset. Distinguishes a cancelled
    /// stream (`rate == 0`, `balance == 0`) from one that simply ran dry
    /// and was withdrawn in full (`balance == 0` but `rate` unchanged).
    pub cancelled: bool,
}

/// Explicit lifecycle state of a stream. Prior to this field a client had
/// to inspect `rate`, `balance`, and `withdrawn` together to infer state;
/// the enum makes it queryable directly. See issue #92.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StreamStatus {
    /// Normal state: rate > 0, balance > 0.
    Active,
    /// Donor cancelled. Balance and rate are both zero.
    Cancelled,
    /// The stream ran to completion: the last withdrawal brought balance
    /// to zero without a cancel.
    Drained,
    /// Donor-paused. The rate and balance are retained, but elapsed time
    /// does not accrue until the donor resumes the stream.
    Paused,
}

/// Per-token totals across a donor's streams.
#[contracttype]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DonorTotals {
    /// Funds still held for the donor's streams, including unsettled accrual.
    pub committed: i128,
    /// Gross amount settled, including any protocol fee.
    pub withdrawn: i128,
}

/// All admin configuration and global contract parameters returned by `get_config`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Whether fund-moving operations are currently halted.
    pub paused: bool,
    /// The address receiving protocol fees, if configured.
    pub treasury: Option<Address>,
    /// The global protocol fee in basis points.
    pub fee_bps: u32,
    /// The minimum deposit accepted by `create_stream` and `top_up`.
    pub min_deposit: i128,
    /// The maximum number of streams a single donor may open.
    pub max_streams_per_donor: u64,
    /// The highest per-second `rate` accepted by `create_stream` and
    /// `modify_rate`, when an admin has configured one. `None` means no cap.
    pub max_rate: Option<i128>,
    /// The shortest time to depletion, in seconds, an admin will accept for a
    /// new or re-rated stream. `0` means no minimum is enforced. See
    /// `set_min_duration` and issue #340.
    pub min_duration: u64,
    /// The NGO registry contract used to verify recipients, if configured.
    pub registry: Option<Address>,
    /// Extra ledgers cancelled stream records are retained for indexing.
    pub cancel_grace_ledgers: u32,
    /// Lifetime total of all stream deposits and top-ups.
    pub total_donated: i128,
    /// Lifetime gross total paid out to NGOs.
    pub total_withdrawn: i128,
    /// Lifetime total protocol fees sent to the treasury.
    pub total_fees_collected: i128,
    /// The address proposed for the next admin transfer, if any.
    pub pending_admin: Option<Address>,
    /// Whether the admin has permanently renounced control.
    pub renounced: bool,
    /// The largest deposit or top-up balance a single stream may hold, when
    /// an admin has configured one. `None` means no ceiling is enforced.
    pub max_deposit_per_stream: Option<i128>,
}

/// A page of results from `withdraw_all` (issue #342): the net payout for each
/// stream in the page plus the cursor for the next page.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawAllPage {
    /// Net payout per stream in the page, in NGO-index order. A stream that
    /// had nothing to withdraw contributes `0`.
    pub amounts: Vec<i128>,
    /// The `start` to pass on the next call: one past the end of this page,
    /// clamped to the NGO's stream count. Iteration is complete once it
    /// reaches that count (see `streams_by_ngo`).
    pub next: u32,
}

/// One page of an NGO's still-live streams, returned by
/// `active_streams_by_ngo` (issue #440).
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ActiveStreamsPage {
    /// Live stream ids in creation order: `Active` and `Paused` streams.
    /// `Cancelled` and `Drained` streams are omitted.
    pub ids: Vec<u64>,
    /// The `start` to pass on the next call. It points past the last stream
    /// examined (or past the last id collected when `limit` filled the page
    /// first). Iteration is complete once it stops advancing.
    pub next: u32,
}

/// A read-only estimate of a new stream, returned by `preview_stream`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamPreview {
    /// Whole seconds `deposit` will take to stream at `rate`, rounded up so a
    /// partial final second counts. `None` when `deposit` or `rate` is not
    /// positive, or when the result does not fit in a `u64`.
    pub duration: Option<u64>,
    /// The fee in basis points that applies to `token`: its per-token
    /// override if one is configured, otherwise the global fee. Matches
    /// `token_fee_bps`.
    pub fee_bps: u32,
    /// The numeric code of the first error `create_stream` would return for
    /// the checks that only depend on `token`, `deposit` and `rate`
    /// (including the pause gate and configured limits), or `None` when those
    /// checks pass. Donor-, NGO-, auth- and registry-specific checks are
    /// outside the preview.
    pub error: Option<u32>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub enum DataKey {
    /// The address authorized to administer the vault.
    Admin,
    /// The administrator nominated to take over, pending acceptance.
    PendingAdmin,
    /// Set once `renounce_admin` is called; checked by `require_admin` so
    /// every admin-gated entry point fails with `Error::AdminRenounced`
    /// afterwards instead of the misleading `NotInitialized`.
    /// Set by `renounce_admin`. Distinguishes a vault whose admin permanently
    /// stepped down from one that was never initialized, and blocks `init`
    /// from installing a new admin afterwards.
    AdminRenounced,
    /// The next stream identifier to allocate.
    NextStreamId,
    /// A donation stream keyed by its numeric identifier.
    Stream(u64),
    /// Whether fund-moving operations are currently paused.
    Paused,
    /// The address that receives protocol fees, when configured.
    Treasury,
    /// The protocol fee in basis points.
    FeeBps,
    /// An optional protocol fee override in basis points for a token.
    TokenFeeBps(Address),
    /// Lifetime sum of successful stream deposits and top-ups.
    TotalDonated,
    /// Lifetime gross amount settled and paid out to NGOs.
    TotalWithdrawn,
    /// Lifetime protocol fees collected and sent to the treasury.
    TotalFeesCollected,
    /// Stream ids currently associated with an NGO.
    NgoStreams(Address),
    /// The token allowlist surfaced to frontend token pickers. See
    /// [`allowed_tokens`](DonationVault::allowed_tokens); empty until an
    /// operator configures one.
    AllowedTokens,
    /// Admin-settable per-donor stream cap. See `set_max_streams_per_donor`.
    MaxStreamsPerDonor,
    /// Count of streams a donor currently has open. Incremented on
    /// `create_stream`, never decremented (streams are cancelled, not
    /// deleted) ΓÇö so this is really a lifetime cap, not a live cap.
    DonorStreamCount(Address),
    /// Minimum permitted stream deposit amount.
    MinDeposit,
    /// Optional maximum per-second `rate` accepted by `create_stream` and
    /// `modify_rate`. Absent means "no cap".
    MaxRate,
    /// Additional ledgers to retain a cancelled stream for indexing.
    CancelGraceLedgers,
    /// Optional NGO registry contract used to verify NGOs before a stream
    /// is opened. Absent means "no registry check configured".
    Registry,
    /// The address an NGO has configured to receive withdrawals, keyed by the
    /// NGO owner address. Absent means payouts go to the owner address itself.
    NgoPayoutAddress(Address),
    /// Seconds that must elapse between `propose_upgrade` and
    /// `execute_upgrade`. Zero (or absent) means the timelock is disabled and
    /// the immediate `upgrade` entry point is allowed. See issue #343.
    UpgradeDelay,
    /// The upgrade recorded by `propose_upgrade` and not yet executed or
    /// cancelled. Absent means no upgrade is pending.
    PendingUpgrade,
    /// Per-token total of funds the vault holds on behalf of streams (the sum
    /// of stream balances for that token). Used by `sweep_surplus` to bound a
    /// sweep to the surplus above it. Absent means zero. See issue #346.
    CommittedBalance(Address),
    /// Shortest time to depletion, in seconds, that `create_stream` and
    /// `modify_rate` accept. Absent (or zero) means no minimum. See
    /// `set_min_duration` and issue #340.
    MinDuration,
    /// Indexed committed and gross withdrawn totals for a donor and token.
    DonorTotals(Address, Address),
    /// Present if every stream was created with donor totals indexing enabled.
    DonorTotalsIndexedFromStart,
    /// Optional maximum deposit (and post-top-up balance) per stream.
    /// Absent means "no ceiling". See `set_max_deposit_per_stream`.
    MaxDepositPerStream,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// The vault has already been initialized and cannot be initialized again.
    AlreadyInitialized = 1,
    /// The vault has not been initialized yet.
    NotInitialized = 2,
    /// The requested stream id does not exist.
    StreamNotFound = 3,
    /// A deposit, rate, or amount was zero or negative.
    InvalidAmount = 4,
    /// The stream has accrued nothing since its last checkpoint.
    NothingToWithdraw = 5,
    /// The vault is paused; fund-moving operations are blocked.
    ContractPaused = 6,
    /// The requested fee exceeds `MAX_FEE_BPS`.
    FeeTooHigh = 7,
    /// `accept_admin` or `cancel_admin_proposal` was called without a prior pending proposal.
    NoPendingAdmin = 8,
    /// A stream's `balance` or `withdrawn` (or the stream-id counter) would
    /// leave its type's range. Returned instead of letting the release
    /// profile's overflow checks panic and abort the transaction.
    ArithmeticOverflow = 9,
    /// The donor and the NGO are the same address, so the stream would pay
    /// the donor back their own deposit. Rejected at creation: a stream that
    /// nets to zero still counts as a committed donation in the indexer and
    /// on impact pages, which is a way to inflate those totals for free.
    SelfStream = 10,
    /// `set_treasury` was given the vault's own address. Fees paid there
    /// could never be moved out again.
    InvalidTreasury = 11,
    /// The vault is already paused.
    AlreadyPaused = 12,
    /// The vault is already unpaused.
    AlreadyUnpaused = 13,
    /// `deposit` was below the configured `min_deposit`.
    DepositTooLow = 14,
    /// `top_up` or `modify_rate` was called on a stream that `cancel_stream`
    /// has already closed out. A cancelled stream's rate and balance are
    /// zeroed for good; topping it up would just sit inert, and changing
    /// its rate would quietly revive a stream the backend already treats
    /// as terminal.
    StreamCancelled = 15,
    /// The proposed administrator is not a valid replacement.
    InvalidAdmin = 16,
    /// The donor already has `max_streams_per_donor` streams. Raised by
    /// `create_stream` before the deposit is pulled. See issue #94.
    StreamLimitExceeded = 17,
    /// The NGO address passed to `create_stream` is not verified in the
    /// configured ngo-registry. Only set when a registry address has been
    /// stored via `set_registry`.
    NgoNotVerified = 18,
    /// `NextStreamId` was missing from instance storage when `create_stream`
    /// tried to read it. `init` always sets it, so this should be
    /// unreachable in practice, but a missing counter must never be
    /// silently treated as `0` ΓÇö that could collide with an existing
    /// stream. Returned instead of defaulting.
    StreamCounterMissing = 19,
    /// The admin has renounced control, so admin-gated calls are permanently
    /// disabled.
    AdminRenounced = 20,
    /// `set_fee_bps` was called with a non-zero fee while no treasury is
    /// configured. Without this, the fee would be silently dropped by
    /// `compute_fee` (which returns 0 whenever no treasury is set,
    /// regardless of `fee_bps`) - the admin would believe revenue is
    /// accruing when it isn't, with no error or event to say otherwise.
    FeeRequiresTreasury = 21,
    /// `rescue_stream` was called while the vault is not paused. It only
    /// exists for incident response, not as an ordinary way to close a
    /// stream out.
    NotPaused = 22,
    /// Every stream in a batch withdrawal must target the same NGO.
    MixedNgo = 23,
    /// The number of stream IDs requested in `get_streams` exceeds `MAX_BATCH_GET_STREAMS`.
    BatchLimitExceeded = 24,
    /// `withdraw_batch` was called with duplicate stream IDs.
    DuplicateStreamId = 25,
    /// A paused stream cannot be withdrawn until it is resumed.
    StreamPaused = 28,
    /// `create_stream` was given the vault's own address as the NGO. Nobody
    /// can authorise as the vault, so the stream could never be withdrawn.
    InvalidNgo = 26,
    /// `create_stream` was given the vault's own address as the token. The
    /// vault is not a token contract, so the deposit could never move.
    InvalidToken = 27,
    /// `create_stream` or `modify_rate` was given a `rate` above the
    /// admin-configured `max_rate`. Only raised when an admin has set a
    /// maximum via `set_max_rate`.
    RateTooHigh = 29,
    /// `create_stream_with_start` was given a `start` more than
    /// `MAX_START_DELAY_SECONDS` ahead of the current ledger time.
    StartTooFar = 30,
    /// A creation entry point or `set_memo` was given a `memo` longer than
    /// `MAX_MEMO_BYTES`.
    MemoTooLong = 31,
    /// `pause_stream` was called on a stream that is already paused.
    StreamAlreadyPaused = 32,
    /// `resume_stream` was called on a stream that is not paused.
    StreamNotPaused = 33,
    /// `execute_upgrade` or `cancel_upgrade` was called with no upgrade
    /// proposed.
    NoPendingUpgrade = 34,
    /// `execute_upgrade` was called before the proposed upgrade's timelock
    /// elapsed.
    UpgradeDelayNotElapsed = 35,
    /// The immediate `upgrade` entry point was called while a non-zero
    /// `upgrade_delay` is configured: upgrades must go through
    /// `propose_upgrade` / `execute_upgrade`.
    UpgradeTimelockEnabled = 36,
    /// `sweep_surplus` was called before a treasury was configured, so the
    /// surplus has nowhere to go.
    NoTreasury = 37,
    /// `sweep_surplus` was asked to move more than the surplus above the
    /// token's committed balance, which would dip into stream funds.
    SweepExceedsSurplus = 38,
    /// A stream's deposit would deplete in fewer seconds than the
    /// admin-configured `min_duration`. Raised by `create_stream` and
    /// `modify_rate` before any funds move. Distinct from `InvalidAmount`,
    /// which only covers a zero or negative amount. See issue #340.
    StreamTooShort = 39,
    /// `cancel_streams` was given a stream belonging to a different donor.
    /// A batch is all-or-nothing, so a single foreign stream rejects the
    /// whole call rather than cancelling the caller's streams around it.
    NotStreamOwner = 40,
    /// `create_stream` or `top_up` was given a deposit that would push the
    /// stream's balance above the admin-configured `max_deposit_per_stream`.
    /// Only raised when an admin has set a ceiling via
    /// `set_max_deposit_per_stream`.
    DepositTooHigh = 41,
    /// `sweep_surplus` or `sweep_surplus_to` named the vault itself as the
    /// destination, which would move nothing.
    InvalidSweepDestination = 42,
    /// `pause_stream` was called on a stream that ran to completion
    /// (`Drained`). It was not cancelled: it paid out in full.
    StreamDrained = 43,
    /// `pause_stream` was called on a stream whose balance is zero but that is
    /// neither cancelled, drained nor paused, so there is nothing to pause.
    StreamEmpty = 44,
    /// `set_registry` was given an address that does not answer the
    /// registry's `ngo_count` probe: a plain account, the vault itself, or
    /// an unrelated contract. Storing it would make every registry check
    /// fail closed until the admin noticed and cleared it. See issue #403.
    InvalidRegistry = 45,
    // Error numbers are append-only: never renumber, reuse, or remove a
    // variant. New errors take the next unused number. See
    // docs/UPGRADE_POLICY.md.
}

/// Maximum number of streams that can be requested in a single `get_streams` call.
/// Chosen to stay well within Soroban's per-transaction ledger read footprint limit (40 reads).
pub const MAX_BATCH_GET_STREAMS: u32 = 40;

/// Maximum number of streams that can be withdrawn in a single `withdraw_batch` call.
/// Chosen to stay well within Soroban's per-transaction CPU and footprint budget.
pub const MAX_BATCH_WITHDRAW_STREAMS: u32 = 20;

/// Maximum number of streams that can be cancelled in a single `cancel_streams`
/// call. Matches `MAX_BATCH_WITHDRAW_STREAMS`: a batch cancel settles each
/// stream and refunds it, so it has the same per-stream CPU and footprint cost.
pub const MAX_BATCH_CANCEL_STREAMS: u32 = 20;

/// Maximum number of NGO stream ids returned by `streams_by_ngo` in one page.
pub const MAX_STREAMS_BY_NGO: u32 = 100;

const MAX_FEE_BPS: u32 = 1_000;

/// Default per-donor stream cap applied when `set_max_streams_per_donor`
/// has not been called. Chosen so an ordinary donor can open hundreds of
/// streams across many NGOs, but a flood attack hits the ceiling long
/// before it can bloat persistent storage. See issue #94.
const DEFAULT_MAX_STREAMS_PER_DONOR: u64 = 100;

/// Longest a `create_stream_with_start` cliff may be pushed into the future
/// (issue #337). A scheduled pledge or trial period needs a delayed start, but
/// an unbounded one is easy to mistype and would strand the deposit behind an
/// unreachable date. One year covers the realistic cases while keeping the
/// cliff human-scale.
const MAX_START_DELAY_SECONDS: u64 = 365 * 24 * 60 * 60;

/// Longest stream memo accepted by the create entry points (issue #341). A memo
/// is a short tag (a campaign id, say), not a payload ΓÇö 32 bytes is plenty and
/// keeps the stored record and `created` event small.
const MAX_MEMO_BYTES: u32 = 32;

/// Stream-specific bump and threshold amounts, using the 90-day persistent entry constants.
const STREAM_BUMP_AMOUNT: u32 = PERSISTENT_BUMP_AMOUNT;
const STREAM_LIFETIME_THRESHOLD: u32 = PERSISTENT_LIFETIME_THRESHOLD;

/// Keeps the contract instance (admin, config, next-id counter) from being
/// archived. Called on every state-changing entry point.
fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

/// Keeps a stream's persistent entry alive for 90 days past its last
/// touch, so a slow-draining stream doesn't get archived out from under
/// its donor and NGO between activity.
fn extend_stream_ttl(env: &Env, stream_id: u64) {
    env.storage().persistent().extend_ttl(
        &DataKey::Stream(stream_id),
        STREAM_LIFETIME_THRESHOLD,
        STREAM_BUMP_AMOUNT,
    );
}

/// Extends a cancelled stream beyond the normal retention period by the
/// configured indexing grace period.
///
/// The threshold is the extended window itself, not the ordinary stream
/// threshold: `load_stream` / `save_stream` may already have bumped the entry
/// to `STREAM_BUMP_AMOUNT`, which sits above the ordinary threshold and would
/// otherwise make this extension a no-op.
fn extend_cancelled_stream_ttl(env: &Env, stream_id: u64, grace_ledgers: u32) -> Result<(), Error> {
    let bump_amount = STREAM_BUMP_AMOUNT
        .checked_add(grace_ledgers)
        .ok_or(Error::ArithmeticOverflow)?;
    env.storage()
        .persistent()
        .extend_ttl(&DataKey::Stream(stream_id), bump_amount, bump_amount);
    Ok(())
}

/// Loads a stream from persistent storage, bumping its TTL on access so an
/// actively used stream cannot be archived out from under its donor or NGO.
fn load_stream(env: &Env, stream_id: u64) -> Result<Stream, Error> {
    let stream: Stream = env
        .storage()
        .persistent()
        .get(&DataKey::Stream(stream_id))
        .ok_or(Error::StreamNotFound)?;
    extend_stream_ttl(env, stream_id);
    Ok(stream)
}

/// Like [`load_stream`] for read paths where a missing id is a normal result
/// (batch lookups and index scans): returns `None` but still bumps the TTL of
/// every entry that exists.
fn try_load_stream(env: &Env, stream_id: u64) -> Option<Stream> {
    let stream: Stream = env
        .storage()
        .persistent()
        .get(&DataKey::Stream(stream_id))?;
    extend_stream_ttl(env, stream_id);
    Some(stream)
}

/// Saves a stream to persistent storage and bumps its TTL in the same step, so
/// no caller can write a stream and forget to keep its entry alive.
fn save_stream(env: &Env, stream_id: u64, stream: &Stream) {
    env.storage()
        .persistent()
        .set(&DataKey::Stream(stream_id), stream);
    extend_stream_ttl(env, stream_id);
}

/// Reads the configured admin and requires their auth, failing with
/// `Error::NotInitialized` if `init` hasn't been called yet. Shared by
/// every admin-gated entry point so the same three steps aren't repeated
/// at each call site.
fn admin_renounced(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::AdminRenounced)
        .unwrap_or(false)
}

fn require_admin(env: &Env) -> Result<Address, Error> {
    if admin_renounced(env) {
        return Err(Error::AdminRenounced);
    }
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::NotInitialized)?;
    admin.require_auth();
    Ok(admin)
}

/// Returns `Err(Error::ContractPaused)` if an admin has paused the vault.
/// Checked at the top of every fund-moving entry point.
fn require_not_paused(env: &Env) -> Result<(), Error> {
    let paused: bool = env
        .storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false);
    if paused {
        return Err(Error::ContractPaused);
    }
    Ok(())
}

/// Reads the admin-configured upgrade timelock, in seconds. Defaults to zero
/// (timelock disabled) when an admin has not set one.
fn get_upgrade_delay(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::UpgradeDelay)
        .unwrap_or(0)
}

/// Reads the amount of `token` the vault holds on behalf of streams. Absent
/// means zero. See issue #346.
fn get_committed(env: &Env, token: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::CommittedBalance(token.clone()))
        .unwrap_or(0)
}

/// Writes the committed balance for `token`, removing the entry entirely when
/// it reaches zero so storage isn't left holding a stale key.
fn set_committed(env: &Env, token: &Address, amount: i128) {
    let key = DataKey::CommittedBalance(token.clone());
    if amount == 0 {
        env.storage().persistent().remove(&key);
        return;
    }
    env.storage().persistent().set(&key, &amount);
    env.storage()
        .persistent()
        .extend_ttl(&key, INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

/// Adjusts the committed balance for `token` by `delta`, which may be negative
/// (a payout or refund leaving the vault). Checked so a broken invariant aborts
/// rather than wrapping. See issue #346.
fn add_committed(env: &Env, token: &Address, delta: i128) -> Result<(), Error> {
    let next = get_committed(env, token)
        .checked_add(delta)
        .ok_or(Error::ArithmeticOverflow)?;
    set_committed(env, token, next);
    Ok(())
}

// A missing aggregate can be from an older deployment. Rebuild it from the
// existing streams without writing on a view, then persist it on the next
// successful mutation for this donor and token.
fn read_donor_totals(
    env: &Env,
    key: &DataKey,
    donor: &Address,
    token: &Address,
) -> Result<DonorTotals, Error> {
    let storage = env.storage().persistent();
    if let Some(totals) = storage.get(key) {
        return Ok(totals);
    }

    let mut totals = DonorTotals::default();
    let instance = env.storage().instance();
    if instance.has(&DataKey::DonorTotalsIndexedFromStart) {
        return Ok(totals);
    }
    let total: u64 = instance.get(&DataKey::NextStreamId).unwrap_or(0);
    for id in 0..total {
        let Some(stream) = try_load_stream(env, id) else {
            continue;
        };
        if stream.donor == *donor && stream.token == *token {
            totals = adjust_donor_totals(totals, stream.balance, stream.withdrawn)?;
        }
    }
    Ok(totals)
}

fn adjust_donor_totals(
    totals: DonorTotals,
    committed_delta: i128,
    withdrawn_delta: i128,
) -> Result<DonorTotals, Error> {
    Ok(DonorTotals {
        committed: totals
            .committed
            .checked_add(committed_delta)
            .ok_or(Error::ArithmeticOverflow)?,
        withdrawn: totals
            .withdrawn
            .checked_add(withdrawn_delta)
            .ok_or(Error::ArithmeticOverflow)?,
    })
}

fn update_donor_totals(
    env: &Env,
    donor: &Address,
    token: &Address,
    committed_delta: i128,
    withdrawn_delta: i128,
) -> Result<(), Error> {
    let key = DataKey::DonorTotals(donor.clone(), token.clone());
    let totals = read_donor_totals(env, &key, donor, token)?;
    let next = adjust_donor_totals(totals, committed_delta, withdrawn_delta)?;
    let storage = env.storage().persistent();
    storage.set(&key, &next);
    storage.extend_ttl(&key, STREAM_LIFETIME_THRESHOLD, STREAM_BUMP_AMOUNT);
    Ok(())
}

/// If an NGO registry is configured, verifies that `ngo` is approved/verified.
/// If no registry is configured, verification is skipped and this returns `Ok(())`.
/// Returns `Error::NgoNotVerified` if the NGO is unverified, unregistered, revoked,
/// or if the registry cross-contract call fails.
fn require_verified_ngo(env: &Env, ngo: &Address) -> Result<(), Error> {
    if let Some(registry_addr) = env
        .storage()
        .instance()
        .get::<_, Address>(&DataKey::Registry)
    {
        let ngo_entry = env
            .try_invoke_contract::<NgoRecord, soroban_sdk::Error>(
                &registry_addr,
                &symbol_short!("get_ngo"),
                soroban_sdk::vec![env, ngo.clone().into_val(env)],
            )
            .map_err(|_| Error::NgoNotVerified)?
            .map_err(|_| Error::NgoNotVerified)?;
        if !ngo_entry.verified {
            return Err(Error::NgoNotVerified);
        }
    }
    Ok(())
}

/// Moves `amount` from a stream's `balance` into its `withdrawn` total,
/// failing with `Error::ArithmeticOverflow` rather than panicking if
/// either would leave i128's range.
fn record_payout(env: &Env, stream: &mut Stream, amount: i128) -> Result<(), Error> {
    stream.balance = stream
        .balance
        .checked_sub(amount)
        .ok_or(Error::ArithmeticOverflow)?;
    stream.withdrawn = stream
        .withdrawn
        .checked_add(amount)
        .ok_or(Error::ArithmeticOverflow)?;
    update_donor_totals(env, &stream.donor, &stream.token, -amount, amount)
}

/// Derives the fee taken from a payout as `accrued - net`, where `net` is
/// what `pay_ngo` (or a batch's per-stream split) actually sent to the
/// NGO.
///
/// `net` never exceeds `accrued`, so the subtraction cannot fail while that
/// invariant holds. If it is ever broken this returns
/// `Error::ArithmeticOverflow` rather than reporting a fee of zero, so the
/// transaction aborts instead of publishing a misleading `withdraw` event.
fn fee_from_payout(accrued: i128, net: i128) -> Result<i128, Error> {
    accrued.checked_sub(net).ok_or(Error::ArithmeticOverflow)
}

/// Computes `amount * fee_bps / 10_000` without an intermediate overflow.
/// `amount * fee_bps` can exceed `i128::MAX` for a large `amount` even
/// though `fee_bps` is capped at `MAX_FEE_BPS` (1_000) ΓÇö a naive
/// `amount * fee_bps` (or a `saturating_mul` that silently clamps the
/// overflowed product before dividing) both under-count the fee for such
/// amounts. Splitting `amount` into a quotient/remainder around the 10_000
/// divisor first keeps every multiplication in range: `quotient * fee_bps`
/// is bounded by `amount / 10_000`, and `remainder * fee_bps` is bounded by
/// `9_999 * 1_000`, both comfortably inside i128. The result is the exact
/// `floor(amount * fee_bps / 10_000)`, not an approximation of it.
fn compute_fee(env: &Env, token: &Address, amount: i128) -> i128 {
    let treasury: Option<Address> = env.storage().instance().get(&DataKey::Treasury);
    match treasury {
        Some(_) => {
            let fee_bps = effective_fee_bps(env, token) as i128;
            let quotient = amount / 10_000;
            let remainder = amount % 10_000;
            (quotient * fee_bps + (remainder * fee_bps) / 10_000).min(amount)
        }
        None => 0,
    }
}

/// Returns a token-specific fee override, or the global fee when no override
/// has been configured for `token`.
fn effective_fee_bps(env: &Env, token: &Address) -> u32 {
    env.storage()
        .persistent()
        .get(&DataKey::TokenFeeBps(token.clone()))
        .unwrap_or_else(|| env.storage().instance().get(&DataKey::FeeBps).unwrap_or(0))
}

/// Pays `amount` out to the NGO, skimming a protocol fee to the treasury
/// first if one is configured. With no treasury set, the full amount goes
/// to the NGO regardless of `fee_bps` ΓÇö there's nowhere to send a fee.
///
/// Returns the net amount actually transferred to the NGO. This is the
/// single place the fee split is computed, so callers that report the
/// payout to their own callers (`withdraw`) can return exactly what the
/// NGO received rather than recomputing the fee and risking drift.
fn pay_ngo(env: &Env, token_client: &token::Client, ngo: &Address, amount: i128) -> i128 {
    if amount <= 0 {
        return 0;
    }

    let treasury: Option<Address> = env.storage().instance().get(&DataKey::Treasury);
    let fee = compute_fee(env, &token_client.address, amount);
    let net = amount - fee;

    if net > 0 {
        token_client.transfer(&env.current_contract_address(), ngo, &net);
    }
    if fee > 0 {
        if let Some(treasury_address) = treasury {
            token_client.transfer(&env.current_contract_address(), &treasury_address, &fee);
        }
    }

    net
}

/// Resolves where a payout for `ngo` should be sent: the payout address the
/// NGO configured via `set_payout_address`, or the NGO owner address itself
/// when none is set. Only `withdraw` and `withdraw_batch` route through this,
/// so an NGO can point withdrawals at a treasury wallet while its owner key
/// only authorises.
fn payout_destination(env: &Env, ngo: &Address) -> Address {
    env.storage()
        .persistent()
        .get(&DataKey::NgoPayoutAddress(ngo.clone()))
        .unwrap_or_else(|| ngo.clone())
}

/// Shared implementation of `sweep_surplus` and `sweep_surplus_to`. `None`
/// resolves the destination from the configured treasury (`NoTreasury` when
/// unset); `Some` sweeps to the caller-supplied address instead (issue #439).
fn sweep_surplus_inner(
    env: Env,
    token: Address,
    amount: i128,
    destination: Option<Address>,
) -> Result<(), Error> {
    require_admin(&env)?;
    require_not_paused(&env)?;

    if amount <= 0 {
        return Err(Error::InvalidAmount);
    }

    let destination = match destination {
        Some(destination) => destination,
        None => env
            .storage()
            .instance()
            .get(&DataKey::Treasury)
            .ok_or(Error::NoTreasury)?,
    };
    if destination == env.current_contract_address() {
        return Err(Error::InvalidSweepDestination);
    }

    let token_client = token::Client::new(&env, &token);
    let held = token_client.balance(&env.current_contract_address());
    // Anything held beyond the streams' committed balance is sweatable. A
    // broken invariant (held below committed) leaves no surplus rather than
    // underflowing, so a sweep is simply rejected.
    let surplus = held.saturating_sub(get_committed(&env, &token));
    if amount > surplus {
        return Err(Error::SweepExceedsSurplus);
    }

    token_client.transfer(&env.current_contract_address(), &destination, &amount);
    extend_instance_ttl(&env);

    SurplusSweptEvent {
        token,
        amount,
        destination,
    }
    .publish(&env);

    Ok(())
}

/// Returns the admin-configured maximum per-second rate, if any. `None`
/// (the default) means rates are uncapped, so the check in `create_stream`
/// and `modify_rate` is skipped entirely.
fn max_rate_cap(env: &Env) -> Option<i128> {
    env.storage().instance().get(&DataKey::MaxRate)
}

/// Returns the admin-configured minimum time to depletion, in seconds. `0`
/// (the default) means no minimum is enforced, so the check in
/// `create_stream` and `modify_rate` is skipped entirely. See issue #340.
fn min_duration_seconds(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::MinDuration)
        .unwrap_or(0)
}

/// Returns the admin-configured maximum balance a single stream may hold
/// (at creation or after a top-up), if any. `None` (the default) means no
/// ceiling is enforced, so the checks in `create_stream_inner` and `top_up`
/// are skipped entirely.
fn max_deposit_per_stream_cap(env: &Env) -> Option<i128> {
    env.storage().instance().get(&DataKey::MaxDepositPerStream)
}

/// Returns true when a stream holding `balance` at a per-second `rate` would
/// deplete in fewer than `min_duration` seconds. A `min_duration` of `0`
/// disables the check.
///
/// The lifespan is measured with ceiling division so a stream that lasts a
/// full final partial second is not misjudged: a 100-unit balance at 30 per
/// second lasts 4 seconds, not 3. `ceil(balance / rate) < min_duration` is
/// rewritten as `balance <= (min_duration - 1) * rate`, which avoids the
/// division and reports the (unreachable) overflow case as "too short".
fn depletes_before_min_duration(balance: i128, rate: i128, min_duration: u64) -> bool {
    if min_duration == 0 {
        return false;
    }
    match ((min_duration - 1) as i128).checked_mul(rate) {
        Some(threshold) => balance <= threshold,
        None => true,
    }
}

/// The token / deposit / rate checks shared by `create_stream_inner` and
/// `preview_stream`. Keeping them in one place means the preview cannot
/// drift from the real creation call for these inputs.
fn validate_stream_token_and_amount(
    env: &Env,
    token: &Address,
    deposit: i128,
    rate: i128,
) -> Result<(), Error> {
    // The vault is not a token contract, so the deposit could never move.
    if token == &env.current_contract_address() {
        return Err(Error::InvalidToken);
    }
    if deposit <= 0 || rate <= 0 {
        return Err(Error::InvalidAmount);
    }
    Ok(())
}

/// The admin-configured limits shared by `create_stream_inner` and
/// `preview_stream`: minimum deposit, maximum rate, maximum per-stream
/// deposit, and minimum duration.
fn validate_stream_config_limits(env: &Env, deposit: i128, rate: i128) -> Result<(), Error> {
    if deposit < DonationVault::min_deposit(env.clone()) {
        return Err(Error::DepositTooLow);
    }

    if let Some(max_rate) = max_rate_cap(env) {
        if rate > max_rate {
            return Err(Error::RateTooHigh);
        }
    }

    if let Some(max_dep) = max_deposit_per_stream_cap(env) {
        if deposit > max_dep {
            return Err(Error::DepositTooHigh);
        }
    }

    if depletes_before_min_duration(deposit, rate, min_duration_seconds(env)) {
        return Err(Error::StreamTooShort);
    }

    Ok(())
}

fn record_outflow_totals(env: &Env, gross: i128, fee: i128) -> Result<(), Error> {
    if gross > 0 {
        let withdrawn: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalWithdrawn)
            .unwrap_or(0);
        let next = withdrawn
            .checked_add(gross)
            .ok_or(Error::ArithmeticOverflow)?;
        env.storage()
            .instance()
            .set(&DataKey::TotalWithdrawn, &next);
    }
    if fee > 0 {
        let collected: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalFeesCollected)
            .unwrap_or(0);
        let next = collected
            .checked_add(fee)
            .ok_or(Error::ArithmeticOverflow)?;
        env.storage()
            .instance()
            .set(&DataKey::TotalFeesCollected, &next);
    }
    Ok(())
}

/// The timestamp a stream's accrual is measured from: the later of its last
/// checkpoint and its `start`. Until `start` arrives this is `start` itself,
/// so `now - base` is zero and nothing accrues ΓÇö the cliff ΓÇö even though
/// `last_update` may have been checkpointed earlier. See issue #337.
fn accrual_base(stream: &Stream) -> u64 {
    stream.last_update.max(stream.start)
}

/// Settles the accrual accumulated since the stream's last checkpoint.
///
/// This is shared by every operation that changes a stream's balance or rate
/// so payout accounting, checked arithmetic, and the checkpoint timestamp
/// cannot drift between entry points.
///
/// It marks an emptied Active stream `Drained` but does **not** emit the
/// `drained` event itself: `cancel_stream` and `top_up` reuse this to settle a
/// stream they are about to reclassify (Cancelled / Active), so the "ran to
/// completion" event would be spurious for them. Every other caller goes
/// through the [`settle`] wrapper, which announces the transition (issue #344).
fn settle_core(env: &Env, stream: &mut Stream, now: u64) -> Result<i128, Error> {
    let accrued = if stream.status == StreamStatus::Paused {
        0
    } else {
        let elapsed = now.saturating_sub(accrual_base(stream));
        math::accrued(stream.rate, elapsed, stream.balance)
    };

    if accrued > 0 {
        // Update stream state before the external token transfer
        record_payout(env, stream, accrued)?;
    }
    stream.last_update = now;

    // Mark as Drained if settle emptied the balance
    if stream.balance == 0 && stream.status == StreamStatus::Active {
        stream.status = StreamStatus::Drained;
    }

    // External token transfer is the last step
    if accrued > 0 {
        // The accrued amount leaves the vault's committed balance here: this
        // is the single place a payout is sent, so every entry point that
        // settles a stream inherits the reduction (issue #346).
        add_committed(env, &stream.token, -accrued)?;
        let token_client = token::Client::new(env, &stream.token);
        let fee = compute_fee(env, &stream.token, accrued);
        record_outflow_totals(env, accrued, fee)?;
        pay_ngo(env, &token_client, &stream.ngo, accrued);
    }

    Ok(accrued)
}

/// Settles a stream and, if that emptied it, publishes the `drained` event
/// (issue #344). Wraps [`settle_core`] so every settle-driven drain announces
/// the transition exactly once, from the same place the status flips.
fn settle(env: &Env, stream_id: u64, stream: &mut Stream, now: u64) -> Result<i128, Error> {
    let was_active = stream.status == StreamStatus::Active;
    let accrued = settle_core(env, stream, now)?;
    if was_active && stream.status == StreamStatus::Drained {
        DrainedEvent {
            stream_id,
            total_paid: stream.withdrawn,
        }
        .publish(env);
    }
    Ok(accrued)
}

/// Shared implementation behind `cancel_stream` and `cancel_streams`: settles
/// whatever has accrued on `stream` to the NGO, refunds the untouched
/// remainder to the donor, and leaves the stream zeroed and marked cancelled.
///
/// The caller has already loaded the stream, authorised its donor, and
/// rejected an already-cancelled stream. Keeping the body here means the
/// single and batch entry points cannot drift on settlement, refund,
/// committed-balance accounting, TTL retention, or event emission.
fn cancel_stream_inner(
    env: &Env,
    stream_id: u64,
    mut stream: Stream,
    now: u64,
    grace_ledgers: u32,
) -> Result<i128, Error> {
    let accrued = settle_core(env, &mut stream, now)?;

    let refund = stream.balance;
    if refund > 0 {
        update_donor_totals(env, &stream.donor, &stream.token, -refund, 0)?;
    }

    // Update state before the external token transfer
    stream.balance = 0;
    stream.rate = 0;
    stream.cancelled = true;
    stream.last_update = now;
    stream.status = StreamStatus::Cancelled;
    save_stream(env, stream_id, &stream);
    extend_cancelled_stream_ttl(env, stream_id, grace_ledgers)?;

    // External token transfer is the last step
    let token_client = token::Client::new(env, &stream.token);
    if refund > 0 {
        // `settle_core` already dropped the accrued payout from the committed
        // balance; the refunded remainder leaves it here (issue #346).
        add_committed(env, &stream.token, -refund)?;
        token_client.transfer(&env.current_contract_address(), &stream.donor, &refund);
    }

    // Only emit the cancel event when something actually moved. When both
    // values are zero the stream was already cancelled ΓÇö emitting here would
    // produce a duplicate that an indexer can't distinguish from a real
    // cancellation (see issue #91).
    if accrued > 0 || refund > 0 {
        CancelledStreamEvent {
            stream_id,
            accrued,
            refund,
        }
        .publish(env);
    }

    Ok(refund)
}

/// Shared implementation behind `create_stream`, `create_stream_with_start`,
/// and `create_stream_with_memo`: validates the arguments, records the stream,
/// and pulls the deposit into the vault.
///
/// `start` is the cliff timestamp ΓÇö the ledger time before which nothing
/// accrues ΓÇö or `None` for a stream that begins accruing immediately, in which
/// case it defaults to the current ledger time (issue #337). `memo` is the
/// donor's optional tag, empty when none was supplied (issue #341).
#[allow(clippy::too_many_arguments)]
fn create_stream_inner(
    env: Env,
    donor: Address,
    ngo: Address,
    token: Address,
    deposit: i128,
    rate: i128,
    start: Option<u64>,
    memo: Bytes,
) -> Result<u64, Error> {
    require_not_paused(&env)?;
    donor.require_auth();

    // Checked before the deposit is pulled and before the amounts are
    // validated: a self-stream is never a legitimate call regardless of
    // how the other arguments look, and it must not reach the transfer.
    if donor == ngo {
        return Err(Error::SelfStream);
    }

    // The vault itself can be neither party nor asset: nobody can
    // authorise as the vault to withdraw, and the vault is not a token
    // contract. Checked before any state is written or funds move,
    // whether or not a registry is configured.
    let vault = env.current_contract_address();
    if ngo == vault {
        return Err(Error::InvalidNgo);
    }

    validate_stream_token_and_amount(&env, &token, deposit, rate)?;

    // The cliff, if any, may sit up to `MAX_START_DELAY_SECONDS` ahead of now
    // (issue #337). Anything further is treated as a mistyped date so a deposit
    // can't be stranded behind an unreachable start. `None` (plain
    // `create_stream`) starts now, matching the deposit's own ledger time.
    let now = env.ledger().timestamp();
    let start = start.unwrap_or(now);
    if start > now.saturating_add(MAX_START_DELAY_SECONDS) {
        return Err(Error::StartTooFar);
    }

    // The memo is a short tag, not a payload (issue #341): anything past
    // `MAX_MEMO_BYTES` is rejected rather than truncated, so a donor never
    // believes a tag was stored when only part of it was.
    if memo.len() > MAX_MEMO_BYTES {
        return Err(Error::MemoTooLong);
    }

    // Admin-configured limits: minimum deposit (issue #76), per-second rate
    // ceiling (issue #339), per-stream deposit ceiling, and minimum lifespan
    // (issue #340). Shared with `preview_stream` so the two can never disagree
    // on these checks.
    validate_stream_config_limits(&env, deposit, rate)?;

    // Per-donor stream cap (issue #94). Read before the token transfer
    // so a rejected donor is rejected without moving funds.
    let max_streams: u64 = env
        .storage()
        .instance()
        .get(&DataKey::MaxStreamsPerDonor)
        .unwrap_or(DEFAULT_MAX_STREAMS_PER_DONOR);
    let donor_streams: u64 = env
        .storage()
        .persistent()
        .get(&DataKey::DonorStreamCount(donor.clone()))
        .unwrap_or(0);
    if donor_streams >= max_streams {
        return Err(Error::StreamLimitExceeded);
    }

    // If a registry has been configured, verify the NGO is approved before
    // pulling any funds from the donor.
    require_verified_ngo(&env, &ngo)?;

    let total_donated: i128 = env
        .storage()
        .instance()
        .get(&DataKey::TotalDonated)
        .unwrap_or(0);
    let next_total = total_donated
        .checked_add(deposit)
        .ok_or(Error::ArithmeticOverflow)?;

    let stream_id: u64 = env
        .storage()
        .instance()
        .get(&DataKey::NextStreamId)
        .ok_or(Error::StreamCounterMissing)?;

    let stream = Stream {
        donor: donor.clone(),
        ngo: ngo.clone(),
        token: token.clone(),
        rate,
        balance: deposit,
        withdrawn: 0,
        created_at: now,
        start,
        last_update: now,
        memo: memo.clone(),
        status: StreamStatus::Active,
        cancelled: false,
    };

    // Load any legacy totals before adding the stream to storage.
    update_donor_totals(&env, &donor, &token, deposit, 0)?;

    // Write all state before the external token transfer
    save_stream(&env, stream_id, &stream);
    let ngo_key = DataKey::NgoStreams(ngo.clone());
    let mut ngo_streams: Vec<u64> = env
        .storage()
        .persistent()
        .get(&ngo_key)
        .unwrap_or_else(|| Vec::new(&env));
    ngo_streams.push_back(stream_id);
    env.storage().persistent().set(&ngo_key, &ngo_streams);
    env.storage()
        .persistent()
        .extend_ttl(&ngo_key, STREAM_LIFETIME_THRESHOLD, STREAM_BUMP_AMOUNT);
    let next_stream_id = stream_id.checked_add(1).ok_or(Error::ArithmeticOverflow)?;
    env.storage()
        .instance()
        .set(&DataKey::NextStreamId, &next_stream_id);

    let donor_key = DataKey::DonorStreamCount(donor.clone());
    let new_count = donor_streams
        .checked_add(1)
        .ok_or(Error::ArithmeticOverflow)?;
    env.storage().persistent().set(&donor_key, &new_count);
    env.storage().persistent().extend_ttl(
        &donor_key,
        STREAM_LIFETIME_THRESHOLD,
        STREAM_BUMP_AMOUNT,
    );

    env.storage()
        .instance()
        .set(&DataKey::TotalDonated, &next_total);

    extend_instance_ttl(&env);

    // The deposit now belongs to this stream, so record it against the
    // token's committed balance before the funds move (issue #346).
    add_committed(&env, &token, deposit)?;

    // External token transfer is the last step
    let token_client = token::Client::new(&env, &token);
    token_client.transfer(&donor, env.current_contract_address(), &deposit);

    CreatedEvent {
        stream_id,
        donor,
        ngo,
        token,
        deposit,
        rate,
        memo,
    }
    .publish(&env);

    Ok(stream_id)
}

#[contract]
pub struct DonationVault;

#[contractimpl]
impl DonationVault {
    /// Initializes the vault admin and seeds the initial configuration atomically at deployment time.
    pub fn __constructor(env: Env, admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::NextStreamId, &0u64);
        env.storage()
            .instance()
            .set(&DataKey::DonorTotalsIndexedFromStart, &true);
        env.storage().instance().set(&DataKey::MinDeposit, &0i128);
        env.storage().instance().set(&DataKey::TotalDonated, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::TotalWithdrawn, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::TotalFeesCollected, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::CancelGraceLedgers, &0u32);
        extend_instance_ttl(&env);
        InitializedEvent { admin }.publish(&env);
    }

    /// Sets the vault admin and seeds the stream-id counter. Can only be called once.
    /// Neutralized in favor of `__constructor` for atomic deployment; requires admin auth.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultArgs, DonationVaultClient};
    /// let env = Env::default();
    /// env.mock_all_auths();
    ///
    /// let admin = Address::generate(&env);
    /// let contract_id = env.register(DonationVault, DonationVaultArgs::__constructor(&admin));
    /// let client = DonationVaultClient::new(&env, &contract_id);
    /// ```
    pub fn init(env: Env, admin: Address) -> Result<(), Error> {
        admin.require_auth();
        if admin_renounced(&env) {
            return Err(Error::AdminRenounced);
        }
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::NextStreamId, &0u64);
        env.storage()
            .instance()
            .set(&DataKey::DonorTotalsIndexedFromStart, &true);
        env.storage().instance().set(&DataKey::MinDeposit, &0i128);
        env.storage().instance().set(&DataKey::TotalDonated, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::TotalWithdrawn, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::TotalFeesCollected, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::CancelGraceLedgers, &0u32);
        extend_instance_ttl(&env);
        InitializedEvent { admin }.publish(&env);
        Ok(())
    }

    /// Permanently gives up admin control. Admin-authed.
    ///
    /// Clears the stored admin and any pending admin proposal. After this
    /// call every admin-gated entry point fails with
    /// `Error::AdminRenounced`, so the admin-gated surface is permanently
    /// disabled. This cannot be undone.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// client.renounce_admin();
    ///
    /// // The admin-gated surface is now permanently disabled.
    /// assert!(client.try_pause().is_err());
    /// ```
    pub fn renounce_admin(env: Env) -> Result<(), Error> {
        require_admin(&env)?;

        env.storage().instance().remove(&DataKey::Admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.storage()
            .instance()
            .set(&DataKey::AdminRenounced, &true);
        extend_instance_ttl(&env);

        RenouncedAdminEvent { data: () }.publish(&env);

        Ok(())
    }

    /// Returns the cumulative amount deposited into all streams, including top-ups.
    /// Withdrawals and cancellations do not reduce this lifetime total.
    pub fn total_donated(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalDonated)
            .unwrap_or(0)
    }

    /// Returns the cumulative gross amount settled and paid out from streams.
    pub fn total_withdrawn(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalWithdrawn)
            .unwrap_or(0)
    }

    /// Returns the cumulative protocol fees sent to the treasury.
    pub fn total_fees_collected(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalFeesCollected)
            .unwrap_or(0)
    }

    /// Reads back the vault admin set by `init`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// assert_eq!(client.admin(), admin);
    /// ```
    pub fn admin(env: Env) -> Result<Address, Error> {
        if admin_renounced(&env) {
            return Err(Error::AdminRenounced);
        }
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    /// Reads back the address proposed by `propose_admin`, if any hasn't
    /// yet been accepted or cancelled.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// assert_eq!(client.pending_admin(), None);
    ///
    /// let new_admin = Address::generate(&env);
    /// client.propose_admin(&new_admin);
    /// assert_eq!(client.pending_admin(), Some(new_admin));
    /// ```
    pub fn pending_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::PendingAdmin)
    }

    /// Starts a two-step admin transfer by recording `new_admin` as pending.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// let new_admin = Address::generate(&env);
    /// client.propose_admin(&new_admin);
    /// // The old admin is still in charge until accept_admin is called.
    /// assert_eq!(client.admin(), admin);
    /// ```
    pub fn propose_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        let current_admin = require_admin(&env)?;
        if new_admin == current_admin {
            return Err(Error::InvalidAdmin);
        }

        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        extend_instance_ttl(&env);

        ProposedAdminEvent { admin: new_admin }.publish(&env);

        Ok(())
    }

    /// Completes a two-step admin transfer.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// let new_admin = Address::generate(&env);
    /// client.propose_admin(&new_admin);
    /// client.accept_admin();
    /// assert_eq!(client.admin(), new_admin);
    /// ```
    pub fn accept_admin(env: Env) -> Result<(), Error> {
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .ok_or(Error::NoPendingAdmin)?;
        pending.require_auth();

        env.storage().instance().set(&DataKey::Admin, &pending);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        extend_instance_ttl(&env);

        AcceptedAdminEvent { admin: pending }.publish(&env);

        Ok(())
    }

    /// Withdraws a pending admin proposal, leaving nothing pending.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// let new_admin = Address::generate(&env);
    /// client.propose_admin(&new_admin);
    ///
    /// // The admin changes their mind before it's accepted.
    /// client.cancel_admin_proposal();
    ///
    /// // Nothing left to accept.
    /// let result = client.try_accept_admin();
    /// assert!(result.is_err());
    /// ```
    pub fn cancel_admin_proposal(env: Env) -> Result<(), Error> {
        require_admin(&env)?;

        if !env.storage().instance().has(&DataKey::PendingAdmin) {
            return Err(Error::NoPendingAdmin);
        }
        env.storage().instance().remove(&DataKey::PendingAdmin);
        extend_instance_ttl(&env);

        CancelledAdminEvent { data: () }.publish(&env);

        Ok(())
    }

    /// Reads back a stream by id.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::{Address as _, Ledger}, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &1_000);
    /// let stream_id = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    ///
    /// let stream = client.get_stream(&stream_id);
    /// assert_eq!(stream.balance, 1_000);
    /// assert_eq!(stream.rate, 10);
    /// assert!(!stream.cancelled);
    ///
    /// // Once cancelled, `cancelled` stays true even though a drained
    /// // (fully withdrawn) stream would also show `rate == 0 && balance == 0`.
    /// client.cancel_stream(&stream_id);
    /// assert!(client.get_stream(&stream_id).cancelled);
    /// ```
    pub fn get_stream(env: Env, stream_id: u64) -> Result<Stream, Error> {
        load_stream(&env, stream_id)
    }

    /// Reads multiple streams in a single call, returning one entry per
    /// requested id in the same order (`None` for an id that does not exist).
    /// Read-only; requires no authorization.
    ///
    /// Requests with more than `MAX_BATCH_GET_STREAMS` ids are rejected with
    /// `Error::BatchLimitExceeded`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, token, Address, Env, Vec};
    /// # use donation_vault::{DonationVault, DonationVaultArgs, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(DonationVault, DonationVaultArgs::__constructor(&admin));
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &2_000);
    /// let a = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    /// let b = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &20);
    ///
    /// let mut ids = Vec::new(&env);
    /// ids.push_back(a);
    /// ids.push_back(999); // never created
    /// ids.push_back(b);
    ///
    /// let streams = client.get_streams(&ids);
    /// assert!(streams.get(0).unwrap().is_some());
    /// assert!(streams.get(1).unwrap().is_none());
    /// assert_eq!(streams.get(2).unwrap().unwrap().rate, 20);
    /// ```
    pub fn get_streams(env: Env, ids: Vec<u64>) -> Result<Vec<Option<Stream>>, Error> {
        if ids.len() > MAX_BATCH_GET_STREAMS {
            return Err(Error::BatchLimitExceeded);
        }
        let mut streams: Vec<Option<Stream>> = Vec::new(&env);
        for id in ids.iter() {
            streams.push_back(try_load_stream(&env, id));
        }
        Ok(streams)
    }

    /// Lists every stream id belonging to `donor`, in creation order.
    ///
    /// Cancelled streams are still included because their records are kept
    /// (callers can inspect `status`/`cancelled`). This is a read-only view.
    pub fn streams_by_donor(env: Env, donor: Address) -> Vec<u64> {
        let total: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextStreamId)
            .unwrap_or(0);
        let mut ids = Vec::new(&env);
        for id in 0..total {
            if let Some(stream) = try_load_stream(&env, id) {
                if stream.donor == donor {
                    ids.push_back(id);
                }
            }
        }
        ids
    }

    /// Returns the per-donor stream counter used to enforce the stream cap.
    ///
    /// This is the same counter checked by `create_stream`. It is incremented
    /// when a stream is created and never decremented, so cancelled streams
    /// still count toward the donor's limit. Returns `0` for donors that have
    /// never created a stream.
    pub fn donor_stream_count(env: Env, donor: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::DonorStreamCount(donor))
            .unwrap_or(0)
    }

    /// Returns committed and gross withdrawn funds for the donor and token.
    /// Requires no authorization and does not mutate storage.
    pub fn donor_totals(env: Env, donor: Address, token: Address) -> Result<DonorTotals, Error> {
        let key = DataKey::DonorTotals(donor.clone(), token.clone());
        read_donor_totals(&env, &key, &donor, &token)
    }

    /// Lists stream ids associated with an NGO in creation order.
    /// Results are bounded by `limit`, which is clamped to `MAX_STREAMS_BY_NGO`.
    pub fn streams_by_ngo(env: Env, ngo: Address, start: u32, limit: u32) -> Vec<u64> {
        let ngo_streams: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::NgoStreams(ngo))
            .unwrap_or_else(|| Vec::new(&env));
        let capped = limit.min(MAX_STREAMS_BY_NGO);
        let mut ids = Vec::new(&env);
        if start >= ngo_streams.len() {
            return ids;
        }
        let end = start.saturating_add(capped).min(ngo_streams.len());
        for i in start..end {
            if let Some(id) = ngo_streams.get(i) {
                ids.push_back(id);
            }
        }
        ids
    }

    /// Lists the ids of an NGO's streams that are still live — `Active` or
    /// `Paused` — skipping `Cancelled` and `Drained` ones (issue #440).
    ///
    /// Unlike `streams_by_ngo`, this pages over a filtered view: finished
    /// streams never occupy a slot, so a caller enumerating an NGO's open
    /// positions does not have to fetch and discard them. The contract
    /// examines at most `MAX_STREAMS_BY_NGO` index positions per call and
    /// returns at most `limit` ids (also clamped to `MAX_STREAMS_BY_NGO`);
    /// `next` is the position to pass as `start` on the next call.
    ///
    /// Iteration is complete when `next` does not advance past the `start`
    /// passed in — either because every position was examined or because
    /// `limit` was zero. An empty `ids` with an advancing `next` only means
    /// the scanned window held no live streams; keep paging. This is a
    /// read-only view.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # client.init(&admin);
    /// let page = client.active_streams_by_ngo(&ngo, &0, &20);
    /// assert!(page.ids.len() <= 20);
    /// // Page again from `page.next` until it stops advancing.
    /// ```
    pub fn active_streams_by_ngo(
        env: Env,
        ngo: Address,
        start: u32,
        limit: u32,
    ) -> ActiveStreamsPage {
        let ngo_streams: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::NgoStreams(ngo))
            .unwrap_or_else(|| Vec::new(&env));
        let total = ngo_streams.len();
        if start >= total || limit == 0 {
            return ActiveStreamsPage {
                ids: Vec::new(&env),
                next: start.min(total),
            };
        }

        let target = limit.min(MAX_STREAMS_BY_NGO);
        let window_end = start.saturating_add(MAX_STREAMS_BY_NGO).min(total);
        let mut ids = Vec::new(&env);
        let mut index = start;
        while index < window_end && ids.len() < target {
            let id = ngo_streams.get(index).unwrap();
            index += 1;
            if let Some(stream) = try_load_stream(&env, id) {
                if matches!(stream.status, StreamStatus::Active | StreamStatus::Paused) {
                    ids.push_back(id);
                }
            }
        }

        ActiveStreamsPage { ids, next: index }
    }

    /// Reads back the number of streams ever created — the exclusive upper
    /// bound on valid stream ids.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// assert_eq!(client.stream_count(), 0);
    ///
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &1_000);
    /// client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    /// assert_eq!(client.stream_count(), 1);
    /// ```
    pub fn stream_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::NextStreamId)
            .unwrap_or(0)
    }

    /// Read-only lookup of how much a stream has accrued to the NGO so far.
    /// A paused stream reports `0`: no time passes for it until it resumes.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::{Address as _, Ledger}, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &1_000);
    /// let stream_id = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    /// env.ledger().with_mut(|l| l.timestamp += 50);
    ///
    /// assert_eq!(client.pending_accrual(&stream_id), 500);
    /// // Balance is untouched ΓÇö pending_accrual doesn't pay out.
    /// assert_eq!(client.get_stream(&stream_id).balance, 1_000);
    /// ```
    pub fn pending_accrual(env: Env, stream_id: u64) -> Result<i128, Error> {
        let stream = load_stream(&env, stream_id)?;

        // Mirror `settle`: a paused stream accrues nothing.
        if stream.status == StreamStatus::Paused {
            return Ok(0);
        }

        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(accrual_base(&stream));
        Ok(math::accrued(stream.rate, elapsed, stream.balance))
    }

    /// Read-only lookup of the ledger timestamp at which a stream's balance
    /// runs out, so every client gets the same answer with the rounding done
    /// in one place. The stream's `balance` counts everything not yet paid
    /// out (including what's accrued but unwithdrawn) and `last_update` is
    /// when it was last settled, so this is `last_update` plus the seconds
    /// `balance` takes at `rate`, rounded up ΓÇö a partial final second counts
    /// as a whole one.
    ///
    /// Returns `None` when the stream will never deplete: a cancelled or
    /// zero-rate stream, or a timestamp too far out to represent. An already
    /// empty stream that still has a rate returns its `last_update`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &1_000);
    /// // 1_000 units at 300/s take 3.33s, so the stream ends at second 4.
    /// let stream_id = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &300);
    /// let created_at = client.get_stream(&stream_id).created_at;
    /// assert_eq!(client.depletion_time(&stream_id), Some(created_at + 4));
    ///
    /// // A cancelled stream never depletes.
    /// client.cancel_stream(&stream_id);
    /// assert_eq!(client.depletion_time(&stream_id), None);
    /// ```
    pub fn depletion_time(env: Env, stream_id: u64) -> Result<Option<u64>, Error> {
        let stream = load_stream(&env, stream_id)?;

        Ok(math::seconds_to_deplete(stream.rate, stream.balance)
            .and_then(|seconds| accrual_base(&stream).checked_add(seconds)))
    }

    /// Read-only view of the net amount the NGO would actually receive and the
    /// fee that would be taken if `withdraw` were called right now.
    ///
    /// Unlike `pending_accrual`, which returns the gross accrued amount, this
    /// accounts for any configured treasury fee, so a UI can show the correct
    /// "you will receive X" figure rather than overstating it.
    ///
    /// Never mutates storage or moves funds.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::{Address as _, Ledger}, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &1_000);
    /// let treasury = Address::generate(&env);
    /// client.set_treasury(&treasury);
    /// client.set_fee_bps(&500); // 5%
    ///
    /// let stream_id = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    /// env.ledger().with_mut(|l| l.timestamp += 50); // 500 accrues
    ///
    /// let (net, fee) = client.pending_payout(&stream_id);
    /// assert_eq!(fee, 25);   // 5% of 500
    /// assert_eq!(net, 475);  // 500 - 25
    /// ```
    pub fn pending_payout(env: Env, stream_id: u64) -> Result<(i128, i128), Error> {
        let stream = load_stream(&env, stream_id)?;

        // Mirror `settle`: a paused stream accrues nothing.
        let gross = if stream.status == StreamStatus::Paused {
            0
        } else {
            let now = env.ledger().timestamp();
            let elapsed = now.saturating_sub(accrual_base(&stream));
            math::accrued(stream.rate, elapsed, stream.balance)
        };
        let fee = compute_fee(&env, &stream.token, gross);
        Ok((gross - fee, fee))
    }

    /// Read-only view of the amount a `cancel_stream` would refund the donor
    /// right now: the stream's remaining `balance` minus what has accrued to
    /// the NGO (issue #345).
    ///
    /// `pending_payout` shows what the NGO would receive; this is the donor's
    /// mirror image, so a UI can show the refund a cancel would return before
    /// the donor commits to it. It applies the same rules as `cancel_stream`,
    /// which settles via the shared `settle` path: nothing accrues before a
    /// stream's `start` (cliff), and a stream paused by its donor accrues
    /// nothing, so its whole balance is refundable. Accrual is capped at the
    /// balance, so the result is never negative.
    ///
    /// A drained stream (`balance` 0) and an already-cancelled stream (balance
    /// and rate zeroed) both report `0`, consistent with `pending_payout` and
    /// `depletion_time`, which likewise read a cancelled stream's zeroed state
    /// rather than failing.
    ///
    /// Never mutates storage or moves funds.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::{Address as _, Ledger}, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &1_000);
    /// let stream_id = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    /// env.ledger().with_mut(|l| l.timestamp += 20); // 200 accrues
    ///
    /// // A cancel right now would refund the untouched 800 to the donor.
    /// assert_eq!(client.pending_refund(&stream_id), 800);
    /// ```
    pub fn pending_refund(env: Env, stream_id: u64) -> Result<i128, Error> {
        let stream = load_stream(&env, stream_id)?;

        // Mirror `settle`'s accrual rules so this matches what `cancel_stream`
        // would actually refund: a paused stream accrues nothing, and a
        // non-positive rate or a stream before its cliff accrues nothing.
        let accrued = if stream.status == StreamStatus::Paused {
            0
        } else {
            let now = env.ledger().timestamp();
            let elapsed = now.saturating_sub(accrual_base(&stream));
            math::accrued(stream.rate, elapsed, stream.balance)
        };

        Ok(stream.balance - accrued)
    }

    /// Bumps a stream's persistent-storage TTL without touching its state.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &1_000);
    /// let stream_id = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    ///
    /// // Anyone can keep the stream's storage alive, no auth required.
    /// client.extend_stream(&stream_id);
    /// ```
    pub fn extend_stream(env: Env, stream_id: u64) -> Result<(), Error> {
        load_stream(&env, stream_id)?;
        Ok(())
    }

    /// Halts stream creation, withdrawal, top-up, and rate changes.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// client.pause();
    /// assert!(client.paused());
    /// ```
    pub fn pause(env: Env) -> Result<(), Error> {
        require_admin(&env)?;
        let already_paused = env
            .storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::Paused)
            .unwrap_or(false);
        if already_paused {
            return Err(Error::AlreadyPaused);
        }
        env.storage().instance().set(&DataKey::Paused, &true);
        extend_instance_ttl(&env);
        PausedEvent { data: () }.publish(&env);
        Ok(())
    }

    /// Lifts a pause, restoring normal operation. Admin-gated.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # client.pause();
    /// client.unpause();
    /// assert!(!client.paused());
    /// ```
    pub fn unpause(env: Env) -> Result<(), Error> {
        require_admin(&env)?;
        let already_unpaused = !env
            .storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::Paused)
            .unwrap_or(false);
        if already_unpaused {
            return Err(Error::AlreadyUnpaused);
        }
        env.storage().instance().set(&DataKey::Paused, &false);
        extend_instance_ttl(&env);
        UnpausedEvent { data: () }.publish(&env);
        Ok(())
    }

    /// Whether the vault is currently paused.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// assert!(!client.paused());
    /// ```
    pub fn paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    /// Reads back all admin-set configuration and contract state parameters in one call:
    /// `paused`, `treasury`, `fee_bps`, `min_deposit`, `max_streams_per_donor`,
    /// `max_rate`, `min_duration`, `registry`, `cancel_grace_ledgers`, `total_donated`,
    /// `total_withdrawn`, `total_fees_collected`, `pending_admin`, `renounced`, and
    /// `max_deposit_per_stream`.
    pub fn get_config(env: Env) -> Config {
        Config {
            paused: env
                .storage()
                .instance()
                .get(&DataKey::Paused)
                .unwrap_or(false),
            treasury: env.storage().instance().get(&DataKey::Treasury),
            fee_bps: env.storage().instance().get(&DataKey::FeeBps).unwrap_or(0),
            min_deposit: env
                .storage()
                .instance()
                .get(&DataKey::MinDeposit)
                .unwrap_or(0),
            max_streams_per_donor: env
                .storage()
                .instance()
                .get(&DataKey::MaxStreamsPerDonor)
                .unwrap_or(DEFAULT_MAX_STREAMS_PER_DONOR),
            max_rate: max_rate_cap(&env),
            min_duration: min_duration_seconds(&env),
            registry: env.storage().instance().get(&DataKey::Registry),
            cancel_grace_ledgers: env
                .storage()
                .instance()
                .get(&DataKey::CancelGraceLedgers)
                .unwrap_or(0),
            total_donated: env
                .storage()
                .instance()
                .get(&DataKey::TotalDonated)
                .unwrap_or(0),
            total_withdrawn: env
                .storage()
                .instance()
                .get(&DataKey::TotalWithdrawn)
                .unwrap_or(0),
            total_fees_collected: env
                .storage()
                .instance()
                .get(&DataKey::TotalFeesCollected)
                .unwrap_or(0),
            pending_admin: env.storage().instance().get(&DataKey::PendingAdmin),
            renounced: env
                .storage()
                .instance()
                .get(&DataKey::AdminRenounced)
                .unwrap_or(false),
            max_deposit_per_stream: max_deposit_per_stream_cap(&env),
        }
    }

    /// Sets where the protocol fee (if any) gets paid. Admin-gated.
    /// Emits a `treasset` event carrying the new address so an off-chain
    /// indexer can track treasury changes without polling `treasury`.
    ///
    /// Fails with `Error::InvalidTreasury` if `treasury` is this contract's
    /// own address: the vault has no way to spend from itself, so fees sent
    /// there would be locked permanently.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// let treasury = Address::generate(&env);
    /// client.set_treasury(&treasury);
    /// assert_eq!(client.treasury(), Some(treasury));
    /// ```
    pub fn set_treasury(env: Env, treasury: Address) -> Result<(), Error> {
        require_admin(&env)?;
        if treasury == env.current_contract_address() {
            return Err(Error::InvalidTreasury);
        }
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        extend_instance_ttl(&env);

        TreasurySetEvent { treasury }.publish(&env);

        Ok(())
    }

    /// Reads back the configured treasury address, if any.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// assert_eq!(client.treasury(), None);
    /// ```
    pub fn treasury(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Treasury)
    }

    /// Removes the configured treasury so protocol fees are no longer collected.
    /// After this call `treasury()` returns `None` and `pay_ngo` sends the full
    /// payout directly to the NGO, the same as if a treasury had never been set.
    /// Admin-gated.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// let treasury = Address::generate(&env);
    /// client.set_treasury(&treasury);
    /// assert_eq!(client.treasury(), Some(treasury));
    ///
    /// client.clear_treasury();
    /// assert_eq!(client.treasury(), None);
    /// ```
    pub fn clear_treasury(env: Env) -> Result<(), Error> {
        require_admin(&env)?;
        // Reject clearing the treasury when a fee is currently set
        let fee_bps: u32 = env.storage().instance().get(&DataKey::FeeBps).unwrap_or(0);
        if fee_bps > 0 {
            return Err(Error::FeeRequiresTreasury);
        }
        env.storage().instance().remove(&DataKey::Treasury);
        extend_instance_ttl(&env);

        TreasuryClearedEvent { data: () }.publish(&env);

        Ok(())
    }

    /// Sets the protocol fee, in basis points, taken out of accrued payouts
    /// to the NGO. Admin-gated, capped at `MAX_FEE_BPS`. Has no effect
    /// unless a treasury is also set. Emits a `feeset` event carrying the
    /// new value so an off-chain indexer can track fee changes without
    /// polling `fee_bps`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// client.set_fee_bps(&500); // 5%
    /// assert_eq!(client.fee_bps(), 500);
    ///
    /// // Anything over the 10% cap is rejected.
    /// let result = client.try_set_fee_bps(&1_001);
    /// assert!(result.is_err());
    /// ```
    pub fn set_fee_bps(env: Env, fee_bps: u32) -> Result<(), Error> {
        require_admin(&env)?;
        if fee_bps > MAX_FEE_BPS {
            return Err(Error::FeeTooHigh);
        }
        // Reject setting a non-zero fee when no treasury is configured
        if fee_bps > 0 {
            let treasury: Option<Address> = env.storage().instance().get(&DataKey::Treasury);
            if treasury.is_none() {
                return Err(Error::FeeRequiresTreasury);
            }
        }
        env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
        extend_instance_ttl(&env);

        FeeBpsSetEvent { fee_bps }.publish(&env);

        Ok(())
    }

    /// Reads back the configured protocol fee, in basis points.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// assert_eq!(client.fee_bps(), 0);
    /// ```
    pub fn fee_bps(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::FeeBps).unwrap_or(0)
    }

    /// Returns the maximum protocol fee, in basis points.
    ///
    /// This is exposed on-chain so clients can present the contract's fee
    /// ceiling without maintaining a separate off-chain copy.
    pub fn max_fee_bps(_env: Env) -> u32 {
        MAX_FEE_BPS
    }

    /// Sets a per-token protocol fee override, in basis points (issue
    /// #200). `pay_ngo` uses this instead of the global `fee_bps` for any
    /// payout in `token`, falling back to the global default for every
    /// token with no override configured. Admin-gated, capped at the same
    /// `MAX_FEE_BPS` as the global fee. Emits a `tokfeeset` event carrying
    /// the token and new value.
    ///
    /// Stored per-token in persistent storage rather than instance storage
    /// (unlike the global `FeeBps`): the set of tokens ever streamed
    /// through this vault is unbounded, so this should not grow the fixed
    /// instance entry that every invocation already has to read and write.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token = Address::generate(&env);
    /// client.set_fee_bps(&500); // 5% global default
    /// client.set_token_fee_bps(&token, &100); // 1% for this one token
    ///
    /// assert_eq!(client.token_fee_bps(&token), 100);
    /// // A token with no override still sees the global default.
    /// let other_token = Address::generate(&env);
    /// assert_eq!(client.token_fee_bps(&other_token), 500);
    /// ```
    pub fn set_token_fee_bps(env: Env, token: Address, fee_bps: u32) -> Result<(), Error> {
        require_admin(&env)?;
        if fee_bps > MAX_FEE_BPS {
            return Err(Error::FeeTooHigh);
        }
        // Reject setting a non-zero fee when no treasury is configured
        if fee_bps > 0 {
            let treasury: Option<Address> = env.storage().instance().get(&DataKey::Treasury);
            if treasury.is_none() {
                return Err(Error::FeeRequiresTreasury);
            }
        }
        let key = DataKey::TokenFeeBps(token.clone());
        env.storage().persistent().set(&key, &fee_bps);
        env.storage().persistent().extend_ttl(
            &key,
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
        extend_instance_ttl(&env);

        TokenFeeBpsSetEvent { token, fee_bps }.publish(&env);

        Ok(())
    }

    /// Clears the fee override for `token`, causing subsequent payouts in
    /// this token to fall back to the global `fee_bps`. Admin-gated.
    ///
    /// If no override was configured for `token`, this is a clean no-op
    /// (consistent with `clear_treasury`), returning `Ok(())` without modifying
    /// storage or emitting an event.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token = Address::generate(&env);
    /// client.set_fee_bps(&500);
    /// client.set_token_fee_bps(&token, &100);
    /// assert_eq!(client.token_fee_bps(&token), 100);
    ///
    /// client.clear_token_fee_bps(&token);
    /// assert_eq!(client.token_fee_bps(&token), 500);
    /// ```
    pub fn clear_token_fee_bps(env: Env, token: Address) -> Result<(), Error> {
        require_admin(&env)?;
        let key = DataKey::TokenFeeBps(token.clone());
        if !env.storage().persistent().has(&key) {
            return Ok(());
        }
        env.storage().persistent().remove(&key);
        extend_instance_ttl(&env);

        TokenFeeBpsClearedEvent { token, data: () }.publish(&env);

        Ok(())
    }

    /// Reads back the fee, in basis points, that actually applies to
    /// `token`: its override if `set_token_fee_bps` has been called for it,
    /// otherwise the global `fee_bps`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token = Address::generate(&env);
    /// assert_eq!(client.token_fee_bps(&token), 0); // no global fee, no override
    /// ```
    pub fn token_fee_bps(env: Env, token: Address) -> u32 {
        effective_fee_bps(&env, &token)
    }

    /// Returns the configured token allowlist for frontend token pickers.
    /// Until an allowlist is configured, this returns an empty vector.
    pub fn allowed_tokens(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::AllowedTokens)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Sets the minimum `deposit` accepted by `create_stream`, letting an
    /// operator filter out dust streams without changing application-level
    /// validation on every frontend that talks to the contract. Admin-gated.
    /// Defaults to `0` (today's behavior) until set.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// client.set_min_deposit(&100);
    /// assert_eq!(client.min_deposit(), 100);
    /// ```
    pub fn set_min_deposit(env: Env, min_deposit: i128) -> Result<(), Error> {
        require_admin(&env)?;
        if min_deposit < 0 {
            return Err(Error::InvalidAmount);
        }
        env.storage()
            .instance()
            .set(&DataKey::MinDeposit, &min_deposit);
        extend_instance_ttl(&env);

        MinDepositSetEvent { min_deposit }.publish(&env);

        Ok(())
    }

    /// Reads back the configured minimum deposit. `0` until an admin sets
    /// one.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// assert_eq!(client.min_deposit(), 0);
    /// ```
    pub fn min_deposit(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::MinDeposit)
            .unwrap_or(0)
    }

    /// Sets the extra retention window applied to cancelled stream records
    /// so indexers can observe their final state before the entry expires.
    /// The value is measured in ledgers and defaults to zero.
    pub fn set_cancel_grace_ledgers(env: Env, grace_ledgers: u32) -> Result<(), Error> {
        require_admin(&env)?;
        STREAM_BUMP_AMOUNT
            .checked_add(grace_ledgers)
            .ok_or(Error::ArithmeticOverflow)?;
        env.storage()
            .instance()
            .set(&DataKey::CancelGraceLedgers, &grace_ledgers);
        extend_instance_ttl(&env);

        CancelGraceLedgersSetEvent { grace_ledgers }.publish(&env);

        Ok(())
    }

    /// Returns the configured cancelled-stream retention window in ledgers.
    pub fn cancel_grace_ledgers(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::CancelGraceLedgers)
            .unwrap_or(0)
    }

    /// Sets the per-donor stream cap. Admin-gated.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// client.set_max_streams_per_donor(&5);
    /// assert_eq!(client.max_streams_per_donor(), 5);
    /// ```
    pub fn set_max_streams_per_donor(env: Env, limit: u64) -> Result<(), Error> {
        require_admin(&env)?;
        env.storage()
            .instance()
            .set(&DataKey::MaxStreamsPerDonor, &limit);
        extend_instance_ttl(&env);
        MaxStreamsPerDonorSetEvent { limit }.publish(&env);
        Ok(())
    }

    /// Reads back the configured per-donor stream cap, or the default if
    /// `set_max_streams_per_donor` has never been called.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// assert_eq!(client.max_streams_per_donor(), 100);
    /// ```
    pub fn max_streams_per_donor(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::MaxStreamsPerDonor)
            .unwrap_or(DEFAULT_MAX_STREAMS_PER_DONOR)
    }

    /// Sets an optional ceiling on a stream's per-second `rate`, in token
    /// units. Admin-gated. `create_stream` and `modify_rate` reject any rate
    /// above it with `Error::RateTooHigh`.
    ///
    /// This is off by default: until an admin calls it, rates are uncapped
    /// and a typo of, say, a thousand times too large is accepted. Setting a
    /// ceiling lets the operator catch such mistakes at the contract edge
    /// rather than after the deposit is drained. `max_rate` must be positive;
    /// to remove the ceiling use `clear_max_rate`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// client.set_max_rate(&1_000);
    /// assert_eq!(client.max_rate(), Some(1_000));
    /// ```
    pub fn set_max_rate(env: Env, max_rate: i128) -> Result<(), Error> {
        require_admin(&env)?;
        if max_rate <= 0 {
            return Err(Error::InvalidAmount);
        }
        env.storage().instance().set(&DataKey::MaxRate, &max_rate);
        extend_instance_ttl(&env);

        MaxRateSetEvent { max_rate }.publish(&env);

        Ok(())
    }

    /// Removes any configured per-second rate ceiling, so subsequent
    /// `create_stream` and `modify_rate` calls accept any positive rate.
    /// Admin-gated. Emits a `maxrtclr` event.
    ///
    /// If no ceiling was set, this is a clean no-op returning `Ok(())`.
    pub fn clear_max_rate(env: Env) -> Result<(), Error> {
        require_admin(&env)?;
        env.storage().instance().remove(&DataKey::MaxRate);
        extend_instance_ttl(&env);

        MaxRateClearedEvent { data: () }.publish(&env);

        Ok(())
    }

    /// Reads back the configured maximum per-second rate, if any. `None`
    /// until an admin sets one. See `set_max_rate`.
    pub fn max_rate(env: Env) -> Option<i128> {
        max_rate_cap(&env)
    }

    /// Sets an optional ceiling on how large a single stream's balance may
    /// be, in token units. Admin-gated. `create_stream` rejects an initial
    /// deposit above it, and `top_up` rejects any top-up whose amount would
    /// push the post-settlement balance above it, both with
    /// `Error::DepositTooHigh`.
    ///
    /// This is off by default: until an admin calls it, stream balances are
    /// uncapped. Setting a ceiling lets an operator limit exposure during
    /// an early launch without preventing ordinary-sized donations.
    /// `max_deposit` must be positive; to remove the ceiling use
    /// `clear_max_deposit_per_stream`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultArgs, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, DonationVaultArgs::__constructor(&Address::generate(&env)));
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// client.set_max_deposit_per_stream(&10_000);
    /// assert_eq!(client.max_deposit_per_stream(), Some(10_000));
    /// ```
    pub fn set_max_deposit_per_stream(env: Env, max_deposit: i128) -> Result<(), Error> {
        require_admin(&env)?;
        if max_deposit <= 0 {
            return Err(Error::InvalidAmount);
        }
        env.storage()
            .instance()
            .set(&DataKey::MaxDepositPerStream, &max_deposit);
        extend_instance_ttl(&env);

        MaxDepositPerStreamSetEvent { max_deposit }.publish(&env);

        Ok(())
    }

    /// Removes the per-stream deposit ceiling so `create_stream` and `top_up`
    /// accept any positive amount. Admin-gated. Emits a `maxdepclr` event.
    ///
    /// If no ceiling was set this is a clean no-op returning `Ok(())`.
    pub fn clear_max_deposit_per_stream(env: Env) -> Result<(), Error> {
        require_admin(&env)?;
        env.storage()
            .instance()
            .remove(&DataKey::MaxDepositPerStream);
        extend_instance_ttl(&env);

        MaxDepositPerStreamClearedEvent { data: () }.publish(&env);

        Ok(())
    }

    /// Reads back the configured maximum balance per stream, if any. `None`
    /// until an admin sets one. See `set_max_deposit_per_stream`.
    pub fn max_deposit_per_stream(env: Env) -> Option<i128> {
        max_deposit_per_stream_cap(&env)
    }

    /// Sets the shortest allowed time to depletion, in seconds. Admin-gated.
    /// `create_stream` and `modify_rate` reject a stream whose deposit would
    /// drain in fewer than this many seconds with `Error::StreamTooShort`.
    ///
    /// This is off by default (`0`): until an admin sets it, a deposit can be
    /// configured to drain in a single second, which is rarely intended and
    /// wastes fees for everyone. Setting a floor catches a mistyped rate at
    /// the contract edge instead of after the deposit is gone. Passing `0`
    /// disables the check.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// client.set_min_duration(&60);
    /// assert_eq!(client.min_duration(), 60);
    /// ```
    pub fn set_min_duration(env: Env, min_duration: u64) -> Result<(), Error> {
        require_admin(&env)?;
        env.storage()
            .instance()
            .set(&DataKey::MinDuration, &min_duration);
        extend_instance_ttl(&env);

        MinDurationSetEvent { min_duration }.publish(&env);

        Ok(())
    }

    /// Reads back the configured minimum stream duration in seconds. `0`
    /// until an admin sets one, meaning no minimum is enforced. See
    /// `set_min_duration`.
    pub fn min_duration(env: Env) -> u64 {
        min_duration_seconds(&env)
    }

    /// Sets the NGO registry contract used to verify NGOs before a stream
    /// is opened. Admin-gated.
    ///
    /// The candidate is probed with `ngo_count` before being stored, so a
    /// plain account, the vault itself, or an unrelated contract is rejected
    /// with `Error::InvalidRegistry` and the previous registry is left
    /// untouched.
    ///
    /// To clear an existing registry, use `clear_registry`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// let registry = Address::generate(&env);
    /// client.set_registry(&registry);
    /// assert_eq!(client.registry(), Some(registry));
    /// ```
    pub fn set_registry(env: Env, registry: Address) -> Result<(), Error> {
        require_admin(&env)?;
        let previous_registry = env.storage().instance().get(&DataKey::Registry);

        // Probe the candidate with a harmless registry read before storing
        // it. A plain account, the vault itself, or an unrelated contract
        // does not answer `ngo_count`, so it is rejected here instead of
        // being stored and failing every NGO verification afterwards.
        NgoRegistryClient::new(&env, &registry)
            .try_ngo_count()
            .map_err(|_| Error::InvalidRegistry)?
            .map_err(|_| Error::InvalidRegistry)?;

        env.storage().instance().set(&DataKey::Registry, &registry);
        extend_instance_ttl(&env);

        RegistrySetEvent {
            registry,
            previous_registry,
        }
        .publish(&env);

        Ok(())
    }

    /// Clears the configured NGO registry address, disabling registry verification
    /// for subsequent streams. Admin-gated.
    ///
    /// # Trade-off
    ///
    /// Clearing the registry turns off NGO verification for all new streams opened
    /// while the registry is unset ΓÇö any recipient address (as long as it differs from
    /// the donor) will be accepted. Use `set_registry` to configure a new registry.
    ///
    /// If no registry was set, this is an idempotent no-op (mirroring `clear_treasury`),
    /// removing the key if present, extending instance TTL, and publishing the `regclr` event.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// let registry = Address::generate(&env);
    /// client.set_registry(&registry);
    /// assert_eq!(client.registry(), Some(registry));
    ///
    /// client.clear_registry();
    /// assert_eq!(client.registry(), None);
    /// ```
    pub fn clear_registry(env: Env) -> Result<(), Error> {
        require_admin(&env)?;
        env.storage().instance().remove(&DataKey::Registry);
        extend_instance_ttl(&env);

        RegistryClearedEvent { data: () }.publish(&env);

        Ok(())
    }

    /// Reads back the configured NGO registry address, if any.
    pub fn registry(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Registry)
    }

    /// Sets the address that receives this NGO's withdrawals. Owner-auth-gated,
    /// so only the NGO's own owner key can redirect its payouts.
    ///
    /// Once set, `withdraw` and `withdraw_batch` pay the net amount to this
    /// address instead of the NGO owner address, while the owner key continues
    /// to authorise those calls. That lets an NGO route funds to a treasury or
    /// multisig while keeping a hot signing key for authority only. With no
    /// payout address set, payouts go to the owner address as before.
    ///
    /// Emits a `payoutset` event carrying the NGO and the new address so an
    /// indexer can track payout routing without polling `payout_address`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// let ngo = Address::generate(&env);
    /// let treasury = Address::generate(&env);
    /// client.set_payout_address(&ngo, &treasury);
    /// assert_eq!(client.payout_address(&ngo), Some(treasury));
    /// ```
    pub fn set_payout_address(
        env: Env,
        ngo: Address,
        payout_address: Address,
    ) -> Result<(), Error> {
        ngo.require_auth();

        let key = DataKey::NgoPayoutAddress(ngo.clone());
        env.storage().persistent().set(&key, &payout_address);
        env.storage().persistent().extend_ttl(
            &key,
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
        extend_instance_ttl(&env);

        PayoutAddressSetEvent {
            ngo,
            payout: payout_address,
        }
        .publish(&env);

        Ok(())
    }

    /// Reads back the payout address an NGO has configured, if any.
    ///
    /// `None` means withdrawals are paid to the NGO owner address itself.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let ngo = Address::generate(&env);
    /// assert_eq!(client.payout_address(&ngo), None);
    /// ```
    pub fn payout_address(env: Env, ngo: Address) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::NgoPayoutAddress(ngo))
    }

    /// Previews a new stream before it is created: how long `deposit` will
    /// last at `rate`, the fee in basis points that applies to `token`, and
    /// the first validation error `create_stream` would return for those
    /// parameters.
    ///
    /// This is a read-only view: it never pulls funds, writes storage, or
    /// requires auth. It reuses the same token/deposit/rate and configured
    /// limit checks as `create_stream`, so a non-`None` `error` is exactly
    /// what the real call would fail with for those inputs. Donor-, NGO-,
    /// auth- and registry-specific checks are outside the preview because
    /// they depend on arguments this view does not take.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// let token = Address::generate(&env);
    /// let preview = client.preview_stream(&token, &1_000, &10);
    /// assert_eq!(preview.duration, Some(100));
    /// assert_eq!(preview.error, None);
    /// ```
    pub fn preview_stream(env: Env, token: Address, deposit: i128, rate: i128) -> StreamPreview {
        let fee_bps = effective_fee_bps(&env, &token);
        let duration = if deposit > 0 {
            math::seconds_to_deplete(rate, deposit)
        } else {
            None
        };

        let error = require_not_paused(&env)
            .err()
            .or_else(|| validate_stream_token_and_amount(&env, &token, deposit, rate).err())
            .or_else(|| validate_stream_config_limits(&env, deposit, rate).err())
            .map(|error| error as u32);

        StreamPreview {
            duration,
            fee_bps,
            error,
        }
    }

    /// Opens a new stream: pulls `deposit` of `token` from the donor into the
    /// vault, to be released to the NGO at `rate` per second on withdrawal.
    /// `donor` and `ngo` must be distinct addresses, and neither `ngo` nor
    /// `token` may be the vault's own address (`Error::InvalidNgo` /
    /// `Error::InvalidToken`).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient, Error};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &1_000);
    /// // Stream 1_000 units of the token to `ngo` at 10 units/second.
    /// let stream_id = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    /// assert_eq!(client.get_stream(&stream_id).balance, 1_000);
    ///
    /// // A stream needs two distinct parties ΓÇö the vault refuses to pay a
    /// // donor back their own deposit.
    /// let result = client.try_create_stream(&donor, &donor, &sac.address(), &1_000, &10);
    /// assert_eq!(result, Err(Ok(Error::SelfStream)));
    /// ```
    ///
    /// Begins accruing immediately, at the current ledger time ΓÇö equivalent to
    /// `create_stream_with_start` with `start` equal to now. Use
    /// `create_stream_with_start` instead to hold a stream behind a cliff
    /// (issue #337).
    pub fn create_stream(
        env: Env,
        donor: Address,
        ngo: Address,
        token: Address,
        deposit: i128,
        rate: i128,
    ) -> Result<u64, Error> {
        create_stream_inner(
            env.clone(),
            donor,
            ngo,
            token,
            deposit,
            rate,
            None,
            Bytes::new(&env),
        )
    }

    /// Creates a stream with a cliff: it stays idle until `start`, so nothing
    /// accrues before that ledger timestamp (issue #337). Use this for pledges
    /// that should begin on a future date, or after a trial period, instead of
    /// `create_stream`.
    ///
    /// The deposit is still pulled at creation; it just doesn't start
    /// unlocking until `start`. `pending_accrual` stays 0 before the cliff and
    /// `depletion_time` is measured from `start` (plus the deposit's duration)
    /// rather than from creation. Cancelling before `start` refunds the whole
    /// deposit, because nothing has accrued.
    ///
    /// `start` may be at or before the current ledger time ΓÇö the stream then
    /// behaves exactly like `create_stream` ΓÇö but not more than
    /// `MAX_START_DELAY_SECONDS` (one year) ahead of it; a further timestamp is
    /// rejected with `Error::StartTooFar` so a mistyped cliff can't strand the
    /// deposit. Every other check in `create_stream` applies unchanged.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::{Address as _, Ledger}, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &1_000);
    /// // Start paying 100 seconds from now.
    /// let start = env.ledger().timestamp() + 100;
    /// let stream_id =
    ///     client.create_stream_with_start(&donor, &ngo, &sac.address(), &1_000, &10, &start);
    ///
    /// // Nothing accrues before the cliff.
    /// env.ledger().with_mut(|l| l.timestamp += 50);
    /// assert_eq!(client.pending_accrual(&stream_id), 0);
    ///
    /// // Once it passes, accrual is dated from `start`, not creation.
    /// env.ledger().with_mut(|l| l.timestamp += 100);
    /// assert_eq!(client.pending_accrual(&stream_id), 500);
    /// ```
    pub fn create_stream_with_start(
        env: Env,
        donor: Address,
        ngo: Address,
        token: Address,
        deposit: i128,
        rate: i128,
        start: u64,
    ) -> Result<u64, Error> {
        create_stream_inner(
            env.clone(),
            donor,
            ngo,
            token,
            deposit,
            rate,
            Some(start),
            Bytes::new(&env),
        )
    }

    /// Creates a stream that begins accruing immediately, tagged with an
    /// optional `memo` (issue #341). Use this to attach a short label to a
    /// stream ΓÇö a campaign id, an invoice reference ΓÇö that callers can read
    /// back from `get_stream` and from the `created` event.
    ///
    /// The memo is capped at `MAX_MEMO_BYTES` (32) bytes; a longer one is
    /// rejected with `Error::MemoTooLong` rather than truncated, so a donor
    /// never believes a tag was stored when only part of it was. Pass
    /// `Bytes::new(&env)` for a stream with no memo. Every other check in
    /// `create_stream` applies unchanged.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, token, Address, Bytes, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &1_000);
    /// let memo = Bytes::from_array(&env, b"campaign-42");
    /// let stream_id =
    ///     client.create_stream_with_memo(&donor, &ngo, &sac.address(), &1_000, &10, &memo);
    /// assert_eq!(client.get_stream(&stream_id).memo, memo);
    /// ```
    pub fn create_stream_with_memo(
        env: Env,
        donor: Address,
        ngo: Address,
        token: Address,
        deposit: i128,
        rate: i128,
        memo: Bytes,
    ) -> Result<u64, Error> {
        create_stream_inner(env, donor, ngo, token, deposit, rate, None, memo)
    }

    /// Pauses a stream after settling all accrued funds. The balance and rate
    /// are retained, while the stream no longer accrues until resumed.
    ///
    /// Fails with `StreamCancelled` for a cancelled stream, `StreamAlreadyPaused`
    /// for a paused one, `StreamDrained` for one that already paid out in full,
    /// and `StreamEmpty` for one with a zero balance that is none of those.
    pub fn pause_stream(env: Env, stream_id: u64) -> Result<i128, Error> {
        require_not_paused(&env)?;
        let mut stream = load_stream(&env, stream_id)?;

        stream.donor.require_auth();
        if stream.cancelled {
            return Err(Error::StreamCancelled);
        }
        if stream.status == StreamStatus::Paused {
            return Err(Error::StreamAlreadyPaused);
        }
        if stream.status == StreamStatus::Drained {
            return Err(Error::StreamDrained);
        }
        if stream.balance == 0 {
            return Err(Error::StreamEmpty);
        }

        let now = env.ledger().timestamp();
        let settled = settle(&env, stream_id, &mut stream, now)?;
        if stream.balance == 0 {
            stream.status = StreamStatus::Drained;
        } else {
            stream.status = StreamStatus::Paused;
        }
        save_stream(&env, stream_id, &stream);
        extend_instance_ttl(&env);

        PausedStreamEvent { stream_id, settled }.publish(&env);
        Ok(settled)
    }

    /// Replaces the donor's tag on an existing stream (issue #441). Donor-auth
    /// gated. Accepts any memo up to `MAX_MEMO_BYTES`, including an empty one
    /// to clear the tag, and rejects a cancelled stream with
    /// `Error::StreamCancelled`. Publishes the old and new memo on the
    /// `memoset` event.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, token, Address, Bytes, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &1_000);
    /// let memo = Bytes::from_array(&env, b"campaign-42");
    /// let stream_id =
    ///     client.create_stream_with_memo(&donor, &ngo, &sac.address(), &1_000, &10, &memo);
    ///
    /// let renamed = Bytes::from_array(&env, b"campaign-43");
    /// client.set_memo(&stream_id, &renamed);
    /// assert_eq!(client.get_stream(&stream_id).memo, renamed);
    /// ```
    pub fn set_memo(env: Env, stream_id: u64, memo: Bytes) -> Result<(), Error> {
        let mut stream = load_stream(&env, stream_id)?;

        stream.donor.require_auth();

        if stream.cancelled {
            return Err(Error::StreamCancelled);
        }

        // Same cap as the create entry points (issue #341): reject a memo
        // past `MAX_MEMO_BYTES` rather than store only part of it.
        if memo.len() > MAX_MEMO_BYTES {
            return Err(Error::MemoTooLong);
        }

        let old_memo = stream.memo.clone();
        stream.memo = memo.clone();
        save_stream(&env, stream_id, &stream);
        extend_instance_ttl(&env);

        MemoSetEvent {
            stream_id,
            old_memo,
            new_memo: memo,
        }
        .publish(&env);

        Ok(())
    }

    /// Resumes a paused stream. No time elapsed while paused is added to accrual.
    pub fn resume_stream(env: Env, stream_id: u64) -> Result<(), Error> {
        require_not_paused(&env)?;
        let mut stream = load_stream(&env, stream_id)?;

        stream.donor.require_auth();
        if stream.cancelled {
            return Err(Error::StreamCancelled);
        }
        if stream.status != StreamStatus::Paused {
            return Err(Error::StreamNotPaused);
        }

        // Re-check that the NGO is still verified before resuming accrual.
        // A donor must not be able to restart payments to an NGO whose
        // verification was revoked while the stream was paused.
        require_verified_ngo(&env, &stream.ngo)?;

        stream.status = StreamStatus::Active;
        stream.last_update = env.ledger().timestamp();
        save_stream(&env, stream_id, &stream);
        extend_instance_ttl(&env);

        ResumedStreamEvent { stream_id }.publish(&env);
        Ok(())
    }

    /// Pays out everything accrued to the NGO since the last checkpoint.
    /// NGO-auth-gated.
    ///
    /// Returns the net amount the NGO actually receives: the gross accrued
    /// amount minus any protocol fee routed to the treasury. With no
    /// treasury configured, or when the fee rounds down to zero, that equals
    /// the full accrued amount. The stream's `withdrawn` bookkeeping and the
    /// `withdraw` event still report the gross accrued value.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::{Address as _, Ledger}, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &1_000);
    /// let stream_id = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    ///
    /// // 50 seconds pass -> 10/s * 50 = 500 has accrued.
    /// env.ledger().with_mut(|l| l.timestamp += 50);
    ///
    /// // No treasury is configured here, so the return value is the full
    /// // accrued amount. With a fee configured it would be net of that fee.
    /// let withdrawn = client.withdraw(&stream_id);
    /// assert_eq!(withdrawn, 500);
    /// ```
    pub fn withdraw(env: Env, stream_id: u64) -> Result<i128, Error> {
        require_not_paused(&env)?;

        let mut stream = load_stream(&env, stream_id)?;

        stream.ngo.require_auth();

        if stream.status == StreamStatus::Paused {
            return Err(Error::StreamPaused);
        }

        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(accrual_base(&stream));
        let accrued = math::accrued(stream.rate, elapsed, stream.balance);

        if accrued <= 0 {
            return Err(Error::NothingToWithdraw);
        }

        record_payout(&env, &mut stream, accrued)?;
        stream.last_update = now;
        let drained = stream.balance == 0;
        if drained {
            stream.status = StreamStatus::Drained;
        }
        save_stream(&env, stream_id, &stream);
        extend_instance_ttl(&env);

        // The accrued amount is leaving the vault, so it no longer counts
        // against the token's committed balance (issue #346).
        add_committed(&env, &stream.token, -accrued)?;

        // Keep the lifetime outflow totals in step with the payout, matching
        // the shared settle path used by the other entry points.
        let fee = compute_fee(&env, &stream.token, accrued);
        record_outflow_totals(&env, accrued, fee)?;

        let token_client = token::Client::new(&env, &stream.token);
        let payout = payout_destination(&env, &stream.ngo);
        let fee = compute_fee(&env, &stream.token, accrued);
        record_outflow_totals(&env, accrued, fee)?;
        let net = pay_ngo(&env, &token_client, &payout, accrued);

        WithdrawnEvent {
            stream_id,
            accrued,
            fee,
            net,
            payout,
        }
        .publish(&env);

        // Announce the terminal transition after the payout and `withdraw`
        // event, so an indexer observes them in order (issue #344).
        if drained {
            DrainedEvent {
                stream_id,
                total_paid: stream.withdrawn,
            }
            .publish(&env);
        }

        Ok(net)
    }

    /// Withdraws from several streams in a single transaction, settling each
    /// one exactly as `withdraw` would but aggregating the gross accruals per
    /// token so the vault makes only one transfer per token regardless of how
    /// many streams contributed to it.
    ///
    /// Every stream must belong to the same NGO, which authorizes the call
    /// once. Streams that have accrued nothing since their last checkpoint
    /// are skipped so an NGO can pass its full stream list without filtering
    /// it first; a stream id that doesn't exist (or an arithmetic overflow)
    /// aborts the whole batch atomically. The protocol fee is computed per
    /// stream (matching `withdraw` and `pending_payout`), then the net amounts
    /// are aggregated per token for transfer.
    ///
    /// Returns `Error::BatchLimitExceeded` if more than `MAX_BATCH_WITHDRAW_STREAMS`
    /// ids are provided. Returns `Error::DuplicateStreamId` if the same id appears
    /// multiple times. Returns `Error::NothingToWithdraw` if no stream in the batch
    /// has accrued anything.
    ///
    /// Returns the net payout for each input stream, in the same
    /// order as `stream_ids` (0 for a stream that had nothing to withdraw).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::{Address as _, Ledger}, token, Address, Env, Vec};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &2_000);
    /// let a = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    /// let b = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    /// env.ledger().with_mut(|l| l.timestamp += 50);
    ///
    /// let mut ids = Vec::new(&env);
    /// ids.push_back(a);
    /// ids.push_back(b);
    /// // 500 accrued on each, paid out to the NGO in one token transfer.
    /// let withdrawn = client.withdraw_batch(&ids);
    /// assert_eq!(withdrawn.get(0), Some(500));
    /// assert_eq!(withdrawn.get(1), Some(500));
    /// ```
    pub fn withdraw_batch(env: Env, stream_ids: Vec<u64>) -> Result<Vec<i128>, Error> {
        require_not_paused(&env)?;

        // Cap the number of streams per batch to stay within CPU and footprint budget
        if stream_ids.len() > MAX_BATCH_WITHDRAW_STREAMS {
            return Err(Error::BatchLimitExceeded);
        }

        // Reject duplicate stream IDs
        let mut seen_ids: Map<u64, bool> = Map::new(&env);
        for stream_id in stream_ids.iter() {
            if seen_ids.contains_key(stream_id) {
                return Err(Error::DuplicateStreamId);
            }
            seen_ids.set(stream_id, true);
        }

        let now = env.ledger().timestamp();
        let mut ngo: Option<Address> = None;
        let mut net_amounts: Vec<i128> = Vec::new(&env);
        let mut accrued_amounts: Vec<i128> = Vec::new(&env);
        let mut net_payouts: Map<Address, i128> = Map::new(&env);
        let mut fee_payouts: Map<Address, i128> = Map::new(&env);
        let mut gross_payouts: Map<Address, i128> = Map::new(&env);
        // Streams this batch empties, mapped to the water mark of their
        // cumulative payout, so the `drained` event can be published after the
        // transfers alongside each `withdraw` (issue #344).
        let mut drained_totals: Map<u64, i128> = Map::new(&env);
        let mut total_accrued: i128 = 0;

        for stream_id in stream_ids.iter() {
            let mut stream = load_stream(&env, stream_id)?;

            if stream.status == StreamStatus::Paused {
                return Err(Error::StreamPaused);
            }

            // Every stream in a batch has to share one NGO: the payouts are
            // summed per token and sent to a single address, so a batch that
            // mixed NGOs would silently pay the first one for the others'
            // streams. Requiring the NGO's auth here also stops a caller from
            // draining streams that aren't theirs.
            match &ngo {
                None => {
                    stream.ngo.require_auth();
                    ngo = Some(stream.ngo.clone());
                }
                Some(existing) if existing != &stream.ngo => return Err(Error::MixedNgo),
                Some(_) => {}
            }

            let elapsed = now.saturating_sub(accrual_base(&stream));
            let accrued = math::accrued(stream.rate, elapsed, stream.balance);
            accrued_amounts.push_back(accrued);
            total_accrued = total_accrued
                .checked_add(accrued)
                .ok_or(Error::ArithmeticOverflow)?;

            let fee = if accrued > 0 {
                compute_fee(&env, &stream.token, accrued)
            } else {
                0
            };
            let net = accrued.checked_sub(fee).ok_or(Error::ArithmeticOverflow)?;
            net_amounts.push_back(net);

            if accrued <= 0 {
                continue;
            }

            record_payout(&env, &mut stream, accrued)?;
            stream.last_update = now;
            if stream.balance == 0 {
                stream.status = StreamStatus::Drained;
                drained_totals.set(stream_id, stream.withdrawn);
            }
            save_stream(&env, stream_id, &stream);

            // Accumulate net amounts per token
            let net_total = net_payouts.get(stream.token.clone()).unwrap_or(0);
            net_payouts.set(
                stream.token.clone(),
                net_total
                    .checked_add(net)
                    .ok_or(Error::ArithmeticOverflow)?,
            );

            // Accumulate fees per token
            if fee > 0 {
                let fee_total = fee_payouts.get(stream.token.clone()).unwrap_or(0);
                fee_payouts.set(
                    stream.token.clone(),
                    fee_total
                        .checked_add(fee)
                        .ok_or(Error::ArithmeticOverflow)?,
                );
            }

            // Accumulate the gross amount leaving the vault per token, so the
            // committed balance can be reduced by exactly what was paid out
            // (issue #346).
            let gross_total = gross_payouts.get(stream.token.clone()).unwrap_or(0);
            gross_payouts.set(
                stream.token.clone(),
                gross_total
                    .checked_add(accrued)
                    .ok_or(Error::ArithmeticOverflow)?,
            );
        }

        // Return NothingToWithdraw when no stream pays anything
        if total_accrued <= 0 {
            return Err(Error::NothingToWithdraw);
        }

        let mut total_fees: i128 = 0;
        for (_, fee) in fee_payouts.iter() {
            total_fees = total_fees
                .checked_add(fee)
                .ok_or(Error::ArithmeticOverflow)?;
        }
        record_outflow_totals(&env, total_accrued, total_fees)?;

        // The gross amount per token is leaving the vault, so drop it from the
        // committed balance before the transfers (issue #346).
        for (token, gross) in gross_payouts.iter() {
            if gross > 0 {
                add_committed(&env, &token, -gross)?;
            }
        }

        // Every contributing stream shares one NGO (enforced above), so a
        // single resolution covers the whole batch. Withdrawals go to the
        // NGO's configured payout address, or the NGO itself when unset.
        let ngo = ngo.ok_or(Error::NothingToWithdraw)?;
        extend_instance_ttl(&env);
        let payout = payout_destination(&env, &ngo);

        // One net transfer per token to the NGO's payout address
        for (token, net_total) in net_payouts.iter() {
            if net_total > 0 {
                let token_client = token::Client::new(&env, &token);
                token_client.transfer(&env.current_contract_address(), &payout, &net_total);
            }
        }

        // One fee transfer per token to treasury
        let treasury: Option<Address> = env.storage().instance().get(&DataKey::Treasury);
        if let Some(treasury_address) = treasury {
            for (token, fee_total) in fee_payouts.iter() {
                if fee_total > 0 {
                    let token_client = token::Client::new(&env, &token);
                    token_client.transfer(
                        &env.current_contract_address(),
                        &treasury_address,
                        &fee_total,
                    );
                }
            }
        }

        // Emit a `withdraw` per contributing stream, matching `withdraw`'s
        // topics and data so the indexer needs no batch-specific handling.
        for (idx, stream_id) in stream_ids.iter().enumerate() {
            let idx = idx as u32;
            let accrued = accrued_amounts.get(idx).unwrap_or(0);
            let net = net_amounts.get(idx).unwrap_or(0);

            if accrued > 0 {
                let fee = fee_from_payout(accrued, net)?;
                WithdrawnEvent {
                    stream_id,
                    accrued,
                    fee,
                    net,
                    payout: payout.clone(),
                }
                .publish(&env);

                // Follow the `withdraw` for any stream this batch emptied,
                // matching `withdraw`'s withdraw-then-drained order (#344).
                if let Some(total_paid) = drained_totals.get(stream_id) {
                    DrainedEvent {
                        stream_id,
                        total_paid,
                    }
                    .publish(&env);
                }
            }
        }

        Ok(net_amounts)
    }

    /// Withdraws from every stream an NGO owns, a bounded page at a time, using
    /// the NGO's stream index so the caller never has to list stream ids
    /// off-chain (issue #342).
    ///
    /// `start` and `limit` select the page: streams are taken from the NGO's
    /// index in creation order from position `start`, up to `limit` of them.
    /// `limit` is clamped to `MAX_BATCH_WITHDRAW_STREAMS`, matching
    /// `withdraw_batch`'s footprint budget. Each stream is settled exactly as
    /// `withdraw` would, and the page is returned with the cursor to pass as
    /// `start` next time; iteration is complete when `next` reaches the NGO's
    /// stream count (`streams_by_ngo(ngo, 0, ..).len()`).
    ///
    /// The NGO authorizes the call once. A page whose streams have all accrued
    /// nothing is not an error: it returns one `0` per stream in the page, so
    /// the caller can simply advance the cursor and keep paging.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::{Address as _, Ledger}, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &3_000);
    /// let _a = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    /// let _b = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    /// env.ledger().with_mut(|l| l.timestamp += 50);
    ///
    /// // Collect from this NGO's first (and only) page of streams.
    /// let page = client.withdraw_all(&ngo, &0, &20);
    /// assert_eq!(page.amounts.len(), 2);
    /// assert_eq!(page.next, 2);
    /// ```
    pub fn withdraw_all(
        env: Env,
        ngo: Address,
        start: u32,
        limit: u32,
    ) -> Result<WithdrawAllPage, Error> {
        require_not_paused(&env)?;

        let index: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::NgoStreams(ngo))
            .unwrap_or_else(|| Vec::new(&env));
        let total = index.len();

        if limit == 0 || start >= total {
            return Ok(WithdrawAllPage {
                amounts: Vec::new(&env),
                next: total,
            });
        }

        let effective_limit = limit.min(MAX_BATCH_WITHDRAW_STREAMS);
        let end = start
            .checked_add(effective_limit)
            .unwrap_or(total)
            .min(total);

        let mut page = Vec::new(&env);
        for position in start..end {
            if let Some(stream_id) = index.get(position) {
                page.push_back(stream_id);
            }
        }

        // Settle the page exactly like `withdraw_batch` (one transfer per
        // token plus a `withdraw` event per stream). A page that accrues
        // nothing is normal mid-pagination, so it yields zero entries rather
        // than an error, letting the caller advance the cursor.
        let page_len = page.len();
        let amounts = match Self::withdraw_batch(env.clone(), page) {
            Ok(amounts) => amounts,
            Err(Error::NothingToWithdraw) => {
                let mut zeros = Vec::new(&env);
                for _ in 0..page_len {
                    zeros.push_back(0i128);
                }
                zeros
            }
            Err(error) => return Err(error),
        };

        Ok(WithdrawAllPage { amounts, next: end })
    }

    /// Stops a stream for good: settles whatever has already accrued to the
    /// NGO, refunds the untouched remainder to the donor, then zeroes the
    /// stream's rate and balance. Donor-auth-gated. Returns the refunded
    /// amount.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::{Address as _, Ledger}, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &1_000);
    /// let stream_id = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    /// env.ledger().with_mut(|l| l.timestamp += 20); // 200 accrues
    ///
    /// // Settles the 200 already accrued to the NGO, refunds the
    /// // untouched 800 to the donor, and zeroes the stream out.
    /// let refund = client.cancel_stream(&stream_id);
    /// assert_eq!(refund, 800);
    /// assert_eq!(client.get_stream(&stream_id).balance, 0);
    /// assert!(client.get_stream(&stream_id).cancelled);
    /// ```
    pub fn cancel_stream(env: Env, stream_id: u64) -> Result<i128, Error> {
        let stream = load_stream(&env, stream_id)?;

        stream.donor.require_auth();

        // Return early if the stream is already cancelled to avoid rewriting storage
        if stream.cancelled {
            return Err(Error::StreamCancelled);
        }

        let now = env.ledger().timestamp();
        let grace_ledgers = env
            .storage()
            .instance()
            .get(&DataKey::CancelGraceLedgers)
            .unwrap_or(0);
        extend_instance_ttl(&env);

        cancel_stream_inner(&env, stream_id, stream, now, grace_ledgers)
    }

    /// Cancels several of one donor's streams in a single transaction,
    /// settling and refunding each one exactly as `cancel_stream` would. The
    /// donor authorizes the call once, instead of signing and paying a
    /// separate fee per stream.
    ///
    /// The batch is all-or-nothing: every id is checked before any stream is
    /// settled or any token moves, so one unknown id, an already-cancelled
    /// stream, or a stream belonging to another donor rejects the whole call
    /// with no partial cancellation.
    ///
    /// Returns `Error::BatchLimitExceeded` if more than
    /// `MAX_BATCH_CANCEL_STREAMS` ids are provided, `Error::DuplicateStreamId`
    /// if the same id appears twice, `Error::StreamNotFound` for an unknown id,
    /// `Error::StreamCancelled` for a stream already closed out, and
    /// `Error::NotStreamOwner` if any stream belongs to a different donor.
    ///
    /// Returns the refund for each input stream, in the same order as
    /// `stream_ids`. Like `cancel_stream`, it works while the vault is paused.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::{Address as _, Ledger}, token, Address, Env, Vec};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &2_000);
    /// let a = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    /// let b = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    /// env.ledger().with_mut(|l| l.timestamp += 20); // 200 accrues on each
    ///
    /// let mut ids = Vec::new(&env);
    /// ids.push_back(a);
    /// ids.push_back(b);
    /// let refunds = client.cancel_streams(&ids);
    /// assert_eq!(refunds.get(0), Some(800));
    /// assert_eq!(refunds.get(1), Some(800));
    /// ```
    pub fn cancel_streams(env: Env, stream_ids: Vec<u64>) -> Result<Vec<i128>, Error> {
        if stream_ids.len() > MAX_BATCH_CANCEL_STREAMS {
            return Err(Error::BatchLimitExceeded);
        }

        // A repeated id would be cancelled twice: the second pass would find it
        // already cancelled and fail after the first refund had moved, so
        // reject duplicates up front like `withdraw_batch` does.
        let mut seen_ids: Map<u64, bool> = Map::new(&env);
        for stream_id in stream_ids.iter() {
            if seen_ids.contains_key(stream_id) {
                return Err(Error::DuplicateStreamId);
            }
            seen_ids.set(stream_id, true);
        }

        // An empty batch is a no-op rather than an error: there is nothing to
        // cancel and nothing to refund, so no auth is needed either.
        if stream_ids.is_empty() {
            return Ok(Vec::new(&env));
        }

        // First pass: validate the whole batch before anything moves, so an
        // unknown, already-cancelled, or foreign stream aborts the call with
        // no partial cancellation. All ids must share one donor, which
        // authorizes the batch once.
        let mut donor: Option<Address> = None;
        let mut streams: Vec<Stream> = Vec::new(&env);
        for stream_id in stream_ids.iter() {
            let stream = load_stream(&env, stream_id)?;

            match &donor {
                None => donor = Some(stream.donor.clone()),
                Some(existing) if existing != &stream.donor => return Err(Error::NotStreamOwner),
                Some(_) => {}
            }

            if stream.cancelled {
                return Err(Error::StreamCancelled);
            }

            streams.push_back(stream);
        }

        let donor = donor.ok_or(Error::StreamNotFound)?;
        donor.require_auth();

        let now = env.ledger().timestamp();
        let grace_ledgers = env
            .storage()
            .instance()
            .get(&DataKey::CancelGraceLedgers)
            .unwrap_or(0);

        // Second pass: settle and refund each stream. Any failure here (an
        // arithmetic overflow, a failed token transfer) aborts the whole
        // transaction, so the batch still cannot leave a half-cancelled set.
        let mut refunds: Vec<i128> = Vec::new(&env);
        for (idx, stream_id) in stream_ids.iter().enumerate() {
            let stream = streams.get(idx as u32).ok_or(Error::StreamNotFound)?;
            refunds.push_back(cancel_stream_inner(
                &env,
                stream_id,
                stream,
                now,
                grace_ledgers,
            )?);
        }

        extend_instance_ttl(&env);

        Ok(refunds)
    }

    /// Admin-only emergency escape hatch for a paused vault (issue #199).
    ///
    /// Unlike `cancel_stream`, this does not settle any accrued amount to
    /// the NGO first ΓÇö a pause is an incident response, not a normal
    /// wind-down, and the point is to get the donor's funds out with as
    /// little contract logic in the way as possible. The full remaining
    /// `balance` refunds to the donor, and the stream is marked cancelled
    /// exactly as `cancel_stream` would leave it, so a rescued stream can't
    /// later be topped up, rate-modified, or (successfully) cancelled
    /// again.
    ///
    /// Deliberately the mirror image of every other fund-moving entry
    /// point: those all reject while paused so nothing unexpected moves
    /// during an incident; this one requires paused so it can only ever be
    /// used for the emergency it exists for, not as an ordinary way to
    /// close out a stream.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::{Address as _, Ledger}, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &1_000);
    /// let stream_id = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    /// env.ledger().with_mut(|l| l.timestamp += 20); // 200 would have accrued
    ///
    /// client.pause();
    /// // The full 1_000 goes back to the donor -- the 200 that would have
    /// // accrued to the NGO under a normal cancel is not settled first.
    /// let refund = client.rescue_stream(&stream_id);
    /// assert_eq!(refund, 1_000);
    /// assert_eq!(client.get_stream(&stream_id).balance, 0);
    /// assert!(client.get_stream(&stream_id).cancelled);
    /// ```
    pub fn rescue_stream(env: Env, stream_id: u64) -> Result<i128, Error> {
        require_admin(&env)?;

        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if !paused {
            return Err(Error::NotPaused);
        }

        let mut stream = load_stream(&env, stream_id)?;

        if stream.cancelled {
            return Err(Error::StreamCancelled);
        }

        let refund = stream.balance;
        if refund > 0 {
            update_donor_totals(&env, &stream.donor, &stream.token, -refund, 0)?;
            // The refunded remainder is no longer held for the stream, so it
            // leaves the token's committed balance (issue #346).
            add_committed(&env, &stream.token, -refund)?;
            let token_client = token::Client::new(&env, &stream.token);
            token_client.transfer(&env.current_contract_address(), &stream.donor, &refund);
        }

        let now = env.ledger().timestamp();
        stream.balance = 0;
        stream.rate = 0;
        stream.cancelled = true;
        stream.last_update = now;
        stream.status = StreamStatus::Cancelled;
        save_stream(&env, stream_id, &stream);
        extend_instance_ttl(&env);
        let grace_ledgers = env
            .storage()
            .instance()
            .get(&DataKey::CancelGraceLedgers)
            .unwrap_or(0);
        extend_cancelled_stream_ttl(&env, stream_id, grace_ledgers)?;

        RescuedStreamEvent { stream_id, refund }.publish(&env);

        Ok(refund)
    }

    /// Adds more funds to an existing stream. Donor-auth-gated. Settles
    /// whatever has already accrued to the NGO first, so the top-up only
    /// ever affects accrual going forward. Fails with
    /// `Error::StreamCancelled` if `cancel_stream` has already closed the
    /// stream out.
    ///
    /// If an NGO registry is configured, re-verifies that the stream's NGO
    /// is approved and active in the registry. If the NGO has been revoked,
    /// fails with `Error::NgoNotVerified` before settling, modifying balances,
    /// or pulling funds.
    ///
    /// Existing funds in a stream whose NGO was revoked remain protected:
    /// the NGO may still withdraw already-accrued funds, and the donor can
    /// still cancel the stream to receive a refund of unaccrued funds.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::{Address as _, Ledger}, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &2_000);
    /// let stream_id = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    /// env.ledger().with_mut(|l| l.timestamp += 10); // 100 accrues and settles first
    ///
    /// client.top_up(&stream_id, &500);
    /// assert_eq!(client.get_stream(&stream_id).balance, 1_400); // 1000 - 100 + 500
    /// ```
    pub fn top_up(env: Env, stream_id: u64, amount: i128) -> Result<(), Error> {
        require_not_paused(&env)?;

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if amount < Self::min_deposit(env.clone()) {
            return Err(Error::DepositTooLow);
        }

        let mut stream = load_stream(&env, stream_id)?;

        stream.donor.require_auth();

        if stream.cancelled {
            return Err(Error::StreamCancelled);
        }

        // Re-check that the NGO is still verified in the registry before
        // settling, updating state, or transferring new funds.
        require_verified_ngo(&env, &stream.ngo)?;

        let token_client = token::Client::new(&env, &stream.token);

        let now = env.ledger().timestamp();
        let settled = settle_core(&env, &mut stream, now)?;
        update_donor_totals(&env, &stream.donor, &stream.token, amount, 0)?;

        // Optional per-stream deposit ceiling: reject the top-up if it would
        // push the stream's post-settlement balance above the configured cap.
        if let Some(max_dep) = max_deposit_per_stream_cap(&env) {
            let new_balance = stream
                .balance
                .checked_add(amount)
                .ok_or(Error::ArithmeticOverflow)?;
            if new_balance > max_dep {
                return Err(Error::DepositTooHigh);
            }
        }

        // Update state before the external token transfer
        stream.balance = stream
            .balance
            .checked_add(amount)
            .ok_or(Error::ArithmeticOverflow)?;

        // Reactivate a drained stream if it now has a positive balance
        if stream.status == StreamStatus::Drained && stream.balance > 0 {
            stream.status = StreamStatus::Active;
        }

        let total_donated: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalDonated)
            .unwrap_or(0);
        let next_total = total_donated
            .checked_add(amount)
            .ok_or(Error::ArithmeticOverflow)?;

        env.storage()
            .instance()
            .set(&DataKey::TotalDonated, &next_total);
        save_stream(&env, stream_id, &stream);
        extend_instance_ttl(&env);

        // The top-up joins this stream's share of the token's committed
        // balance before it is pulled in (issue #346).
        add_committed(&env, &stream.token, amount)?;

        // External token transfer is the last step
        token_client.transfer(&stream.donor, env.current_contract_address(), &amount);

        ToppedUpEvent {
            stream_id,
            amount,
            settled,
        }
        .publish(&env);

        Ok(())
    }

    /// Changes the per-second accrual rate on an existing stream. Donor-auth-gated.
    /// Settles whatever has already accrued at the old rate first, so the new
    /// rate only ever applies going forward ΓÇö never retroactively. Fails
    /// with `Error::StreamCancelled` if `cancel_stream` has already closed
    /// the stream out ΓÇö otherwise this would quietly revive it.
    ///
    /// If an NGO registry is configured, a rate *increase* re-verifies the
    /// stream's NGO, the same check `top_up` and `transfer_stream` apply,
    /// and fails with `Error::NgoNotVerified` before settling if the NGO's
    /// verification was revoked. A rate *decrease* is always allowed, so a
    /// donor can slow the flow to a revoked NGO without cancelling.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::{Address as _, Ledger}, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// # let donor = Address::generate(&env);
    /// # let ngo = Address::generate(&env);
    /// # token_client.mint(&donor, &1_000);
    /// let stream_id = client.create_stream(&donor, &ngo, &sac.address(), &1_000, &10);
    /// env.ledger().with_mut(|l| l.timestamp += 5); // 50 accrues at the old rate first
    ///
    /// client.modify_rate(&stream_id, &20);
    /// assert_eq!(client.get_stream(&stream_id).rate, 20);
    /// ```
    pub fn modify_rate(env: Env, stream_id: u64, new_rate: i128) -> Result<(), Error> {
        require_not_paused(&env)?;

        if new_rate <= 0 {
            return Err(Error::InvalidAmount);
        }
        // Same optional ceiling as `create_stream` (issue #339): a new rate
        // above the admin-configured maximum is rejected before any state
        // is written.
        if let Some(max_rate) = max_rate_cap(&env) {
            if new_rate > max_rate {
                return Err(Error::RateTooHigh);
            }
        }

        let mut stream = load_stream(&env, stream_id)?;

        stream.donor.require_auth();
        let old_rate = stream.rate;

        if stream.cancelled {
            return Err(Error::StreamCancelled);
        }

        if new_rate == stream.rate {
            return Ok(());
        }

        // A rate increase sends more to the NGO, so ΓÇö like `top_up` and
        // `transfer_stream` ΓÇö it requires the NGO to still be verified when
        // a registry is configured. A decrease only reduces what flows to
        // the NGO, so it stays available even after a revocation.
        if new_rate > stream.rate {
            require_verified_ngo(&env, &stream.ngo)?;
        }

        // Optional minimum lifespan (issue #340): the stream must still last
        // at least `min_duration` seconds at the new rate. Checked against the
        // remaining balance before settling, so a rejection leaves the stream
        // ΓÇö and its funds ΓÇö exactly as they were.
        if depletes_before_min_duration(stream.balance, new_rate, min_duration_seconds(&env)) {
            return Err(Error::StreamTooShort);
        }

        let now = env.ledger().timestamp();
        let settled = settle(&env, stream_id, &mut stream, now)?;
        stream.rate = new_rate;

        save_stream(&env, stream_id, &stream);
        extend_instance_ttl(&env);

        RateModifiedEvent {
            stream_id,
            old_rate,
            new_rate,
            settled,
        }
        .publish(&env);

        Ok(())
    }

    /// Reassigns an ongoing stream to a different NGO. Donor-auth-gated.
    /// Settles whatever has already accrued to the old NGO first, so future
    /// accrual only flows to `new_ngo`. Emits a `transfer` event carrying
    /// the old and new NGO addresses and the settled amount.
    pub fn transfer_stream(env: Env, stream_id: u64, new_ngo: Address) -> Result<(), Error> {
        require_not_paused(&env)?;

        let mut stream = load_stream(&env, stream_id)?;

        stream.donor.require_auth();

        if stream.cancelled {
            return Err(Error::StreamCancelled);
        }

        if new_ngo == stream.donor {
            return Err(Error::SelfStream);
        }

        if new_ngo == stream.ngo {
            return Ok(());
        }

        require_verified_ngo(&env, &new_ngo)?;

        let old_ngo = stream.ngo.clone();
        let now = env.ledger().timestamp();
        let settled = settle(&env, stream_id, &mut stream, now)?;
        if stream.balance == 0 {
            stream.status = StreamStatus::Drained;
        }
        stream.ngo = new_ngo.clone();

        save_stream(&env, stream_id, &stream);

        let old_key = DataKey::NgoStreams(old_ngo.clone());
        if let Some(old_streams) = env.storage().persistent().get::<_, Vec<u64>>(&old_key) {
            let mut remaining = Vec::new(&env);
            for id in old_streams.iter() {
                if id != stream_id {
                    remaining.push_back(id);
                }
            }
            env.storage().persistent().set(&old_key, &remaining);
        }
        let new_key = DataKey::NgoStreams(new_ngo.clone());
        let mut new_streams: Vec<u64> = env
            .storage()
            .persistent()
            .get(&new_key)
            .unwrap_or_else(|| Vec::new(&env));
        new_streams.push_back(stream_id);
        env.storage().persistent().set(&new_key, &new_streams);
        env.storage().persistent().extend_ttl(
            &new_key,
            STREAM_LIFETIME_THRESHOLD,
            STREAM_BUMP_AMOUNT,
        );
        extend_instance_ttl(&env);

        TransferStreamEvent {
            stream_id,
            old_ngo,
            new_ngo,
            settled,
        }
        .publish(&env);

        Ok(())
    }

    /// Reads the admin-configured upgrade timelock, in seconds. Zero means the
    /// timelock is disabled and `upgrade` replaces the code immediately.
    pub fn upgrade_delay(env: Env) -> u64 {
        get_upgrade_delay(&env)
    }

    /// Reads the upgrade proposed by `propose_upgrade`, if one is awaiting its
    /// timelock. `None` when nothing is pending.
    pub fn pending_upgrade(env: Env) -> Option<PendingUpgrade> {
        env.storage().instance().get(&DataKey::PendingUpgrade)
    }

    /// Reads the amount of `token` the vault holds on behalf of streams (the
    /// sum of stream balances for that token). See issue #346.
    pub fn committed_balance(env: Env, token: Address) -> i128 {
        get_committed(&env, &token)
    }

    /// Reads how much of `token` the vault holds *above* its committed
    /// balance ΓÇö the surplus `sweep_surplus` is allowed to move. A token sent
    /// to the vault by mistake surfaces here. See issue #346.
    pub fn surplus(env: Env, token: Address) -> i128 {
        let held = token::Client::new(&env, &token).balance(&env.current_contract_address());
        held.saturating_sub(get_committed(&env, &token))
    }

    /// Sets how many seconds must pass between `propose_upgrade` and
    /// `execute_upgrade`. Admin-gated; emits an `updelay` event. Setting it to
    /// zero explicitly disables the timelock and re-enables the immediate
    /// `upgrade` entry point.
    pub fn set_upgrade_delay(env: Env, delay: u64) -> Result<(), Error> {
        require_admin(&env)?;
        env.storage().instance().set(&DataKey::UpgradeDelay, &delay);
        extend_instance_ttl(&env);

        UpgradeDelaySetEvent { delay }.publish(&env);

        Ok(())
    }

    /// Starts a timelocked upgrade by recording `new_wasm_hash` as pending, to
    /// be carried out by `execute_upgrade` no earlier than the configured
    /// `upgrade_delay` seconds from now. Admin-gated; emits an `uppropose`
    /// event. Proposing again replaces any pending proposal and restarts the
    /// timer.
    pub fn propose_upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        require_admin(&env)?;
        let delay = get_upgrade_delay(&env);
        let executable_at = env
            .ledger()
            .timestamp()
            .checked_add(delay)
            .ok_or(Error::ArithmeticOverflow)?;
        env.storage().instance().set(
            &DataKey::PendingUpgrade,
            &PendingUpgrade {
                new_wasm_hash: new_wasm_hash.clone(),
                executable_at,
            },
        );
        extend_instance_ttl(&env);

        UpgradeProposedEvent {
            new_wasm_hash,
            executable_at,
        }
        .publish(&env);

        Ok(())
    }

    /// Carries out the upgrade proposed by `propose_upgrade`, replacing the
    /// contract's Wasm bytecode in place. Admin-gated; emits a
    /// `config_snapshot` of the current configuration followed by an `upgrade`
    /// event. Fails with `NoPendingUpgrade` when nothing is proposed, and with
    /// `UpgradeDelayNotElapsed` before the timelock has elapsed.
    pub fn execute_upgrade(env: Env) -> Result<(), Error> {
        require_admin(&env)?;
        let pending: PendingUpgrade = env
            .storage()
            .instance()
            .get(&DataKey::PendingUpgrade)
            .ok_or(Error::NoPendingUpgrade)?;
        if env.ledger().timestamp() < pending.executable_at {
            return Err(Error::UpgradeDelayNotElapsed);
        }

        env.storage().instance().remove(&DataKey::PendingUpgrade);
        env.deployer()
            .update_current_contract_wasm(pending.new_wasm_hash.clone());
        extend_instance_ttl(&env);

        // The upgraded code starts from this configuration; publish it before
        // the `upgrade` event so that event remains the call's last one.
        ConfigSnapshotEvent {
            config: DonationVault::get_config(env.clone()),
        }
        .publish(&env);

        UpgradedEvent {
            new_wasm_hash: pending.new_wasm_hash,
        }
        .publish(&env);

        Ok(())
    }

    /// Cancels the upgrade proposed by `propose_upgrade`. Admin-gated; emits
    /// an `upcancel` event. Fails with `NoPendingUpgrade` when nothing is
    /// proposed.
    pub fn cancel_upgrade(env: Env) -> Result<(), Error> {
        require_admin(&env)?;
        let pending: PendingUpgrade = env
            .storage()
            .instance()
            .get(&DataKey::PendingUpgrade)
            .ok_or(Error::NoPendingUpgrade)?;
        env.storage().instance().remove(&DataKey::PendingUpgrade);
        extend_instance_ttl(&env);

        UpgradeCancelledEvent {
            new_wasm_hash: pending.new_wasm_hash,
        }
        .publish(&env);

        Ok(())
    }

    /// Sweeps tokens that were sent to the vault by mistake to the treasury.
    ///
    /// The vault's live balance of `token` is compared against the committed
    /// balance it holds on behalf of streams (`committed_balance`); `amount`
    /// may only cover the surplus above that, so a sweep can never dip into
    /// funds belonging to streams. Admin-gated; emits a `sweep` event naming
    /// the destination. Fails with `NoTreasury` when no treasury is
    /// configured, with `InvalidAmount` when `amount` is not positive, with
    /// `InvalidSweepDestination` when the treasury is the vault itself, and
    /// with `SweepExceedsSurplus` when `amount` exceeds the surplus.
    ///
    /// Use `sweep_surplus_to` to send a sweep to an address other than the
    /// configured treasury (issue #439).
    pub fn sweep_surplus(env: Env, token: Address, amount: i128) -> Result<(), Error> {
        sweep_surplus_inner(env, token, amount, None)
    }

    /// Sweeps tokens that were sent to the vault by mistake to an explicit
    /// `destination`, without requiring a treasury to be configured.
    ///
    /// Applies exactly the same accounting as `sweep_surplus`: only the
    /// balance above the streams' committed total can move, so a sweep can
    /// never touch donor funds. The `sweep` event names `destination`, so indexers can follow misplaced tokens to
    /// wherever the admin sent them. Admin-gated; fails with `InvalidAmount`
    /// when `amount` is not positive, with `InvalidSweepDestination` when
    /// `destination` is the vault itself, and with `SweepExceedsSurplus` when
    /// `amount` exceeds the surplus.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, token, Address, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # let donor = Address::generate(&env);
    /// # let token_admin = Address::generate(&env);
    /// # let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    /// # let token_client = token::StellarAssetClient::new(&env, &sac.address());
    /// let recovered = Address::generate(&env);
    /// token_client.mint(&donor, &500);
    /// token_client.transfer(&donor, &client.address, &500);
    ///
    /// // 500 held against nothing committed: recover it to an explicit address.
    /// client.sweep_surplus_to(&sac.address(), &500, &recovered);
    /// assert_eq!(token_client.balance(&recovered), 500);
    /// ```
    pub fn sweep_surplus_to(
        env: Env,
        token: Address,
        amount: i128,
        destination: Address,
    ) -> Result<(), Error> {
        sweep_surplus_inner(env, token, amount, Some(destination))
    }

    /// Replaces the contract's Wasm bytecode in place. Admin-only.
    /// Lets a bug fix be deployed without changing the contract address,
    /// preserving every existing stream and configuration value.
    ///
    /// Only allowed while the upgrade timelock is explicitly disabled (the
    /// configured `upgrade_delay` is zero). With a non-zero delay, upgrades
    /// must go through `propose_upgrade` / `execute_upgrade` so donors holding
    /// open streams get the warning period; this returns
    /// `Error::UpgradeTimelockEnabled` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};
    /// # use donation_vault::{DonationVault, DonationVaultClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(DonationVault, ());
    /// # let client = DonationVaultClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let new_wasm_hash = BytesN::from_array(&env, &[0u8; 32]);
    /// client.upgrade(&new_wasm_hash);
    /// ```
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        require_admin(&env)?;
        if get_upgrade_delay(&env) != 0 {
            return Err(Error::UpgradeTimelockEnabled);
        }
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        UpgradedEvent { new_wasm_hash }.publish(&env);
        Ok(())
    }
}

#[cfg(test)]
mod test;
