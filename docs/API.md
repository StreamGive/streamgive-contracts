# Contract API reference

This is the public Soroban entry-point reference. `admin` means the address
stored by the contract; `approver` means the admin or an address added with
`add_approver`. Read-only methods require no authorization and emit no event.
Errors below are the contract's `Error` variants; token transfer failures also
abort the invocation.

## donation-vault

| Function | Arguments / result | Auth | Errors | Events |
|---|---|---|---|---|
| `__constructor` | `admin: Address` / `()` | deployment | — | `init` |
| `init` | `admin: Address` / `Result<(), Error>` | `admin` auth during first init | `AlreadyInitialized`, `AdminRenounced` | `init` |
| `renounce_admin` | none / `Result<(), Error>` | admin | `NotInitialized`, `AdminRenounced` | `renounce` |
| `total_donated` | none / `i128` | none | — | — |
| `total_withdrawn` | none / `i128` | none | — | — |
| `total_fees_collected` | none / `i128` | none | — | — |
| `admin` | none / `Result<Address, Error>` | none | `NotInitialized`, `AdminRenounced` | — |
| `pending_admin` | none / `Option<Address>` | none | — | — |
| `propose_admin` | `new_admin: Address` / `Result<(), Error>` | admin | `NotInitialized`, `InvalidAdmin`, `AdminRenounced` | `propadmin` |
| `accept_admin` | none / `Result<(), Error>` | pending admin | `NotInitialized`, `NoPendingAdmin` | `acptadmin` |
| `cancel_admin_proposal` | none / `Result<(), Error>` | admin | `NotInitialized`, `NoPendingAdmin`, `AdminRenounced` | `canceladm` |
| `get_stream` | `stream_id: u64` / `Result<Stream, Error>` | none | `StreamNotFound` | — |
| `get_streams` | `ids: Vec<u64>` / `Result<Vec<Option<Stream>>, Error>` | none | `BatchLimitExceeded` | — |
| `streams_by_donor` | `donor: Address` / `Vec<u64>` | none | — | — |
| `donor_totals` | `donor: Address, token: Address` / `Result<DonorTotals, Error>` (committed, gross withdrawn) | none | `ArithmeticOverflow` | — |
| `streams_by_ngo` | `ngo: Address, start: u32, limit: u32` / `Vec<u64>` | none | — | — |
| `stream_count` | none / `u64` | none | — | — |
| `pending_accrual` | `stream_id: u64` / `Result<i128, Error>` | none | `StreamNotFound` | — |
| `depletion_time` | `stream_id: u64` / `Result<Option<u64>, Error>` | none | `StreamNotFound` | — |
| `pending_payout` | `stream_id: u64` / `Result<(i128, i128), Error>` (net, fee) | none | `StreamNotFound` | — |
| `pending_refund` | `stream_id: u64` / `Result<i128, Error>` | none | `StreamNotFound` | — |
| `extend_stream` | `stream_id: u64` / `Result<(), Error>` | none | `StreamNotFound` | — |
| `pause` | none / `Result<(), Error>` | admin | `NotInitialized`, `AlreadyPaused`, `AdminRenounced` | `pause` |
| `unpause` | none / `Result<(), Error>` | admin | `NotInitialized`, `AlreadyUnpaused`, `AdminRenounced` | `unpause` |
| `paused` | none / `bool` | none | — | — |
| `get_config` | none / `Config` | none | — | — |
| `set_treasury` | `treasury: Address` / `Result<(), Error>` | admin | `InvalidTreasury`, `AdminRenounced` | `treasset` |
| `treasury` | none / `Option<Address>` | none | — | — |
| `clear_treasury` | none / `Result<(), Error>` | admin | `AdminRenounced` | `treclr` |
| `set_fee_bps` | `fee_bps: u32` / `Result<(), Error>` | admin | `FeeTooHigh`, `FeeRequiresTreasury`, `AdminRenounced` | `feeset` |
| `fee_bps` | none / `u32` | none | — | — |
| `max_fee_bps` | none / `u32` | none | — | — |
| `set_token_fee_bps` | `token: Address, fee_bps: u32` / `Result<(), Error>` | admin | `FeeTooHigh`, `AdminRenounced` | `tokfeeset` |
| `clear_token_fee_bps` | `token: Address` / `Result<(), Error>` | admin | `AdminRenounced` | `tokfeeclr` |
| `token_fee_bps` | `token: Address` / `u32` | none | — | — |
| `allowed_tokens` | none / `Vec<Address>` | none | — | — |
| `set_min_deposit` | `min_deposit: i128` / `Result<(), Error>` | admin | `InvalidAmount`, `AdminRenounced` | `mindep` |
| `min_deposit` | none / `i128` | none | — | — |
| `set_cancel_grace_ledgers` | `grace_ledgers: u32` / `Result<(), Error>` | admin | `AdminRenounced` | `grceset` |
| `cancel_grace_ledgers` | none / `u32` | none | — | — |
| `set_max_streams_per_donor` | `limit: u64` / `Result<(), Error>` | admin | `InvalidAmount`, `AdminRenounced` | `maxstrm` |
| `max_streams_per_donor` | none / `u64` | none | — | — |
| `set_max_rate` | `max_rate: i128` / `Result<(), Error>` | admin | `InvalidAmount`, `AdminRenounced` | `maxrate` |
| `clear_max_rate` | none / `Result<(), Error>` | admin | `AdminRenounced` | `maxrtclr` |
| `max_rate` | none / `Option<i128>` | none | — | — |
| `set_min_duration` | `min_duration: u64` / `Result<(), Error>` | admin | `AdminRenounced` | `mindur` |
| `min_duration` | none / `u64` | none | — | — |
| `set_registry` | `registry: Address` / `Result<(), Error>` | admin | `AdminRenounced` | `regset` |
| `clear_registry` | none / `Result<(), Error>` | admin | `AdminRenounced` | `regclr` |
| `registry` | none / `Option<Address>` | none | — | — |
| `set_payout_address` | `ngo: Address, payout: Address` / `Result<(), Error>` | NGO owner | `NotInitialized`, `AdminRenounced` | `payoutset` |
| `payout_address` | `ngo: Address` / `Option<Address>` | none | — | — |
| `create_stream` | `donor, ngo, token: Address, deposit, rate: i128` / `Result<u64, Error>` | donor | `InvalidAmount`, `DepositTooLow`, `StreamLimitExceeded`, `NgoNotVerified`, `ContractPaused`, `SelfStream`, `InvalidNgo`, `InvalidToken`, `RateTooHigh`, `StreamTooShort`, `ArithmeticOverflow` | `created` |
| `create_stream_with_start` | above plus `start: u64` / `Result<u64, Error>` | donor | above plus `StartTooFar` | `created` |
| `create_stream_with_memo` | above plus `memo: Bytes` / `Result<u64, Error>` | donor | above plus `MemoTooLong` | `created` |
| `pause_stream` | `stream_id: u64` / `Result<i128, Error>` | donor | `StreamNotFound`, `StreamAlreadyPaused`, `StreamCancelled` | `streampause`, `drained` when applicable |
| `resume_stream` | `stream_id: u64` / `Result<(), Error>` | donor | `StreamNotFound`, `StreamNotPaused`, `StreamCancelled` | `streamresume` |
| `withdraw` | `stream_id: u64` / `Result<i128, Error>` | NGO / payout authorizer | `StreamNotFound`, `NothingToWithdraw`, `ContractPaused`, `StreamPaused`, `StreamCancelled` | `withdraw`, `drained` |
| `withdraw_batch` | `stream_ids: Vec<u64>` / `Result<Vec<i128>, Error>` | one NGO / payout authorizer | `StreamNotFound`, `NothingToWithdraw`, `MixedNgo`, `DuplicateStreamId`, `ContractPaused`, `StreamPaused` | one `withdraw` per settled stream, `drained` per emptied stream |
| `withdraw_all` | `ngo: Address, start: u32, limit: u32` / `Result<WithdrawAllPage, Error>` | NGO / payout authorizer | `NothingToWithdraw`, `ContractPaused`, `StreamNotFound` | `withdraw`, `drained` |
| `cancel_stream` | `stream_id: u64` / `Result<i128, Error>` | donor | `StreamNotFound`, `StreamCancelled`, `ContractPaused` | `cancel` |
| `rescue_stream` | `stream_id: u64` / `Result<i128, Error>` | admin, vault paused | `NotPaused`, `StreamNotFound`, `StreamCancelled`, `AdminRenounced` | `rescue` |
| `top_up` | `stream_id: u64, amount: i128` / `Result<(), Error>` | donor | `InvalidAmount`, `StreamNotFound`, `StreamCancelled`, `ContractPaused`, `ArithmeticOverflow` | `topup` |
| `modify_rate` | `stream_id: u64, new_rate: i128` / `Result<(), Error>` | donor | `InvalidAmount`, `StreamNotFound`, `StreamCancelled`, `ContractPaused`, `RateTooHigh`, `StreamTooShort` | `ratemod` |
| `transfer_stream` | `stream_id: u64, new_ngo: Address` / `Result<(), Error>` | donor | `StreamNotFound`, `StreamCancelled`, `ContractPaused`, `NgoNotVerified`, `InvalidNgo` | `transfer`, `drained` when applicable |
| `upgrade_delay` | none / `u64` | none | — | — |
| `pending_upgrade` | none / `Option<PendingUpgrade>` | none | — | — |
| `committed_balance` | `token: Address` / `i128` | none | — | — |
| `surplus` | `token: Address` / `i128` | none | — | — |
| `set_upgrade_delay` | `delay: u64` / `Result<(), Error>` | admin | `AdminRenounced` | `updelay` |
| `propose_upgrade` | `new_wasm_hash: BytesN<32>` / `Result<(), Error>` | admin | `AdminRenounced` | `uppropose` |
| `execute_upgrade` | none / `Result<(), Error>` | admin | `NoPendingUpgrade`, `UpgradeDelayNotElapsed`, `AdminRenounced` | `upgrade` |
| `cancel_upgrade` | none / `Result<(), Error>` | admin | `NoPendingUpgrade`, `AdminRenounced` | `upcancel` |
| `sweep_surplus` | `token: Address, amount: i128` / `Result<(), Error>` | admin | `NoTreasury`, `SweepExceedsSurplus`, `InvalidAmount`, `AdminRenounced` | `sweep` |
| `upgrade` | `new_wasm_hash: BytesN<32>` / `Result<(), Error>` | admin | `UpgradeTimelockEnabled`, `AdminRenounced` | `upgrade` |

## ngo-registry

The registry follows the same `admin` and pending-admin meanings. `get_ngo`
returns `NotRegistered`; boolean/list views return empty/false for absent data.

| Function | Arguments / result | Auth | Errors | Events |
|---|---|---|---|---|
| `__constructor` | `admin: Address` / `()` | deployment | — | `init` |
| `init` | `admin: Address` / `Result<(), Error>` | first initializer | `AlreadyInitialized`, `InvalidAdmin` | `init` |
| `admin` | none / `Result<Address, Error>` | none | `NotInitialized` | — |
| `pending_admin` | none / `Option<Address>` | none | — | — |
| `propose_admin` | `new_admin: Address` / `Result<(), Error>` | admin | `NotInitialized`, `InvalidAdmin` | `propadmin` |
| `accept_admin` | none / `Result<(), Error>` | pending admin | `NoPendingAdmin` | `acptadmin` |
| `cancel_admin_proposal` | none / `Result<(), Error>` | admin | `NoPendingAdmin` | `canceladm` |
| `pause_registrations` | none / `Result<(), Error>` | admin | `NotInitialized`, `RegistrationsPaused` | `regrpause` |
| `resume_registrations` | none / `Result<(), Error>` | admin | `NotInitialized` | `regresume` |
| `registrations_paused` | none / `bool` | none | — | — |
| `register` | `owner: Address, name: String` / `Result<(), Error>` | owner | `AlreadyRegistered`, `InvalidName`, `NameTooLong`, `RegistrationsPaused` | `register` |
| `total_ngos` | none / `u32` | none | — | — |
| `unregister` | `owner: Address` / `Result<(), Error>` | owner | `NotRegistered`, `AlreadyVerified` | `unregist` |
| `remove_ngo` | `ngo_owner: Address` / `Result<(), Error>` | admin | `NotRegistered`, `AlreadyVerified` | `removed` |
| `reject_ngo` | `ngo_owner: Address, reason_code: u32` / `Result<(), Error>` | approver | `NotRegistered`, `AlreadyVerified`, `NotApprover` | `rejected` |
| `add_approver` | `approver: Address` / `Result<(), Error>` | admin | `AlreadyApprover` | `apadd` |
| `remove_approver` | `approver: Address` / `Result<(), Error>` | admin | `NotApprover` | `aprem` |
| `is_approver` | `addr: Address` / `bool` | none | — | — |
| `list_approvers` | none / `Vec<Address>` | none | — | — |
| `list_ngos` | `start: u32, limit: u32` / `Vec<Address>` | none | — | — |
| `list_ngos_page` | `start: u32, limit: u32` / `NgoOwnerPage` | none | — | — |
| `update_name` | `owner: Address, name: String` / `Result<(), Error>` | owner | `NotRegistered`, `AlreadyVerified`, `InvalidName`, `NameTooLong` | `renamed` |
| `set_metadata_uri` | `owner: Address, metadata_uri: Option<String>` / `Result<(), Error>` | owner | `NotRegistered`, `MetadataUriTooLong` | `metadata` |
| `metadata_uri` | `owner: Address` / `Option<String>` | none | — | — |
| `get_ngo` | `owner: Address` / `Result<Ngo, Error>` | none | `NotRegistered` | — |
| `is_verified` | `owner: Address` / `bool` | none | — | — |
| `ngo_count` | none / `u64` | none | — | — |
| `verified_count` | none / `u64` | none | — | — |
| `set_verify_period` | `seconds: u64` / `Result<(), Error>` | admin | `NotInitialized`, `ArithmeticOverflow` | `verperiod` |
| `verify_period` | none / `u64` | none | — | — |
| `approve_ngo` | `approver: Address, ngo_owner: Address` / `Result<(), Error>` | approver | `NotApprover`, `NotRegistered`, `AlreadyVerified` | `approved` |
| `batch_approve` | `approver: Address, owners: Vec<Address>` / `Result<(), Error>` | approver | above; atomic on failure | one `approved` per owner |
| `revoke_ngo` | `approver: Address, ngo_owner: Address` / `Result<(), Error>` | approver | `NotApprover`, `NotRegistered`, `NotVerified` | `revoked` |
| `renew_ngo` | `ngo_owner: Address` / `Result<(), Error>` | admin | `NoVerifyPeriod`, `NotRegistered`, `NotVerified`, `ArithmeticOverflow` | `renew` |
| `touch_ngo` | `owner: Address` / `Result<(), Error>` | none | `NotRegistered` | — |
| `upgrade` | `new_wasm_hash: BytesN<32>` / `Result<(), Error>` | admin | `NotInitialized` | `upgrade` |

For event topic and payload layouts, see [EVENTS.md](./EVENTS.md). For
storage and TTL behavior, see [STORAGE.md](./STORAGE.md).

### Donor totals

`donor_totals` returns the funds still held for the donor's streams in the
requested token and the gross amount paid out, including protocol fees.
Refunds reduce the committed balance without increasing withdrawn.

New deployments index totals from the first stream. On upgraded deployments,
an unindexed donor and token are reconstructed by scanning existing stream
records. The view does not modify storage. The next successful fund movement
for the pair records the aggregate for future reads.
