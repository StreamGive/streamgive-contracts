use soroban_sdk::{contractevent, Address, Bytes, BytesN};

#[contractevent(topics = ["propadmin"], data_format = "single-value")]
pub struct ProposedAdminEvent {
    pub admin: Address,
}

#[contractevent(topics = ["acptadmin"], data_format = "single-value")]
pub struct AcceptedAdminEvent {
    pub admin: Address,
}

#[contractevent(topics = ["canceladm"], data_format = "single-value")]
pub struct CancelledAdminEvent {
    pub data: (),
}

#[contractevent(topics = ["renounce"], data_format = "single-value")]
pub struct RenouncedAdminEvent {
    pub data: (),
}

#[contractevent(topics = ["pause"], data_format = "single-value")]
pub struct PausedEvent {
    pub data: (),
}

#[contractevent(topics = ["unpause"], data_format = "single-value")]
pub struct UnpausedEvent {
    pub data: (),
}

#[contractevent(topics = ["created"], data_format = "vec")]
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

#[contractevent(topics = ["withdraw"], data_format = "vec")]
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
pub struct DrainedEvent {
    #[topic]
    pub stream_id: u64,
    /// The stream's `withdrawn` total at the moment it emptied.
    pub total_paid: i128,
}

#[contractevent(topics = ["cancel"], data_format = "vec")]
pub struct CancelledStreamEvent {
    #[topic]
    pub stream_id: u64,
    pub accrued: i128,
    pub refund: i128,
}

#[contractevent(topics = ["topup"], data_format = "vec")]
pub struct ToppedUpEvent {
    #[topic]
    pub stream_id: u64,
    pub amount: i128,
    pub settled: i128,
}

#[contractevent(topics = ["ratemod"], data_format = "vec")]
pub struct RateModifiedEvent {
    #[topic]
    pub stream_id: u64,
    pub old_rate: i128,
    pub new_rate: i128,
    pub settled: i128,
}

#[contractevent(topics = ["transfer"], data_format = "vec")]
pub struct TransferStreamEvent {
    #[topic]
    pub stream_id: u64,
    pub old_ngo: Address,
    pub new_ngo: Address,
    pub settled: i128,
}

#[contractevent(topics = ["strmpause"], data_format = "vec")]
pub struct PausedStreamEvent {
    #[topic]
    pub stream_id: u64,
    pub settled: i128,
}

#[contractevent(topics = ["strmresume"], data_format = "vec")]
pub struct ResumedStreamEvent {
    #[topic]
    pub stream_id: u64,
}

#[contractevent(topics = ["treasset"], data_format = "single-value")]
pub struct TreasurySetEvent {
    pub treasury: Address,
}

#[contractevent(topics = ["feeset"], data_format = "single-value")]
pub struct FeeBpsSetEvent {
    pub fee_bps: u32,
}

#[contractevent(topics = ["tokfeeset"], data_format = "single-value")]
pub struct TokenFeeBpsSetEvent {
    #[topic]
    pub token: Address,
    pub fee_bps: u32,
}

#[contractevent(topics = ["tokfeeclr"], data_format = "single-value")]
pub struct TokenFeeBpsClearedEvent {
    #[topic]
    pub token: Address,
    pub data: (),
}

#[contractevent(topics = ["maxstrm"], data_format = "single-value")]
pub struct MaxStreamsPerDonorSetEvent {
    pub limit: u64,
}

#[contractevent(topics = ["maxrate"], data_format = "single-value")]
pub struct MaxRateSetEvent {
    pub max_rate: i128,
}

#[contractevent(topics = ["maxrtclr"], data_format = "single-value")]
pub struct MaxRateClearedEvent {
    pub data: (),
}

#[contractevent(topics = ["mindur"], data_format = "single-value")]
pub struct MinDurationSetEvent {
    pub min_duration: u64,
}

#[contractevent(topics = ["rescue"], data_format = "single-value")]
pub struct RescuedStreamEvent {
    #[topic]
    pub stream_id: u64,
    pub refund: i128,
}

#[contractevent(topics = ["treclr"], data_format = "single-value")]
pub struct TreasuryClearedEvent {
    pub data: (),
}

#[contractevent(topics = ["mindep"], data_format = "single-value")]
pub struct MinDepositSetEvent {
    pub min_deposit: i128,
}

#[contractevent(topics = ["grceset"], data_format = "single-value")]
pub struct CancelGraceLedgersSetEvent {
    pub grace_ledgers: u32,
}

#[contractevent(topics = ["regset"], data_format = "single-value")]
pub struct RegistrySetEvent {
    pub registry: Address,
}

#[contractevent(topics = ["regclr"], data_format = "single-value")]
pub struct RegistryClearedEvent {
    pub data: (),
}

#[contractevent(topics = ["payoutset"], data_format = "single-value")]
pub struct PayoutAddressSetEvent {
    #[topic]
    pub ngo: Address,
    pub payout: Address,
}

#[contractevent(topics = ["init"], data_format = "single-value")]
pub struct InitializedEvent {
    pub admin: Address,
}

#[contractevent(topics = ["upgrade"], data_format = "single-value")]
pub struct UpgradedEvent {
    pub new_wasm_hash: BytesN<32>,
}

#[contractevent(topics = ["uppropose"], data_format = "single-value")]
pub struct UpgradeProposedEvent {
    #[topic]
    pub new_wasm_hash: BytesN<32>,
    pub executable_at: u64,
}

#[contractevent(topics = ["upcancel"], data_format = "single-value")]
pub struct UpgradeCancelledEvent {
    pub new_wasm_hash: BytesN<32>,
}

#[contractevent(topics = ["updelay"], data_format = "single-value")]
pub struct UpgradeDelaySetEvent {
    pub delay: u64,
}

/// Emitted by `sweep_surplus` when the admin moves tokens that were sent to
/// the vault by mistake (above the committed balance for that token) to the
/// treasury. See issue #346.
#[contractevent(topics = ["sweep"], data_format = "vec")]
pub struct SurplusSweptEvent {
    #[topic]
    pub token: Address,
    pub amount: i128,
    pub treasury: Address,
}
