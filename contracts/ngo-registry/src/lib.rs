// SPDX-License-Identifier: Apache-2.0
//! NGO registry contract for the StreamGive protocol.
//!
//! Maintains a registry of registered and verified non-governmental organisations
//! (NGOs). Admins and approvers can approve, reject, revoke, and renew entries;
//! the registry is queried by the donation-vault contract before a stream is opened.
#![no_std]
#![allow(missing_docs)]
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, Bytes, BytesN,
    Env, String, Vec,
};
pub use streamgive_types::Ngo;

use common::{
    INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD, PERSISTENT_BUMP_AMOUNT,
    PERSISTENT_LIFETIME_THRESHOLD,
};

/// Event emitted when an NGO registers.
#[contractevent(topics = ["register"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct RegisterEvent {
    #[topic]
    pub owner: Address,
    pub name: String,
}

/// Event emitted when an NGO renames its pending application.
#[contractevent(topics = ["renamed"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct RenamedEvent {
    #[topic]
    pub owner: Address,
    pub name: String,
}

/// Event emitted when an NGO is approved.
#[contractevent(topics = ["approved"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct ApprovedEvent {
    #[topic]
    pub owner: Address,
    pub data: (),
}

/// Event emitted when an NGO's verification is revoked.
#[contractevent(topics = ["revoked"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct RevokedEvent {
    #[topic]
    pub owner: Address,
    pub data: (),
}

/// Event emitted when an NGO unregisters itself.
#[contractevent(topics = ["unregist"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct UnregisteredEvent {
    #[topic]
    pub owner: Address,
    pub data: (),
}

/// Event emitted when an admin removes an NGO entry.
#[contractevent(topics = ["removed"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct RemovedEvent {
    #[topic]
    pub owner: Address,
    pub data: (),
}

/// Event emitted when an NGO application is rejected.
#[contractevent(topics = ["rejected"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct RejectedEvent {
    #[topic]
    pub owner: Address,
    pub reason_code: u32,
}

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

/// Event emitted when an admin proposal is cancelled.
#[contractevent(topics = ["canceladm"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct CancelledAdminEvent {
    pub data: (),
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

/// Event emitted when an upgrade is proposed.
#[contractevent(topics = ["uppropose"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct UpgradeProposedEvent {
    #[topic]
    pub new_wasm_hash: BytesN<32>,
    pub executable_at: u64,
}

/// Event emitted when a proposed upgrade is cancelled.
#[contractevent(topics = ["upcancel"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct UpgradeCancelledEvent {
    pub new_wasm_hash: BytesN<32>,
}

/// Event emitted when the upgrade delay is set.
#[contractevent(topics = ["updelay"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct UpgradeDelaySetEvent {
    pub delay: u64,
}

/// Event emitted when an approver is added.
#[contractevent(topics = ["apadd"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct ApproverAddedEvent {
    #[topic]
    pub approver: Address,
    pub data: (),
}

/// Event emitted when an approver is removed.
#[contractevent(topics = ["aprem"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct ApproverRemovedEvent {
    #[topic]
    pub approver: Address,
    pub data: (),
}

/// Event emitted when an NGO's metadata URI is set or cleared.
#[contractevent(topics = ["metadata"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct MetadataUriSetEvent {
    #[topic]
    pub owner: Address,
    pub metadata_uri: Option<String>,
}

/// Event emitted when a verification is renewed.
#[contractevent(topics = ["renew"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct RenewedEvent {
    #[topic]
    pub owner: Address,
    /// The new verification expiry, as a ledger timestamp.
    pub verified_until: u64,
}

/// Event emitted when the verification period is configured.
#[contractevent(topics = ["verperiod"], data_format = "single-value")]
#[allow(missing_docs)]
pub struct VerifyPeriodSetEvent {
    pub verify_period: u64,
}

/// Event emitted when an NGO's category bitmask is set.
#[contractevent(topics = ["catset"], data_format = "single-value")]
pub struct CategoriesSetEvent {
    #[topic]
    pub owner: Address,
    /// The new bitmask value. Each set bit corresponds to one `CATEGORY_*`
    /// constant; zero means no categories assigned.
    pub categories: u32,
}

/// Event emitted when an NGO owner proposes an ownership transfer.
#[contractevent(topics = ["ownprop"], data_format = "vec")]
pub struct OwnershipProposedEvent {
    #[topic]
    pub current_owner: Address,
    pub new_owner: Address,
}

/// Event emitted when a proposed NGO ownership transfer is accepted.
#[contractevent(topics = ["ownaccpt"], data_format = "vec")]
pub struct OwnershipAcceptedEvent {
    #[topic]
    pub old_owner: Address,
    pub new_owner: Address,
}

/// Event emitted when a proposed NGO ownership transfer is cancelled.
#[contractevent(topics = ["owncancel"], data_format = "single-value")]
pub struct OwnershipCancelledEvent {
    #[topic]
    pub owner: Address,
    pub data: (),
}

/// NGO category bitmask constants.
///
/// Each constant is a single bit. An NGO's `categories` field is the bitwise
/// OR of whichever categories apply. Zero means uncategorised.
pub const CATEGORY_HEALTH: u32 = 1 << 0;
pub const CATEGORY_EDUCATION: u32 = 1 << 1;
pub const CATEGORY_ENVIRONMENT: u32 = 1 << 2;
pub const CATEGORY_HUMANITAR: u32 = 1 << 3;
pub const CATEGORY_ANIMALS: u32 = 1 << 4;
pub const CATEGORY_ARTS: u32 = 1 << 5;
pub const CATEGORY_COMMUNITY: u32 = 1 << 6;
pub const CATEGORY_RESEARCH: u32 = 1 << 7;

/// The union of every defined category bit. Any bit set in a caller-supplied
/// value that is *not* in this mask is a reserved bit and causes
/// `Error::InvalidCategories`.
pub const VALID_CATEGORY_MASK: u32 = CATEGORY_HEALTH
    | CATEGORY_EDUCATION
    | CATEGORY_ENVIRONMENT
    | CATEGORY_HUMANITAR
    | CATEGORY_ANIMALS
    | CATEGORY_ARTS
    | CATEGORY_COMMUNITY
    | CATEGORY_RESEARCH;

#[contractevent(topics = ["regrpause"], data_format = "single-value")]
pub struct RegistrationsPausedEvent {
    pub data: (),
}

#[contractevent(topics = ["regresume"], data_format = "single-value")]
pub struct RegistrationsResumedEvent {
    pub data: (),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Ngo {
    /// The address that registered and owns this NGO entry.
    pub owner: Address,
    /// The display name of the NGO, bounded by `MAX_NGO_NAME_LEN`.
    pub name: String,
    /// Whether an admin or approver has approved this entry.
    pub verified: bool,
    /// Optional owner-supplied reference to off-chain metadata.
    pub metadata_uri: Option<String>,
    /// Optional ledger timestamp at which verification lapses.
    pub verified_until: Option<u64>,
/// OR of `CATEGORY_*` constants. `0` means uncategorised. Set via
/// `set_categories`; locked once the NGO is verified (the owner must
/// request revocation to change it after approval). Readable via
/// `categories()` or `get_ngo()`.
pub categories: u32,

/// Optional pending new owner address. Set by `propose_owner_transfer` and
/// cleared by `accept_owner_transfer` or `cancel_owner_transfer`. `None` means
/// no ownership transfer is pending.
pub pending_owner: Option<Address>,
}

/// The verification state of an NGO owner address, returned by `verification_status`.
///
/// Distinguishes the five cases that `is_verified` collapses into a single
/// `false`: an address that was never registered (`Unknown`), one that is
/// registered but not yet reviewed (`Pending`), one that is currently
/// verified and not expired (`Verified`), one whose verification period has
/// lapsed (`Expired`), and one whose approval was explicitly revoked
/// (`Revoked`). See issue #448.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VerificationStatus {
    /// No entry exists for this address.
    Unknown,
    /// Registered but not yet approved or rejected.
    Pending,
    /// Approved and the verification has not expired.
    Verified,
    /// Was approved but the `verified_until` timestamp has passed.
    Expired,
    /// Approval was explicitly revoked by an admin or approver.
    Revoked,
}

/// A single page of the NGO owners index, returned by `list_ngos_page`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct NgoOwnerPage {
    /// The owner addresses in this page, in registry order.
    pub owners: Vec<Address>,
    /// The `start` to pass on the next call: one past the end of this page,
    /// clamped to the current owner count. Iteration is complete once it
    /// reaches `total_ngos()` (or `ngo_count()`).
    pub next: u32,
}

/// A contract upgrade recorded by `propose_upgrade`, awaiting the timelock
/// before `execute_upgrade` may carry it out.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PendingUpgrade {
    /// The Wasm hash the contract will be upgraded to on execution.
    pub new_wasm_hash: BytesN<32>,
    /// Ledger timestamp at or after which `execute_upgrade` may run.
    pub executable_at: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub enum DataKey {
    /// The address authorized to initialize and administer the registry.
    Admin,
    /// The administrator nominated to take over, pending acceptance.
    PendingAdmin,
    /// Unused and reserved. Preserved to maintain enum discriminant stability
    /// for existing deployed contracts. Callers should read `NgoCount` via `ngo_count()`.
    TotalNgos,
    /// The registry record keyed by an NGO owner's address.
    Ngo(Address),
    /// The total number of NGO records stored in the registry.
    NgoCount,
    /// Current number of registered NGO owners in the index.
    OwnerIndexCount,
    /// Registered NGO owner at a specific index in the owners index.
    OwnerByIndex(u32),
    /// Index of a registered NGO owner in the owners index.
    IndexByOwner(Address),
    /// Addresses (other than the admin) permitted to approve and revoke NGOs.
    Approvers,
    /// Current number of verified NGO records, maintained by the review and
    /// removal entry points and read by `verified_count`.
    VerifiedCount,
    /// Set when an NGO's approval is explicitly revoked. Cleared on
    /// re-approval. Used by `verification_status` to distinguish `Revoked`
    /// from `Pending`. See issue #448.
    NgoRevoked(Address),
    /// The verification period in seconds applied to new approvals and to
    /// `renew_ngo`. `0` (the default) means approvals never expire. See
    /// `set_verify_period`.
    VerifyPeriod,
    /// The owner that claimed an NGO name, keyed by the SHA-256 of the exact
    /// name bytes (a name can be up to 200 bytes, close to the 250-byte
    /// ledger key limit, so the name itself is not used as the key).
    /// Maintained by `register`, `update_name` and the removal entry points
    /// and read by `owner_by_name`. New variants are appended: never reorder
    /// this enum.
    OwnerByName(BytesN<32>),
    /// Whether new NGO registrations are paused by the admin.
    /// `true` while `pause_registrations` is in effect; absent or `false`
    /// means registrations are open. See `pause_registrations` / `resume_registrations`.
    RegistrationsPaused,
    /// Reverse index from a canonical NGO name to its owning address, used to
    /// enforce name uniqueness across registrations and renames.
    NameOwner(Bytes),
    /// Seconds that must elapse between `propose_upgrade` and
    /// `execute_upgrade`. Zero (or absent) means the timelock is disabled.
    UpgradeDelay,
    /// The upgrade recorded by `propose_upgrade` and not yet executed or
    /// cancelled. Absent means no upgrade is pending.
    PendingUpgrade,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// The registry has already been initialized and cannot be initialized again.
    AlreadyInitialized = 1,
    /// The registry has not been initialized yet.
    NotInitialized = 2,
    /// The owner address is already registered.
    AlreadyRegistered = 3,
    /// The owner address is not registered.
    NotRegistered = 4,
    /// The NGO has already been approved: it can no longer be unregistered,
    /// renamed, rejected, or approved again. Revoke it first.
    AlreadyVerified = 5,
    /// `name` is longer than `MAX_NGO_NAME_LEN` (200 bytes).
    NameTooLong = 6,
    /// `revoke_ngo` was called on an NGO that isn't currently verified.
    /// The NGO has not been approved, so it cannot be revoked.
    NotVerified = 7,
    /// `accept_admin` or `cancel_admin_proposal` was called without a prior
    /// (or already-completed) `propose_admin`.
    NoPendingAdmin = 8,
    /// The proposed administrator is not a valid replacement.
    InvalidAdmin = 9,
    /// An arithmetic operation overflowed or underflowed its type bounds.
    ArithmeticOverflow = 10,
    /// The supplied name is empty, contains only Unicode whitespace or invisible
    /// format characters, contains control characters, or is invalid UTF-8.
    InvalidName = 11,
    /// `approve_ngo`, `batch_approve`, or `revoke_ngo` was called by an address
    /// that is neither the admin nor a listed approver.
    NotApprover = 12,
    /// `add_approver` was called for an address already in the approver list.
    AlreadyApprover = 13,
    /// The supplied metadata URI is longer than `MAX_METADATA_URI_LEN`
    /// (512 bytes).
    MetadataUriTooLong = 14,
    /// `renew_ngo` was called while no verification period is configured, so
    /// there is no expiry to extend. Set one with `set_verify_period`.
    NoVerifyPeriod = 15,
    /// `register` was called while the admin has paused new registrations.
    /// Resume with `resume_registrations`.
    RegistrationsPaused = 16,
    /// The supplied canonical name is already held by another NGO.
    NameTaken = 17,
    /// `set_categories` was given a value with one or more reserved bits set.
    InvalidCategories = 18,
    /// `execute_upgrade` or `cancel_upgrade` was called with no upgrade proposed.
    NoPendingUpgrade = 19,
    /// `execute_upgrade` was called before the proposed upgrade's timelock elapsed.
    UpgradeDelayNotElapsed = 20,
    /// The immediate `upgrade` entry point was called while a non-zero
    /// `upgrade_delay` is configured.
    UpgradeTimelockEnabled = 21,
    // Error numbers are append-only: never renumber, reuse, or remove a
    // variant. New errors take the next unused number. See
    // docs/UPGRADE_POLICY.md.
}

/// Upper bound on `Ngo.name`, in bytes. Persistent storage cost scales with
/// what's stored, so without a cap a registration could inflate its own
/// entry's storage footprint indefinitely. Comfortably fits a real
/// organization name while keeping a single entry's storage bounded.
/// Note: this is strictly a byte limit (200 bytes), not a character limit.
pub const MAX_NGO_NAME_LEN: u32 = 200;

/// Upper bound on `Ngo.metadata_uri`, in bytes. As with `MAX_NGO_NAME_LEN`,
/// this keeps a single entry's storage footprint bounded; 512 bytes fits a
/// typical URL (including a logo or IPFS reference) with room to spare.
pub const MAX_METADATA_URI_LEN: u32 = 512;

/// Maximum number of NGO owners returned by `list_ngos` / `list_ngos_page` in
/// a single page. Caps read footprint, memory, and gas consumption to keep
/// pagination bounded.
pub const MAX_LIST_LIMIT: u32 = 50;

/// NGO-specific bump and threshold amounts, using the 90-day persistent entry constants.
const NGO_BUMP_AMOUNT: u32 = PERSISTENT_BUMP_AMOUNT;
const NGO_LIFETIME_THRESHOLD: u32 = PERSISTENT_LIFETIME_THRESHOLD;

fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

fn extend_ngo_ttl(env: &Env, owner: &Address) {
    env.storage().persistent().extend_ttl(
        &DataKey::Ngo(owner.clone()),
        NGO_LIFETIME_THRESHOLD,
        NGO_BUMP_AMOUNT,
    );
}

/// Loads an NGO from persistent storage, bumping its TTL on access so an
/// entry that is actively read or written cannot be archived out from under
/// its owner.
fn load_ngo(env: &Env, owner: &Address) -> Result<Ngo, Error> {
    let ngo: Ngo = env
        .storage()
        .persistent()
        .get(&DataKey::Ngo(owner.clone()))
        .ok_or(Error::NotRegistered)?;
    extend_ngo_ttl(env, owner);
    Ok(ngo)
}

/// Like [`load_ngo`] for paths where a missing entry is a normal result
/// (metadata lookups and index maintenance): returns `None` but still bumps
/// the TTL of every entry that exists.
fn try_load_ngo(env: &Env, owner: &Address) -> Option<Ngo> {
    let ngo: Ngo = env
        .storage()
        .persistent()
        .get(&DataKey::Ngo(owner.clone()))?;
    extend_ngo_ttl(env, owner);
    Some(ngo)
}

/// Saves an NGO to persistent storage and bumps its TTL in the same step, so
/// no caller can write an entry and forget to keep it alive.
fn save_ngo(env: &Env, owner: &Address, ngo: &Ngo) {
    env.storage()
        .persistent()
        .set(&DataKey::Ngo(owner.clone()), ngo);
    extend_ngo_ttl(env, owner);
}

fn extend_owner_index_ttl(env: &Env, index: u32, owner: &Address) {
    env.storage().persistent().extend_ttl(
        &DataKey::OwnerByIndex(index),
        NGO_LIFETIME_THRESHOLD,
        NGO_BUMP_AMOUNT,
    );
    env.storage().persistent().extend_ttl(
        &DataKey::IndexByOwner(owner.clone()),
        NGO_LIFETIME_THRESHOLD,
        NGO_BUMP_AMOUNT,
    );
}

/// The name-index key for `name`: the SHA-256 of its bytes.
fn name_key(env: &Env, name: &String) -> DataKey {
    let bytes: Bytes = name.into();
    DataKey::OwnerByName(env.crypto().sha256(&bytes).to_bytes())
}

fn extend_name_index_ttl(env: &Env, name: &String) {
    let key = name_key(env, name);
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, NGO_LIFETIME_THRESHOLD, NGO_BUMP_AMOUNT);
    }
}

/// Points `name` at `owner` in the name index, unless another owner already
/// holds the name: the first claimant keeps it until it renames or leaves,
/// and a later registrant with the same name is simply not indexed. (Names are
/// not unique yet; this keeps the index from silently flipping between
/// owners until they are.)
fn index_name(env: &Env, name: &String, owner: &Address) {
    let key = name_key(env, name);
    if !env.storage().persistent().has(&key) {
        env.storage().persistent().set(&key, owner);
    }
    if env.storage().persistent().get::<_, Address>(&key).as_ref() == Some(owner) {
        extend_name_index_ttl(env, name);
    }
}

/// Removes `name` from the name index, but only when `owner` is the one it
/// points at, so an owner leaving never clears another owner's claim.
fn unindex_name(env: &Env, name: &String, owner: &Address) {
    let key = name_key(env, name);
    if env.storage().persistent().get::<_, Address>(&key).as_ref() == Some(owner) {
        env.storage().persistent().remove(&key);
    }
}

fn get_owner_count(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::OwnerIndexCount)
        .unwrap_or(0)
}

fn set_owner_count(env: &Env, count: u32) {
    env.storage()
        .instance()
        .set(&DataKey::OwnerIndexCount, &count);
}

fn get_verified_count(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::VerifiedCount)
        .unwrap_or(0)
}

fn set_verified_count(env: &Env, count: u64) {
    env.storage()
        .instance()
        .set(&DataKey::VerifiedCount, &count);
}

/// Reads the configured verification period in seconds. `0` (the default)
/// means approvals never expire. See `set_verify_period`.
fn get_verify_period(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::VerifyPeriod)
        .unwrap_or(0)
}

/// Reads the admin-configured upgrade timelock, in seconds. Defaults to zero
/// (timelock disabled) when an admin has not set one.
fn get_upgrade_delay(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::UpgradeDelay)
        .unwrap_or(0)
}

/// Returns whether a verification expiry has lapsed at the current ledger
/// time. `None` never expires. An entry stays valid while
/// `now < verified_until`, so it is treated as unverified once `now` reaches
/// the stored timestamp. See issue #348.
fn has_expired(env: &Env, verified_until: Option<u64>) -> bool {
    match verified_until {
        Some(until) => env.ledger().timestamp() >= until,
        None => false,
    }
}

/// Returns whether a code point is an invisible or format character (category Cf).
///
/// Deliberately limited to zero-width spaces, joiners, word joiners, soft hyphens,
/// and byte order marks. These are treated as invisible (whitespace-like) for the
/// emptiness check: a name consisting solely of these characters and/or Unicode whitespace
/// is rejected. When present alongside visible characters, they are permitted (e.g.
/// for complex script ligatures).
/// Note: bidirectional override characters (such as U+202E) are not handled here and
/// should be tracked as a follow-up spoofing consideration.
fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{200B}' // Zero Width Space
        | '\u{200C}' // Zero Width Non-Joiner
        | '\u{200D}' // Zero Width Joiner
        | '\u{2060}' // Word Joiner
        | '\u{FEFF}' // Zero Width No-Break Space (BOM)
        | '\u{00AD}' // Soft Hyphen
        | '\u{200E}' // Left-to-Right Mark
        | '\u{200F}' // Right-to-Left Mark
    )
}

/// Validates a raw byte slice as an NGO name.
///
/// Rejects names longer than `MAX_NGO_NAME_LEN` (200 bytes) with `Error::NameTooLong`.
/// Rejects names that are empty, invalid UTF-8, contain any control character
/// (Unicode category Cc), or consist solely of Unicode whitespace and/or invisible
/// format characters with `Error::InvalidName`.
pub(crate) fn validate_name_bytes(bytes: &[u8]) -> Result<(), Error> {
    if bytes.len() > MAX_NGO_NAME_LEN as usize {
        return Err(Error::NameTooLong);
    }
    if bytes.is_empty() {
        return Err(Error::InvalidName);
    }
    let s = core::str::from_utf8(bytes).map_err(|_| Error::InvalidName)?;
    let mut has_visible = false;
    for c in s.chars() {
        if c.is_control() {
            return Err(Error::InvalidName);
        }
        if c.is_whitespace() || is_invisible_format(c) {
            continue;
        }
        has_visible = true;
    }
    if !has_visible {
        return Err(Error::InvalidName);
    }
    Ok(())
}

/// Validates an NGO name in Soroban SDK `String` format.
///
/// Copies into a stack buffer bounded by `MAX_NGO_NAME_LEN` (200 bytes)
/// without heap allocations. Replaces `is_whitespace_only`.
fn validate_name(name: &String) -> Result<(), Error> {
    if name.len() > MAX_NGO_NAME_LEN {
        return Err(Error::NameTooLong);
    }
    if name.is_empty() {
        return Err(Error::InvalidName);
    }
    let len = name.len() as usize;
    let mut buf = [0u8; MAX_NGO_NAME_LEN as usize];
    let bytes: Bytes = name.into();
    bytes.copy_into_slice(&mut buf[..len]);
    validate_name_bytes(&buf[..len])
}

/// Produces a canonical form of an NGO name for use as a uniqueness key.
///
/// The canonical form is the name with leading/trailing ASCII whitespace
/// trimmed and all ASCII letters lowercased. This means "Red Cross",
/// "  red cross  ", and "RED CROSS" all map to the same key, so a new
/// registrant cannot reserve a name that is visually indistinguishable from
/// an existing one. Non-ASCII characters (including Unicode whitespace) are
/// left unchanged.
///
/// Returns the canonical bytes as a `Bytes` value for use as a storage key.
/// The input must have already passed `validate_name`, so it is non-empty
/// valid UTF-8 with at least one visible character.
fn canonical_name_key(env: &Env, name: &String) -> Bytes {
    let len = name.len() as usize;
    let mut buf = [0u8; MAX_NGO_NAME_LEN as usize];
    let sdk_bytes: Bytes = name.into();
    sdk_bytes.copy_into_slice(&mut buf[..len]);
    let slice = &buf[..len];

    // Trim leading ASCII whitespace.
    let start = slice
        .iter()
        .position(|&b| !b.is_ascii_whitespace())
        .unwrap_or(len);
    // Trim trailing ASCII whitespace.
    let end = slice[start..]
        .iter()
        .rposition(|&b| !b.is_ascii_whitespace())
        .map(|i| start + i + 1)
        .unwrap_or(start);
    let trimmed = &slice[start..end];

    // Lowercase ASCII letters in place using a second stack buffer.
    let mut canonical = [0u8; MAX_NGO_NAME_LEN as usize];
    let canonical_len = trimmed.len();
    for (i, &b) in trimmed.iter().enumerate() {
        canonical[i] = b.to_ascii_lowercase();
    }

    Bytes::from_slice(env, &canonical[..canonical_len])
}

/// Validates an optional NGO metadata URI.
///
/// `None` is always allowed. `Some` must be at most `MAX_METADATA_URI_LEN`
/// (512 bytes) or the call fails with `Error::MetadataUriTooLong`. The URI is
/// opaque to the contract â€” an `https://`, `ipfs://`, or any other scheme is
/// accepted â€” so only the length is enforced. See issue #347.
fn validate_metadata_uri(uri: &Option<String>) -> Result<(), Error> {
    if let Some(u) = uri {
        if u.len() > MAX_METADATA_URI_LEN {
            return Err(Error::MetadataUriTooLong);
        }
    }
    Ok(())
}

/// Reads the configured admin and requires their auth, failing with
/// `Error::NotInitialized` if `init` hasn't been called yet. Shared by
/// every admin-gated entry point so the same three steps aren't repeated
/// at each call site.
fn require_admin(env: &Env) -> Result<Address, Error> {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::NotInitialized)?;
    admin.require_auth();
    Ok(admin)
}

/// Reads the stored approver list, or an empty list if none was set.
fn read_approvers(env: &Env) -> Vec<Address> {
    env.storage()
        .instance()
        .get(&DataKey::Approvers)
        .unwrap_or_else(|| Vec::new(env))
}

/// Returns whether `candidate` is in the approver list. The admin is not
/// stored here, so this reflects explicit approvers only.
fn is_approver_address(env: &Env, candidate: &Address) -> bool {
    read_approvers(env).contains(candidate)
}

/// Authorizes a review action (approve/revoke). The acting address must be
/// either the admin or a listed approver, and must have authorized the call.
/// Because a contract can't observe its own caller, the acting address is
/// passed in by the caller and checked here: this is what lets review work be
/// delegated without handing out the admin key. Fails with
/// `Error::NotApprover` for anyone else.
fn require_approver(env: &Env, candidate: &Address) -> Result<(), Error> {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::NotInitialized)?;
    if *candidate != admin && !is_approver_address(env, candidate) {
        return Err(Error::NotApprover);
    }
    candidate.require_auth();
    Ok(())
}

/// Returns `Err(Error::RegistrationsPaused)` if the admin has paused new
/// registrations. Called at the top of `register` to enforce the switch.
fn require_registrations_open(env: &Env) -> Result<(), Error> {
    let paused: bool = env
        .storage()
        .instance()
        .get(&DataKey::RegistrationsPaused)
        .unwrap_or(false);
    if paused {
        return Err(Error::RegistrationsPaused);
    }
    Ok(())
}

/// Marks one registered, unverified NGO as verified and publishes its
/// `approved` event. Shared by `approve_ngo` and `batch_approve` so a single
/// approval behaves the same whichever entry point drives it: an address with
/// no entry fails with `Error::NotRegistered`, and one that is already
/// verified with `Error::AlreadyVerified`.
fn approve_registered_ngo(env: &Env, ngo_owner: Address) -> Result<(), Error> {
    let mut ngo = load_ngo(env, &ngo_owner)?;
    if ngo.verified {
        return Err(Error::AlreadyVerified);
    }
    ngo.verified = true;
    // Stamp the verification expiry from the configured period (issue #348).
    // A zero period leaves `verified_until` as `None`, i.e. no expiry, which
    // is the default and matches the pre-expiry behaviour.
    let period = get_verify_period(env);
    ngo.verified_until = if period == 0 {
        None
    } else {
        Some(
            env.ledger()
                .timestamp()
                .checked_add(period)
                .ok_or(Error::ArithmeticOverflow)?,
        )
    };
    save_ngo(env, &ngo_owner, &ngo);
    // Clear any prior revocation flag so `verification_status` returns
    // `Verified` rather than `Revoked` after a re-approval. See issue #448.
    env.storage()
        .persistent()
        .remove(&DataKey::NgoRevoked(ngo_owner.clone()));

    let verified = get_verified_count(env);
    let new_verified = verified.checked_add(1).ok_or(Error::ArithmeticOverflow)?;
    set_verified_count(env, new_verified);

    extend_instance_ttl(env);

    ApprovedEvent {
        owner: ngo_owner,
        data: (),
    }
    .publish(env);

    Ok(())
}

/// Deletes an NGO's registry entry and its position in the owners index,
/// shrinking the live counts. Shared by `unregister` (owner-initiated),
/// `remove_ngo` (admin-initiated) and `reject_ngo` (reviewer-initiated); the
/// caller is responsible for authorizing the call and for the
/// verified/unverified check. Does not touch the instance TTL â€” the caller
/// decides whether to bump it. Decrements `NgoCount`, and `VerifiedCount` when
/// the entry being removed was verified, using checked subtraction and
/// returning `Error::ArithmeticOverflow` on underflow. Also clears the
/// `NameOwner` uniqueness index entry so the freed name can be re-registered.
fn delete_ngo_entry(env: &Env, owner: &Address) -> Result<(), Error> {
    let count: u64 = env
        .storage()
        .instance()
        .get(&DataKey::NgoCount)
        .unwrap_or(0);
    let new_ngo_count = count.checked_sub(1).ok_or(Error::ArithmeticOverflow)?;

    // Keep `verified_count` and the name index in step with the entry being
    // removed.
    if let Some(ngo) = try_load_ngo(env, owner) {
        unindex_name(env, &ngo.name, owner);
        if ngo.verified {
            let verified = get_verified_count(env);
            let new_verified = verified.checked_sub(1).ok_or(Error::ArithmeticOverflow)?;
            set_verified_count(env, new_verified);
        }
    }
    // Keep `verified_count` in step with the entry being removed, and
    // capture the name before we delete the entry so we can free it below.
    let maybe_name: Option<String> = try_load_ngo(env, owner).map(|ngo| {
        if ngo.verified {
            let verified = get_verified_count(env);
            // Saturating sub rather than propagating an error here:
            // underflow means the counter was already inconsistent,
            // and removing the entry is still the right thing to do.
            set_verified_count(env, verified.saturating_sub(1));
        }
        ngo.name
    });

    env.storage()
        .persistent()
        .remove(&DataKey::Ngo(owner.clone()));

    env.storage()
        .instance()
        .set(&DataKey::NgoCount, &new_ngo_count);

    // Free the name so a future registrant can claim it.
    if let Some(name) = maybe_name {
        let key = canonical_name_key(env, &name);
        env.storage().instance().remove(&DataKey::NameOwner(key));
    }

    // Remove from index using swap-and-pop to maintain a dense [0..count) index
    if let Some(index) = env
        .storage()
        .persistent()
        .get::<_, u32>(&DataKey::IndexByOwner(owner.clone()))
    {
        let count = get_owner_count(env);
        if count > 0 {
            let last_index = count.checked_sub(1).ok_or(Error::ArithmeticOverflow)?;
            if index != last_index {
                if let Some(last_owner) = env
                    .storage()
                    .persistent()
                    .get::<_, Address>(&DataKey::OwnerByIndex(last_index))
                {
                    env.storage()
                        .persistent()
                        .set(&DataKey::OwnerByIndex(index), &last_owner);
                    env.storage()
                        .persistent()
                        .set(&DataKey::IndexByOwner(last_owner.clone()), &index);
                    extend_owner_index_ttl(env, index, &last_owner);
                }
            }
            env.storage()
                .persistent()
                .remove(&DataKey::OwnerByIndex(last_index));
            env.storage()
                .persistent()
                .remove(&DataKey::IndexByOwner(owner.clone()));
            set_owner_count(env, last_index);
        }
    }

    Ok(())
}

#[contract]
pub struct NgoRegistry;

#[contractimpl]
impl NgoRegistry {
    /// Initializes the registry with an administrator atomically at deployment time.
    pub fn __constructor(env: Env, admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        extend_instance_ttl(&env);
        InitializedEvent { admin }.publish(&env);
    }

    /// Sets the registry admin. Can only be called once. Neutralized in favor of
    /// `__constructor` for atomic deployment; requires admin auth.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient};
    /// let env = Env::default();
    /// env.mock_all_auths();
    ///
    /// let admin = Address::generate(&env);
    /// let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// let client = NgoRegistryClient::new(&env, &contract_id);
    /// ```
    pub fn init(env: Env, admin: Address) -> Result<(), Error> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        extend_instance_ttl(&env);
        InitializedEvent { admin }.publish(&env);
        Ok(())
    }

    /// Reads back the registry admin set by `init`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// assert_eq!(client.admin(), admin);
    /// ```
    pub fn admin(env: Env) -> Result<Address, Error> {
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
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
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
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
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
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
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
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
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

    /// Prevents new NGO applications while the switch is on. Admin-gated.
    ///
    /// Once set, every call to `register` fails with `Error::RegistrationsPaused`
    /// until the admin calls `resume_registrations`. All other entry points
    /// (approve, revoke, unregister, etc.) continue to work normally.
    /// Emits a `regrpause` event.
    ///
    /// # Errors
    ///
    /// Returns `Error::NotInitialized` if the registry has not been initialised,
    /// or `Error::RegistrationsPaused` if registrations are already paused.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// client.pause_registrations();
    /// assert!(client.registrations_paused());
    /// let owner = Address::generate(&env);
    /// let name = String::from_str(&env, "Example NGO");
    /// assert!(client.try_register(&owner, &name).is_err());
    /// ```
    pub fn pause_registrations(env: Env) -> Result<(), Error> {
        require_admin(&env)?;
        let already: bool = env
            .storage()
            .instance()
            .get(&DataKey::RegistrationsPaused)
            .unwrap_or(false);
        if already {
            return Err(Error::RegistrationsPaused);
        }
        env.storage()
            .instance()
            .set(&DataKey::RegistrationsPaused, &true);
        extend_instance_ttl(&env);
        RegistrationsPausedEvent { data: () }.publish(&env);
        Ok(())
    }

    /// Re-opens NGO applications after `pause_registrations`. Admin-gated.
    ///
    /// Emits a `regresume` event and clears the registration-paused flag so
    /// `register` works again.
    ///
    /// # Errors
    ///
    /// Returns `Error::NotInitialized` if the registry has not been initialised,
    /// or `Error::NotInitialized` if registrations are not currently paused
    /// (the error is reused to avoid allocating a new code for a transient
    /// state check — see `docs/UPGRADE_POLICY.md`).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// client.pause_registrations();
    /// client.resume_registrations();
    /// assert!(!client.registrations_paused());
    /// let owner = Address::generate(&env);
    /// let name = String::from_str(&env, "Example NGO");
    /// client.register(&owner, &name);
    /// ```
    pub fn resume_registrations(env: Env) -> Result<(), Error> {
        require_admin(&env)?;
        let currently_paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::RegistrationsPaused)
            .unwrap_or(false);
        if !currently_paused {
            // Not paused — nothing to do; treat as a no-op error so callers
            // know the state was already open.
            return Err(Error::NotInitialized);
        }
        env.storage()
            .instance()
            .set(&DataKey::RegistrationsPaused, &false);
        extend_instance_ttl(&env);
        RegistrationsResumedEvent { data: () }.publish(&env);
        Ok(())
    }

    /// Returns `true` while the admin has paused new NGO registrations.
    ///
    /// Read-only; requires no authorization.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// assert!(!client.registrations_paused());
    /// client.pause_registrations();
    /// assert!(client.registrations_paused());
    /// ```
    pub fn registrations_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::RegistrationsPaused)
            .unwrap_or(false)
    }

    /// Submits an NGO application. Callable by the NGO's own address.
    /// The entry starts unverified until an admin approves it. Names must be
    /// at most 200 bytes (`MAX_NGO_NAME_LEN`), valid UTF-8, contain at least one visible
    /// character (not empty or made only of Unicode whitespace or invisible format characters),
    /// and contain no control characters. Names are not trimmed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// let owner = Address::generate(&env);
    /// let name = String::from_str(&env, "Example NGO");
    /// client.register(&owner, &name);
    ///
    /// let ngo = client.get_ngo(&owner);
    /// assert_eq!(ngo.name, name);
    /// assert!(!ngo.verified);
    /// ```
    pub fn register(env: Env, owner: Address, name: String) -> Result<(), Error> {
        owner.require_auth();

        require_registrations_open(&env)?;
        validate_name(&name)?;

        let key = DataKey::Ngo(owner.clone());
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyRegistered);
        }

        // Reject names that are already claimed (after trimming and case-folding).
        let name_key = canonical_name_key(&env, &name);
        if env
            .storage()
            .instance()
            .has(&DataKey::NameOwner(name_key.clone()))
        {
            return Err(Error::NameTaken);
        }

        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NgoCount)
            .unwrap_or(0);
        let new_ngo_count = count.checked_add(1).ok_or(Error::ArithmeticOverflow)?;

        let index = get_owner_count(&env);
        let new_count = index.checked_add(1).ok_or(Error::ArithmeticOverflow)?;

        let ngo = Ngo {
            owner: owner.clone(),
            name: name.clone(),
            verified: false,
            metadata_uri: None,
            verified_until: None,
            categories: 0,
            pending_owner: None,
        };
        save_ngo(&env, &owner, &ngo);

        env.storage()
            .instance()
            .set(&DataKey::NgoCount, &new_ngo_count);

        // Claim the name in the uniqueness index.
        env.storage()
            .instance()
            .set(&DataKey::NameOwner(name_key), &owner);

        // Maintain the registered owners index
        env.storage()
            .persistent()
            .set(&DataKey::OwnerByIndex(index), &owner);
        env.storage()
            .persistent()
            .set(&DataKey::IndexByOwner(owner.clone()), &index);
        set_owner_count(&env, new_count);
        index_name(&env, &name, &owner);

        extend_instance_ttl(&env);
        extend_owner_index_ttl(&env, index, &owner);

        RegisterEvent { owner, name }.publish(&env);

        Ok(())
    }

    /// Deprecated alias of `ngo_count`.
    ///
    /// Reads the same single stored counter (`NgoCount`) as `ngo_count()`.
    /// Preserved for backwards compatibility with existing callers.
    ///
    /// # Conversion
    /// Returns the live count as `u32`. If the stored `u64` count exceeds
    /// `u32::MAX`, saturates at `u32::MAX`. Callers should prefer `ngo_count`.
    pub fn total_ngos(env: Env) -> u32 {
        u32::try_from(Self::ngo_count(env)).unwrap_or(u32::MAX)
    }

    /// Removes the caller's unverified NGO application.
    ///
    /// Verified registrations are intentionally permanent until an admin
    /// revokes verification.
    pub fn unregister(env: Env, owner: Address) -> Result<(), Error> {
        owner.require_auth();

        let ngo = load_ngo(&env, &owner)?;
        if ngo.verified {
            return Err(Error::AlreadyVerified);
        }

        delete_ngo_entry(&env, &owner)?;

        extend_instance_ttl(&env);
        UnregisteredEvent { owner, data: () }.publish(&env);

        Ok(())
    }

    /// Removes an unverified NGO entry entirely. Admin-only, so an admin can
    /// clear out an impersonating or abusive application without waiting for
    /// its owner to `unregister`. Returns `Error::NotRegistered` for an
    /// address with no entry and `Error::AlreadyVerified` for a verified one:
    /// an approval is undone with `revoke_ngo` first, so removal can never
    /// silently drop a verification an admin granted.
    ///
    /// The entry, its position in the owners index, and the live counts
    /// (`TotalNgos`/`OwnerIndexCount`) are all cleared, so the owner can
    /// register again. The lifetime `NgoCount` is left alone, matching
    /// `unregister`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// let spammer = Address::generate(&env);
    /// client.register(&spammer, &String::from_str(&env, "Totally Real Charity"));
    ///
    /// // The admin removes the junk application outright.
    /// client.remove_ngo(&spammer);
    /// assert!(client.try_get_ngo(&spammer).is_err());
    /// ```
    pub fn remove_ngo(env: Env, ngo_owner: Address) -> Result<(), Error> {
        require_admin(&env)?;

        let ngo = load_ngo(&env, &ngo_owner)?;
        if ngo.verified {
            return Err(Error::AlreadyVerified);
        }

        delete_ngo_entry(&env, &ngo_owner)?;

        extend_instance_ttl(&env);
        RemovedEvent {
            owner: ngo_owner,
            data: (),
        }
        .publish(&env);

        Ok(())
    }

    /// Rejects a pending NGO application, removing its entry and publishing the
    /// supplied `reason_code` so the applicant's client can explain why it was
    /// not approved. Callable by the admin or any listed approver (see
    /// `add_approver`), and the counterpart to `approve_ngo` for a reviewer who
    /// decides against an application rather than leaving it pending.
    ///
    /// The entry, its position in the owners index, and the live counts are
    /// cleared exactly as in `remove_ngo`, so the owner can address the problem
    /// and register again. The `reason_code` is an opaque, caller-defined
    /// integer (for example, an off-chain enum agreed between the registry and
    /// its clients); the contract keeps no record of it beyond the event.
    ///
    /// Returns `Error::NotRegistered` for an address with no entry. A verified
    /// NGO cannot be rejected â€” an approval is undone with `revoke_ngo` first â€”
    /// and fails with `Error::AlreadyVerified`. An address that is neither the
    /// admin nor a listed approver fails with `Error::NotApprover`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// let applicant = Address::generate(&env);
    /// client.register(&applicant, &String::from_str(&env, "Unverified Charity"));
    ///
    /// // 3 = "could not verify registration documents".
    /// client.reject_ngo(&admin, &applicant, &3);
    /// assert!(client.try_get_ngo(&applicant).is_err());
    /// ```
    pub fn reject_ngo(
        env: Env,
        approver: Address,
        ngo_owner: Address,
        reason_code: u32,
    ) -> Result<(), Error> {
        require_approver(&env, &approver)?;

        let ngo = load_ngo(&env, &ngo_owner)?;
        if ngo.verified {
            return Err(Error::AlreadyVerified);
        }

        delete_ngo_entry(&env, &ngo_owner)?;

        extend_instance_ttl(&env);
        RejectedEvent {
            owner: ngo_owner,
            reason_code,
        }
        .publish(&env);

        Ok(())
    }

    /// Adds an address to the approver list. Admin-only.
    ///
    /// Approvers may call `approve_ngo`, `batch_approve`, `reject_ngo`, and
    /// `revoke_ngo`
    /// without holding the admin key, which is how review work is delegated
    /// without granting control over upgrades or admin transfer. The admin is
    /// always able to review whether or not it appears in this list, so adding
    /// the admin is allowed but unnecessary. Fails with
    /// `Error::AlreadyApprover` if the address is already listed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// let reviewer = Address::generate(&env);
    /// client.add_approver(&reviewer);
    /// assert!(client.is_approver(&reviewer));
    /// ```
    pub fn add_approver(env: Env, approver: Address) -> Result<(), Error> {
        require_admin(&env)?;

        let mut approvers = read_approvers(&env);
        if approvers.contains(&approver) {
            return Err(Error::AlreadyApprover);
        }
        approvers.push_back(approver.clone());
        env.storage()
            .instance()
            .set(&DataKey::Approvers, &approvers);
        extend_instance_ttl(&env);

        ApproverAddedEvent { approver, data: () }.publish(&env);

        Ok(())
    }

    /// Removes an address from the approver list. Admin-only.
    ///
    /// The address can no longer approve or revoke NGOs; any NGO it approved
    /// stays approved. Fails with `Error::NotApprover` if the address is not
    /// currently listed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// let reviewer = Address::generate(&env);
    /// client.add_approver(&reviewer);
    /// client.remove_approver(&reviewer);
    /// assert!(!client.is_approver(&reviewer));
    /// ```
    pub fn remove_approver(env: Env, approver: Address) -> Result<(), Error> {
        require_admin(&env)?;

        let approvers = read_approvers(&env);
        let mut remaining = Vec::new(&env);
        let mut found = false;
        for listed in approvers.iter() {
            if listed == approver {
                found = true;
            } else {
                remaining.push_back(listed);
            }
        }
        if !found {
            return Err(Error::NotApprover);
        }
        env.storage()
            .instance()
            .set(&DataKey::Approvers, &remaining);
        extend_instance_ttl(&env);

        ApproverRemovedEvent { approver, data: () }.publish(&env);

        Ok(())
    }

    /// Returns whether an address is in the approver list.
    ///
    /// Reflects explicit approvers only: the admin can always approve and
    /// revoke NGOs, so this returns `false` for the admin unless it was also
    /// added with `add_approver`. Never errors or panics.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// let reviewer = Address::generate(&env);
    /// assert!(!client.is_approver(&reviewer));
    /// client.add_approver(&reviewer);
    /// assert!(client.is_approver(&reviewer));
    /// ```
    pub fn is_approver(env: Env, addr: Address) -> bool {
        is_approver_address(&env, &addr)
    }

    /// Returns every address in the approver list, in the order they were
    /// added. Returns an empty list when none are configured. Never errors or
    /// panics.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// let reviewer = Address::generate(&env);
    /// client.add_approver(&reviewer);
    /// assert_eq!(client.list_approvers().len(), 1);
    /// ```
    pub fn list_approvers(env: Env) -> Vec<Address> {
        read_approvers(&env)
    }

    /// Returns a paginated list of registered NGO owner addresses.
    ///
    /// # Pagination Semantics
    /// - `start`: The 0-based offset into the registered owners index. If `start` is
    ///   greater than or equal to the total number of registered owners, an empty list is returned.
    /// - `limit`: The maximum number of entries to return. Capped at `MAX_LIST_LIMIT` (50).
    ///   If `limit == 0`, an empty list is returned.
    /// - Ordering: Stable and deterministic. Entries are indexed in order of registration,
    ///   with unregistration swapping the last entry into the vacated position (swap-and-pop)
    ///   to keep persistent storage dense and bounded without reallocations.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// let owner = Address::generate(&env);
    /// client.register(&owner, &String::from_str(&env, "Example NGO"));
    ///
    /// let owners = client.list_ngos(&0, &10);
    /// assert_eq!(owners.len(), 1);
    /// assert_eq!(owners.get(0).unwrap(), owner);
    /// ```
    pub fn list_ngos(env: Env, start: u32, limit: u32) -> Vec<Address> {
        let count = get_owner_count(&env);
        if start >= count || limit == 0 {
            return Vec::new(&env);
        }

        let effective_limit = limit.min(MAX_LIST_LIMIT);
        let end = start
            .checked_add(effective_limit)
            .unwrap_or(count)
            .min(count);

        let mut owners = Vec::new(&env);
        for index in start..end {
            if let Some(owner) = env
                .storage()
                .persistent()
                .get::<_, Address>(&DataKey::OwnerByIndex(index))
            {
                owners.push_back(owner);
            }
        }
        owners
    }

    /// Returns one page of registered NGO owner addresses together with the
    /// index to use for the following page.
    ///
    /// Same `start`/`limit` semantics, ordering, and `MAX_LIST_LIMIT` cap as
    /// `list_ngos`, but instead of leaving callers to recompute the cursor it
    /// also returns `next`, the `start` for the next call. Because `next`
    /// reaches the total owner count on the final page, a caller can tell the
    /// end of the list apart from a full page without a follow-up call.
    ///
    /// `next` is the exclusive end of this page (one past its last owner),
    /// clamped to the owner count. Iteration is complete once
    /// `next >= total_ngos()`. An out-of-range `start` or a `limit == 0`
    /// returns an empty `owners` and `next` equal to the owner count, so the
    /// caller stops immediately.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// let owner = Address::generate(&env);
    /// client.register(&owner, &String::from_str(&env, "Example NGO"));
    ///
    /// let page = client.list_ngos_page(&0, &10);
    /// assert_eq!(page.owners.get(0).unwrap(), owner);
    /// assert_eq!(page.next, 1);
    /// ```
    pub fn list_ngos_page(env: Env, start: u32, limit: u32) -> NgoOwnerPage {
        let count = get_owner_count(&env);
        if start >= count || limit == 0 {
            return NgoOwnerPage {
                owners: Vec::new(&env),
                next: count,
            };
        }

        let effective_limit = limit.min(MAX_LIST_LIMIT);
        let end = start
            .checked_add(effective_limit)
            .unwrap_or(count)
            .min(count);

        let mut owners = Vec::new(&env);
        for index in start..end {
            if let Some(owner) = env
                .storage()
                .persistent()
                .get::<_, Address>(&DataKey::OwnerByIndex(index))
            {
                owners.push_back(owner);
            }
        }

        NgoOwnerPage { owners, next: end }
    }

    /// Changes the name on an NGO's own pending application, so a typo or
    /// rename can be fixed without going through an admin. Requires the
    /// owner's auth. Fails with `Error::NotRegistered` for an address with
    /// no entry, and `Error::AlreadyVerified` once an admin has approved
    /// it â€” the name an admin approved is the name that stays.
    ///
    /// The new name must meet the same validation rules as in `register`: at most
    /// 200 bytes, valid UTF-8, at least one visible character, and no control characters.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let owner = Address::generate(&env);
    /// client.register(&owner, &String::from_str(&env, "Exmaple NGO"));
    ///
    /// let fixed = String::from_str(&env, "Example NGO");
    /// client.update_name(&owner, &fixed);
    /// assert_eq!(client.get_ngo(&owner).name, fixed);
    /// ```
    pub fn update_name(env: Env, owner: Address, name: String) -> Result<(), Error> {
        owner.require_auth();

        validate_name(&name)?;

        let mut ngo = load_ngo(&env, &owner)?;
        if ngo.verified {
            return Err(Error::AlreadyVerified);
        }

        // Only update and emit event if the name actually changes
        if ngo.name != name {
            unindex_name(&env, &ngo.name, &owner);
            // Check the new name is not already claimed by someone else.
            let new_name_key = canonical_name_key(&env, &name);
            if env
                .storage()
                .instance()
                .has(&DataKey::NameOwner(new_name_key.clone()))
            {
                return Err(Error::NameTaken);
            }

            // Release the old name so another registrant can claim it.
            let old_name_key = canonical_name_key(&env, &ngo.name);
            env.storage()
                .instance()
                .remove(&DataKey::NameOwner(old_name_key));

            // Claim the new name.
            env.storage()
                .instance()
                .set(&DataKey::NameOwner(new_name_key), &owner);

            ngo.name = name.clone();
            save_ngo(&env, &owner, &ngo);
            index_name(&env, &name, &owner);
            extend_instance_ttl(&env);
            RenamedEvent { owner, name }.publish(&env);
        }

        Ok(())
    }

    /// Sets or clears the metadata URI on the caller's own NGO entry, so an
    /// entry can point at off-chain metadata such as a website, description,
    /// or logo. Requires the owner's auth; the entry must already be
    /// registered, or the call fails with `Error::NotRegistered`.
    ///
    /// The URI is opaque to the contract â€” any scheme (`https://`, `ipfs://`,
    /// â€¦) is accepted â€” and must be at most `MAX_METADATA_URI_LEN` (512 bytes),
    /// or the call fails with `Error::MetadataUriTooLong`. Pass `None` to clear
    /// it. Unlike `update_name`, this is not locked by verification: an NGO can
    /// keep its published metadata current after approval. Publishes a
    /// `MetadataUriSetEvent`, but only when the value actually changes. See
    /// issue #347.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let owner = Address::generate(&env);
    /// # client.register(&owner, &String::from_str(&env, "Example NGO"));
    /// let uri = Some(String::from_str(&env, "https://example.org/about"));
    /// client.set_metadata_uri(&owner, &uri);
    /// assert_eq!(client.get_ngo(&owner).metadata_uri, uri);
    /// ```
    pub fn set_metadata_uri(
        env: Env,
        owner: Address,
        metadata_uri: Option<String>,
    ) -> Result<(), Error> {
        owner.require_auth();

        validate_metadata_uri(&metadata_uri)?;

        let mut ngo = load_ngo(&env, &owner)?;

        // Only write and emit when the URI actually changes, matching
        // `update_name`.
        if ngo.metadata_uri != metadata_uri {
            ngo.metadata_uri = metadata_uri.clone();
            save_ngo(&env, &owner, &ngo);
            extend_instance_ttl(&env);

            MetadataUriSetEvent {
                owner,
                metadata_uri,
            }
            .publish(&env);
        }

        Ok(())
    }

    /// Looks up the owner of the NGO registered under `name`, without listing
    /// the registry. Read-only and unauthenticated.
    ///
    /// Returns `None` when no registered NGO holds the name. The match is on
    /// the exact name string (case and whitespace included). The lookup is
    /// backed by an index that `register`, `update_name`, `unregister`,
    /// `remove_ngo` and `reject_ngo` keep in step, so a renamed NGO is found
    /// under its new name only, and a name is free again once its NGO leaves.
    /// If two NGOs register the same name (names are not unique yet), the
    /// first one keeps the name and the other is not indexed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let owner = Address::generate(&env);
    /// let name = String::from_str(&env, "Example NGO");
    /// client.register(&owner, &name);
    /// assert_eq!(client.owner_by_name(&name), Some(owner));
    /// ```
    pub fn owner_by_name(env: Env, name: String) -> Option<Address> {
        env.storage().persistent().get(&name_key(&env, &name))
    }

    /// Reads back an NGO's metadata URI, or `None` when it has none set.
    ///
    /// Never errors or panics: an unregistered address, and a registered
    /// entry that hasn't called `set_metadata_uri`, both return `None`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let owner = Address::generate(&env);
    /// # client.register(&owner, &String::from_str(&env, "Example NGO"));
    /// assert_eq!(client.metadata_uri(&owner), None);
    ///
    /// let uri = Some(String::from_str(&env, "https://example.org"));
    /// client.set_metadata_uri(&owner, &uri);
    /// assert_eq!(client.metadata_uri(&owner), uri);
    /// ```
    pub fn metadata_uri(env: Env, owner: Address) -> Option<String> {
        try_load_ngo(&env, &owner).and_then(|ngo| ngo.metadata_uri)
    }

    /// Sets the category bitmask for the caller's own NGO entry. The bitmask
    /// is the bitwise OR of any `CATEGORY_*` constants that describe what the
    /// NGO does. Pass `0` to clear all categories.
    ///
    /// Categories are **owner-settable before verification** and are locked
    /// once the entry is approved (`Error::AlreadyVerified`). To change them
    /// after approval the owner must ask an admin to revoke verification first.
    ///
    /// Any bit outside `VALID_CATEGORY_MASK` (bits 8–31) is reserved and
    /// causes `Error::InvalidCategories`. Only emits `CategoriesSetEvent` when
    /// the value actually changes.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient, CATEGORY_HEALTH, CATEGORY_EDUCATION};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// # let owner = Address::generate(&env);
    /// # client.register(&owner, &String::from_str(&env, "Example NGO"));
    /// client.set_categories(&owner, &(CATEGORY_HEALTH | CATEGORY_EDUCATION));
    /// assert_eq!(client.categories(&owner), CATEGORY_HEALTH | CATEGORY_EDUCATION);
    /// ```
    pub fn set_categories(env: Env, owner: Address, categories: u32) -> Result<(), Error> {
        owner.require_auth();

        if categories & !VALID_CATEGORY_MASK != 0 {
            return Err(Error::InvalidCategories);
        }

        let mut ngo = load_ngo(&env, &owner)?;

        if ngo.verified {
            return Err(Error::AlreadyVerified);
        }

        if ngo.categories != categories {
            ngo.categories = categories;
            save_ngo(&env, &owner, &ngo);
            extend_instance_ttl(&env);

            CategoriesSetEvent { owner, categories }.publish(&env);
        }

        Ok(())
    }

    /// Reads back the category bitmask for an NGO. Returns `0` for an
    /// unregistered address or one that has not called `set_categories`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient, CATEGORY_ENVIRONMENT};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// # let owner = Address::generate(&env);
    /// # client.register(&owner, &String::from_str(&env, "Example NGO"));
    /// assert_eq!(client.categories(&owner), 0);
    ///
    /// client.set_categories(&owner, &CATEGORY_ENVIRONMENT);
    /// assert_eq!(client.categories(&owner), CATEGORY_ENVIRONMENT);
    /// ```
    pub fn categories(env: Env, owner: Address) -> u32 {
        try_load_ngo(&env, &owner)
            .map(|ngo| ngo.categories)
            .unwrap_or(0)
    }

    /// Reads back an NGO's registry entry, registered or not.
    ///
    /// A verification whose `verified_until` has passed is reported as
    /// unverified (`verified: false`), so callers never act on a lapsed
    /// approval; the stored flag and the past timestamp are left in place and
    /// can be reactivated with `renew_ngo`. See issue #348.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let owner = Address::generate(&env);
    /// # let name = String::from_str(&env, "Example NGO");
    /// # client.register(&owner, &name);
    /// let ngo = client.get_ngo(&owner);
    /// assert_eq!(ngo.owner, owner);
    /// assert!(!ngo.verified);
    /// ```
    pub fn get_ngo(env: Env, owner: Address) -> Result<Ngo, Error> {
        let mut ngo = load_ngo(&env, &owner)?;
        if ngo.verified && has_expired(&env, ngo.verified_until) {
            ngo.verified = false;
        }
        Ok(ngo)
    }

    /// Returns whether an address is a currently verified, registered NGO.
    /// Unknown and unverified addresses both return `false`, as does an
    /// address whose verification has lapsed (`verified_until` passed).
    /// Never errors or panics. See issue #348.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// let owner = Address::generate(&env);
    /// let name = String::from_str(&env, "Example NGO");
    /// client.register(&owner, &name);
    /// assert!(!client.is_verified(&owner));
    /// client.approve_ngo(&admin, &owner);
    /// assert!(client.is_verified(&owner));
    /// ```
    pub fn is_verified(env: Env, owner: Address) -> bool {
        Self::get_ngo(env, owner)
            .map(|ngo| ngo.verified)
            .unwrap_or(false)
    }

    /// Returns the detailed verification state of `owner`, distinguishing the
    /// five cases that `is_verified` collapses into a single `false`:
    /// `Unknown` (no entry), `Pending` (registered, not yet reviewed),
    /// `Verified` (approved and not expired), `Expired` (approved but
    /// `verified_until` has passed), and `Revoked` (approval was explicitly
    /// revoked). Never errors or panics. See issue #448.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient, VerificationStatus};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// let owner = Address::generate(&env);
    /// assert_eq!(client.verification_status(&owner), VerificationStatus::Unknown);
    ///
    /// client.register(&owner, &String::from_str(&env, "Example NGO"));
    /// assert_eq!(client.verification_status(&owner), VerificationStatus::Pending);
    ///
    /// client.approve_ngo(&admin, &owner);
    /// assert_eq!(client.verification_status(&owner), VerificationStatus::Verified);
    /// ```
    pub fn verification_status(env: Env, owner: Address) -> VerificationStatus {
        let ngo: Ngo = match try_load_ngo(&env, &owner) {
            Some(n) => n,
            None => return VerificationStatus::Unknown,
        };

        if ngo.verified {
            if has_expired(&env, ngo.verified_until) {
                return VerificationStatus::Expired;
            }
            return VerificationStatus::Verified;
        }

        // verified == false: distinguish Revoked from Pending via the flag
        // set by revoke_ngo and cleared by approve_registered_ngo.
        let revoked: bool = env
            .storage()
            .persistent()
            .get(&DataKey::NgoRevoked(owner))
            .unwrap_or(false);
        if revoked {
            VerificationStatus::Revoked
        } else {
            VerificationStatus::Pending
        }
    }

    /// Reads back the total number of registered NGOs.
    ///
    /// Lets callers (such as the impact page) display the total count
    /// of registered NGOs without querying a backend indexer.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// assert_eq!(client.ngo_count(), 0);
    ///
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// let owner = Address::generate(&env);
    /// let name = String::from_str(&env, "Example NGO");
    /// client.register(&owner, &name);
    /// assert_eq!(client.ngo_count(), 1);
    /// ```
    pub fn ngo_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::NgoCount)
            .unwrap_or(0)
    }

    /// Returns how many registered NGO entries are currently verified.
    ///
    /// Backed by a single stored counter, kept in step by `approve_ngo` /
    /// `batch_approve` (increment), `revoke_ngo` (decrement), and removal of a
    /// verified entry (decrement), so a dashboard can read the verified tally
    /// without listing and filtering every entry.
    ///
    /// The counter tracks the stored verification flag. Time never clears that
    /// flag, so an entry whose `verified_until` has lapsed still counts here
    /// until an admin revokes or renews it, even though `is_verified` already
    /// reports it as unverified. See issue #348.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// let owner = Address::generate(&env);
    /// client.register(&owner, &String::from_str(&env, "Example NGO"));
    /// assert_eq!(client.verified_count(), 0);
    ///
    /// client.approve_ngo(&admin, &owner);
    /// assert_eq!(client.verified_count(), 1);
    /// ```
    pub fn verified_count(env: Env) -> u64 {
        get_verified_count(&env)
    }

    /// Sets the verification period, in seconds, that `approve_ngo` /
    /// `batch_approve` and `renew_ngo` add to the current ledger time to stamp
    /// an entry's `verified_until`. Admin-only.
    ///
    /// A period of `0` (the default) means approvals never expire:
    /// `verified_until` stays `None`. Changing the period only affects future
    /// approvals and renewals; entries already approved keep their existing
    /// expiry until they are renewed. Fails with `Error::NotInitialized`
    /// before `init`. Publishes a `VerifyPeriodSetEvent`. See issue #348.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// assert_eq!(client.verify_period(), 0);
    ///
    /// // Verifications last one year.
    /// client.set_verify_period(&(365 * 24 * 60 * 60));
    /// assert_eq!(client.verify_period(), 365 * 24 * 60 * 60);
    /// ```
    pub fn set_verify_period(env: Env, seconds: u64) -> Result<(), Error> {
        require_admin(&env)?;

        env.storage()
            .instance()
            .set(&DataKey::VerifyPeriod, &seconds);
        extend_instance_ttl(&env);

        VerifyPeriodSetEvent {
            verify_period: seconds,
        }
        .publish(&env);

        Ok(())
    }

    /// Reads back the configured verification period in seconds. `0` means
    /// approvals never expire. Never errors or panics. See `set_verify_period`
    /// and issue #348.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// assert_eq!(client.verify_period(), 0);
    /// ```
    pub fn verify_period(env: Env) -> u64 {
        get_verify_period(&env)
    }

    /// Marks a registered NGO as verified. Callable by the admin or a listed
    /// approver; `approver` names the acting address and must have authorized
    /// the call (see `add_approver`). Fails with
    /// `Error::AlreadyVerified` if the NGO is already verified, so a
    /// repeated call can't rewrite the entry or publish a duplicate
    /// `approved` event, and `Error::NotApprover` if `approver` is neither the
    /// admin nor a listed approver.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let owner = Address::generate(&env);
    /// # let name = String::from_str(&env, "Example NGO");
    /// # client.register(&owner, &name);
    /// client.approve_ngo(&admin, &owner);
    /// assert!(client.get_ngo(&owner).verified);
    /// ```
    pub fn approve_ngo(env: Env, approver: Address, ngo_owner: Address) -> Result<(), Error> {
        require_approver(&env, &approver)?;
        approve_registered_ngo(&env, ngo_owner)
    }

    /// Marks several registered NGOs as verified in a single call. Callable by
    /// the admin or a listed approver; `approver` names the acting address and
    /// must have authorized the call.
    ///
    /// Approves each owner in order, publishing one `approved` event per NGO
    /// exactly as `approve_ngo` would. The call is atomic: if any owner has no
    /// entry (`Error::NotRegistered`) or is already verified
    /// (`Error::AlreadyVerified`, which also covers an owner repeated within
    /// the same batch) nothing is approved and no event is published. An empty
    /// batch succeeds and emits nothing.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, vec, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// let a = Address::generate(&env);
    /// let b = Address::generate(&env);
    /// client.register(&a, &String::from_str(&env, "Red Cross"));
    /// client.register(&b, &String::from_str(&env, "Oxfam"));
    ///
    /// client.batch_approve(&admin, &vec![&env, a, b]);
    /// ```
    pub fn batch_approve(env: Env, approver: Address, owners: Vec<Address>) -> Result<(), Error> {
        require_approver(&env, &approver)?;
        for ngo_owner in owners.iter() {
            approve_registered_ngo(&env, ngo_owner)?;
        }
        Ok(())
    }

    /// Reverses a prior approval, marking a registered NGO as unverified
    /// again. Callable by the admin or a listed approver; `approver` names the
    /// acting address and must have authorized the call. Returns
    /// `Error::NotRegistered` for an address with no entry, matching
    /// `approve_ngo`'s existing behavior, `Error::NotVerified` if the NGO
    /// isn't currently verified, so a repeated call can't publish a duplicate
    /// `revoked` event, and `Error::NotApprover` if `approver` is neither the
    /// admin nor a listed approver.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let owner = Address::generate(&env);
    /// # let name = String::from_str(&env, "Example NGO");
    /// # client.register(&owner, &name);
    /// # client.approve_ngo(&admin, &owner);
    /// client.revoke_ngo(&admin, &owner);
    /// assert!(!client.get_ngo(&owner).verified);
    /// ```
    pub fn revoke_ngo(env: Env, approver: Address, ngo_owner: Address) -> Result<(), Error> {
        require_approver(&env, &approver)?;

        let mut ngo = load_ngo(&env, &ngo_owner)?;
        if !ngo.verified {
            return Err(Error::NotVerified);
        }
        ngo.verified = false;
        // A revoked verification has no expiry to keep; clear it so the entry
        // doesn't carry a stale timestamp (#348).
        ngo.verified_until = None;
        save_ngo(&env, &ngo_owner, &ngo);
        // Mark as revoked so `verification_status` can distinguish this from
        // a pending (never-approved) entry. See issue #448.
        env.storage()
            .persistent()
            .set(&DataKey::NgoRevoked(ngo_owner.clone()), &true);

        let verified = get_verified_count(&env);
        let new_verified = verified.checked_sub(1).ok_or(Error::ArithmeticOverflow)?;
        set_verified_count(&env, new_verified);

        extend_instance_ttl(&env);

        RevokedEvent {
            owner: ngo_owner,
            data: (),
        }
        .publish(&env);

        Ok(())
    }

    /// Extends a lapsed â€” or still-valid â€” verification by the configured
    /// period, moving its `verified_until` forward from the current ledger
    /// time. Admin-only.
    ///
    /// This is the periodic-renewal counterpart to `approve_ngo`: an entry
    /// whose verification has lapsed needs an admin to renew it, because
    /// nothing on-chain flips the stored flag back on its own. The entry must
    /// be registered (`Error::NotRegistered`) and its stored verification flag
    /// must be set (`Error::NotVerified`); the flag stays set after a lapse, so
    /// an expired entry can be renewed, while one that was never approved or
    /// was revoked must be approved first. Renewal requires a non-zero
    /// configured period, or it fails with `Error::NoVerifyPeriod`. Fails with
    /// `Error::NotInitialized` before `init`. Publishes a `RenewedEvent` with
    /// the new expiry. See issue #348.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryArgs, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let admin = Address::generate(&env);
    /// # let contract_id = env.register(NgoRegistry, NgoRegistryArgs::__constructor(&admin));
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// client.set_verify_period(&(365 * 24 * 60 * 60));
    /// let owner = Address::generate(&env);
    /// client.register(&owner, &String::from_str(&env, "Example NGO"));
    /// client.approve_ngo(&admin, &owner);
    ///
    /// // A year later the verification has lapsed, and renewing it revives it.
    /// client.renew_ngo(&owner);
    /// assert!(client.is_verified(&owner));
    /// ```
    pub fn renew_ngo(env: Env, ngo_owner: Address) -> Result<(), Error> {
        require_admin(&env)?;

        let period = get_verify_period(&env);
        if period == 0 {
            return Err(Error::NoVerifyPeriod);
        }

        let mut ngo = load_ngo(&env, &ngo_owner)?;
        if !ngo.verified {
            return Err(Error::NotVerified);
        }

        let verified_until = env
            .ledger()
            .timestamp()
            .checked_add(period)
            .ok_or(Error::ArithmeticOverflow)?;
        ngo.verified_until = Some(verified_until);
        save_ngo(&env, &ngo_owner, &ngo);

        extend_instance_ttl(&env);

        RenewedEvent {
            owner: ngo_owner,
            verified_until,
        }
        .publish(&env);

        Ok(())
    }

    /// Bumps a registered NGO entry's storage TTL without changing
    /// anything about it. Callable by anyone â€” a verified NGO that
    /// `register`, `approve_ngo`, and `revoke_ngo` haven't touched in a
    /// while would otherwise have its entry archived after 90 days, with
    /// no other way to keep it alive.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, String};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
    /// # let admin = Address::generate(&env);
    /// # client.init(&admin);
    /// # let owner = Address::generate(&env);
    /// # let name = String::from_str(&env, "Example NGO");
    /// # client.register(&owner, &name);
    /// client.touch_ngo(&owner);
    /// ```
    pub fn touch_ngo(env: Env, owner: Address) -> Result<(), Error> {
        let ngo = load_ngo(&env, &owner)?;
        extend_instance_ttl(&env);
        if env
            .storage()
            .persistent()
            .get::<_, Address>(&name_key(&env, &ngo.name))
            .as_ref()
            == Some(&owner)
        {
            extend_name_index_ttl(&env, &ngo.name);
        }
        if let Some(index) = env
            .storage()
            .persistent()
            .get::<_, u32>(&DataKey::IndexByOwner(owner.clone()))
        {
            extend_owner_index_ttl(&env, index, &owner);
        }
        Ok(())
    }

    /// Replaces the contract's Wasm bytecode in place. Admin-only.
    /// Lets a bug fix be deployed without changing the contract address,
    /// preserving every existing NGO entry and the admin key.
    ///
    /// Only allowed while the upgrade timelock is explicitly disabled (the
    /// configured `upgrade_delay` is zero). With a non-zero delay, upgrades
    /// must go through `propose_upgrade` / `execute_upgrade` to provide a
    /// warning period; this returns `Error::UpgradeTimelockEnabled` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};
    /// # use ngo_registry::{NgoRegistry, NgoRegistryClient};
    /// # let env = Env::default();
    /// # env.mock_all_auths();
    /// # let contract_id = env.register(NgoRegistry, ());
    /// # let client = NgoRegistryClient::new(&env, &contract_id);
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
        extend_instance_ttl(&env);
        UpgradedEvent { new_wasm_hash }.publish(&env);
        Ok(())
    }

    /// Reads the configured upgrade delay in seconds. `0` means the timelock
    /// is disabled and the immediate `upgrade` entry point is allowed.
    pub fn upgrade_delay(env: Env) -> u64 {
        get_upgrade_delay(&env)
    }

    /// Reads the upgrade proposed by `propose_upgrade`, if one is awaiting its
    /// timelock. `None` when nothing is pending.
    pub fn pending_upgrade(env: Env) -> Option<PendingUpgrade> {
        env.storage().instance().get(&DataKey::PendingUpgrade)
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
    /// contract's Wasm bytecode in place. Admin-gated; emits an `upgrade`
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

    /// Proposes a new owner for the NGO entry. The current owner initiates the
    /// transfer, and the new owner must accept it via `accept_owner_transfer`.
    /// The verified status is preserved through the transfer. Owner-gated; emits
    /// an `ownprop` event.
    pub fn propose_owner_transfer(
        env: Env,
        current_owner: Address,
        new_owner: Address,
    ) -> Result<(), Error> {
        current_owner.require_auth();

        if current_owner == new_owner {
            return Err(Error::InvalidAdmin); // Reusing error for invalid transfer
        }

        let mut ngo = load_ngo(&env, &current_owner)?;

        ngo.pending_owner = Some(new_owner.clone());
        save_ngo(&env, &current_owner, &ngo);

        extend_instance_ttl(&env);

        OwnershipProposedEvent {
            current_owner,
            new_owner,
        }
        .publish(&env);

        Ok(())
    }

    /// Accepts a proposed ownership transfer. The new owner (pending_owner) must
    /// call this to complete the transfer. The NGO entry is moved to be keyed by
    /// the new owner's address, and the verified status is preserved. Emits an
    /// `ownaccpt` event.
    pub fn accept_owner_transfer(
        env: Env,
        old_owner: Address,
        new_owner: Address,
    ) -> Result<(), Error> {
        new_owner.require_auth();

        let old_key = DataKey::Ngo(old_owner.clone());
        let ngo = load_ngo(&env, &old_owner)?;

        // Verify the new_owner matches the pending_owner
        if ngo.pending_owner != Some(new_owner.clone()) {
            return Err(Error::NoPendingAdmin); // Reusing error for no pending transfer
        }

        // Check if new_owner already has an NGO registered
        if try_load_ngo(&env, &new_owner).is_some() {
            return Err(Error::AlreadyRegistered);
        }

        // Create the new entry with updated owner and no pending_owner
        let updated_ngo = Ngo {
            owner: new_owner.clone(),
            name: ngo.name.clone(),
            verified: ngo.verified,
            metadata_uri: ngo.metadata_uri.clone(),
            verified_until: ngo.verified_until,
            categories: ngo.categories,
            pending_owner: None,
        };

        // Remove the old entry
        env.storage().persistent().remove(&old_key);

        // Update the owner index
        if let Some(index) = env
            .storage()
            .persistent()
            .get::<_, u32>(&DataKey::IndexByOwner(old_owner.clone()))
        {
            env.storage()
                .persistent()
                .set(&DataKey::OwnerByIndex(index), &new_owner);
            env.storage()
                .persistent()
                .remove(&DataKey::IndexByOwner(old_owner.clone()));
            env.storage()
                .persistent()
                .set(&DataKey::IndexByOwner(new_owner.clone()), &index);
            extend_owner_index_ttl(&env, index, &new_owner);
        }

        // Store the new entry
        save_ngo(&env, &new_owner, &updated_ngo);

        extend_instance_ttl(&env);

        // Update vault expectations: the vault will need to handle this
        // by allowing withdrawals to either the old or new owner during a grace period
        // or by having the NGO update their payout address

        OwnershipAcceptedEvent {
            old_owner,
            new_owner,
        }
        .publish(&env);

        Ok(())
    }

    /// Cancels a pending ownership transfer. The current owner can call this to
    /// withdraw the proposal. Emits an `owncancel` event.
    pub fn cancel_owner_transfer(env: Env, owner: Address) -> Result<(), Error> {
        owner.require_auth();

        let mut ngo = load_ngo(&env, &owner)?;

        if ngo.pending_owner.is_none() {
            return Err(Error::NoPendingAdmin); // Reusing error for no pending transfer
        }

        ngo.pending_owner = None;
        save_ngo(&env, &owner, &ngo);

        extend_instance_ttl(&env);

        OwnershipCancelledEvent { owner, data: () }.publish(&env);

        Ok(())
    }
}

#[cfg(test)]
#[cfg(test)]
mod test;
