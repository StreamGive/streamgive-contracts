use soroban_sdk::{
    contractclient, contracterror, contracttype, Address, Bytes, BytesN, Env, String, Vec,
};

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

/// Mirrors ngo-registry's `Ngo` record for cross-contract calls. Declared
/// locally rather than imported from the `ngo-registry` crate: depending on
/// its source directly would pull that crate's own `#[contractimpl]` exports
/// into this contract's Wasm link unit, colliding with this contract's
/// identically-named entry points (`admin`, `init`, `upgrade`, ...).
#[contracttype]
#[derive(Clone, Debug)]
pub struct NgoRecord {
    pub owner: Address,
    pub name: String,
    pub verified: bool,
}

/// Thin cross-contract interface onto the configured ngo-registry contract.
/// Only the one method `create_stream` needs. See `NgoRecord` for why this
/// isn't just imported from the `ngo-registry` crate.
#[contractclient(name = "NgoRegistryClient")]
#[allow(dead_code)]
pub trait NgoRegistryInterface {
    fn get_ngo(env: Env, owner: Address) -> NgoRecord;
}

/// A single donor -> NGO streaming donation.
///
/// `balance` is the undrawn amount still deposited in the vault; `rate` is
/// how much of it accrues to the NGO per second. `created_at` is set once,
/// by `create_stream`, and never changes; `last_update` moves forward on
/// every checkpoint (withdraw, cancel, top-up, or rate change).
#[contracttype]
// Debug and PartialEq let tests assert_eq! on a try_* call's full
// Result<Result<Stream, _>, _> rather than unwrapping it by hand first,
// and compare a whole stream at once instead of field by field.
#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    pub donor: Address,
    pub ngo: Address,
    pub token: Address,
    pub rate: i128,
    pub balance: i128,
    pub withdrawn: i128,
    pub created_at: u64,
    /// When this stream begins paying out. Equal to `created_at` for a plain
    /// `create_stream`; later than it for `create_stream_with_start`. No time
    /// before this timestamp accrues, so a cliff stream stays idle until it
    /// arrives. See issue #337.
    pub start: u64,
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

/// All admin configuration and global contract parameters returned by `get_config`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub paused: bool,
    pub treasury: Option<Address>,
    pub fee_bps: u32,
    pub min_deposit: i128,
    pub max_streams_per_donor: u64,
    /// The highest per-second `rate` accepted by `create_stream` and
    /// `modify_rate`, when an admin has configured one. `None` means no cap.
    pub max_rate: Option<i128>,
    /// The shortest time to depletion, in seconds, an admin will accept for a
    /// new or re-rated stream. `0` means no minimum is enforced. See
    /// `set_min_duration` and issue #340.
    pub min_duration: u64,
    pub registry: Option<Address>,
    pub cancel_grace_ledgers: u32,
    pub total_donated: i128,
    pub total_withdrawn: i128,
    pub total_fees_collected: i128,
    pub pending_admin: Option<Address>,
    pub renounced: bool,
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
    /// [`allowed_tokens`](crate::DonationVault::allowed_tokens); empty until an
    /// operator configures one.
    AllowedTokens,
    /// Admin-settable per-donor stream cap. See `set_max_streams_per_donor`.
    MaxStreamsPerDonor,
    /// Count of streams a donor currently has open. Incremented on
    /// `create_stream`, never decremented (streams are cancelled, not
    /// deleted) — so this is really a lifetime cap, not a live cap.
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
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    StreamNotFound = 3,
    InvalidAmount = 4,
    NothingToWithdraw = 5,
    ContractPaused = 6,
    FeeTooHigh = 7,
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
    AlreadyPaused = 12,
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
    /// silently treated as `0` — that could collide with an existing
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
    /// A creation entry point was given a `memo` longer than `MAX_MEMO_BYTES`.
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

/// Maximum number of NGO stream ids returned by `streams_by_ngo` in one page.
pub const MAX_STREAMS_BY_NGO: u32 = 100;

pub const MAX_FEE_BPS: u32 = 1_000;

/// Default per-donor stream cap applied when `set_max_streams_per_donor`
/// has not been called. Chosen so an ordinary donor can open hundreds of
/// streams across many NGOs, but a flood attack hits the ceiling long
/// before it can bloat persistent storage. See issue #94.
pub const DEFAULT_MAX_STREAMS_PER_DONOR: u64 = 100;

/// Longest a `create_stream_with_start` cliff may be pushed into the future
/// (issue #337). A scheduled pledge or trial period needs a delayed start, but
/// an unbounded one is easy to mistype and would strand the deposit behind an
/// unreachable date. One year covers the realistic cases while keeping the
/// cliff human-scale.
pub const MAX_START_DELAY_SECONDS: u64 = 365 * 24 * 60 * 60;

/// Longest stream memo accepted by the create entry points (issue #341). A memo
/// is a short tag (a campaign id, say), not a payload — 32 bytes is plenty and
/// keeps the stored record and `created` event small.
pub const MAX_MEMO_BYTES: u32 = 32;

/// Approximate ledgers per day at a 5-second close time. Used to express
/// storage TTLs (which the network counts in ledgers, not wall time) in
/// human terms.
pub const DAY_IN_LEDGERS: u32 = 17_280;

pub const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
pub const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

pub const STREAM_BUMP_AMOUNT: u32 = 90 * DAY_IN_LEDGERS;
pub const STREAM_LIFETIME_THRESHOLD: u32 = STREAM_BUMP_AMOUNT - DAY_IN_LEDGERS;
