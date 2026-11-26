# Contract events

Every event published by the contracts in this repo, with its topics and
data shape, so an indexer can be written directly against this repo without
going to the external documentation site first.

Soroban events have two parts:

- **Topics** — a tuple, always starting with a `Symbol` naming the event.
  Topics are indexed/filterable.
- **Data** — the event payload. Shown below as the Rust type(s) passed to
  `env.events().publish((topics...), data)`. A single value publishes as
  itself; a tuple of values publishes as an XDR array in that order.

The contracts define migrated event declarations with Soroban SDK's
`#[contractevent]` macro. Explicit topic names and data formats keep the wire
layout stable. Event type names include `RegisterEvent`, `RenamedEvent`,
`ApprovedEvent`, `RevokedEvent`, `UnregisteredEvent`, `RemovedEvent`,
`ProposedAdminEvent`, `AcceptedAdminEvent`, `CancelledAdminEvent`,
`PausedEvent`, `UnpausedEvent`, `RenouncedAdminEvent`, `TreasurySetEvent`, `TreasuryClearedEvent`,
`FeeBpsSetEvent`, `TokenFeeBpsSetEvent`, `TokenFeeBpsClearedEvent`, `TokenDepositCapSetEvent`,
`MinDepositSetEvent`, `CancelGraceLedgersSetEvent`, `RegistrySetEvent`,
`PayoutAddressSetEvent`, `MaxStreamsPerDonorSetEvent`, `MaxRateSetEvent`,
`MaxRateClearedEvent`, `InitializedEvent`,
`UpgradedEvent`, `ConfigSnapshotEvent`, `UpgradeProposedEvent`,
`UpgradeCancelledEvent`, `UpgradeDelaySetEvent`, `ApproverAddedEvent`,
`ApproverRemovedEvent`,
`MetadataUriSetEvent`, `RenewedEvent`, `VerifyPeriodSetEvent`, `CreatedEvent`,
`WithdrawnEvent`, `DrainedEvent`, `CancelledStreamEvent`, `RescuedStreamEvent`,
`ToppedUpEvent`, `RateModifiedEvent`, `TransferStreamEvent`,
`PausedStreamEvent`, `ResumedStreamEvent`, and `SurplusSweptEvent`.

`Address`, `i128`, `u64`, `u32`, `bool`, and `String` are the standard
Soroban SDK/XDR types (`Address` is the SDK's account/contract address
type, `String` is `soroban_sdk::String`).

## `ngo-registry`

### `propadmin`

Emitted by `propose_admin` when the registry admin nominates a replacement.

| | |
|---|---|
| Topics | `("propadmin",)` |
| Data | `new_admin: Address` |

### `acptadmin`

Emitted by `accept_admin` when the pending registry admin takes control.

| | |
|---|---|
| Topics | `("acptadmin",)` |
| Data | `new_admin: Address` |

### `canceladm`

Emitted by `cancel_admin_proposal` when the registry admin cancels the pending
transfer.

| | |
|---|---|
| Topics | `("canceladm",)` |
| Data | `()` (no payload) |

### `register`

Emitted by `register` when an NGO submits an application.

| | |
|---|---|
| Topics | `("register", owner: Address)` |
| Data | `name: String` |

The entry starts unverified (`verified: false`); look for a matching
`approved` event for the same `owner` to know when it's live.

### `unregist`

Emitted by `unregister` when an unverified NGO removes its own application.

| | |
|---|---|
| Topics | `("unregist", owner: Address)` |
| Data | `()` (no payload) |

### `renamed`

Emitted by `update_name` when an NGO changes the name on its application
before it has been approved.

| | |
|---|---|
| Topics | `("renamed", owner: Address)` |
| Data | `name: String` (the new name) |

Once an `approved` event has been seen for an `owner`, no further
`renamed` events can follow for it while it stays verified.

### `metadata`

Emitted by `set_metadata_uri` when an NGO changes the metadata URI on its own
entry (a reference to off-chain metadata such as a website, description, or
logo). It fires whenever the stored value changes, including when the URI is
cleared.

| | |
|---|---|
| Topics | `("metadata", owner: Address)` |
| Data | `metadata_uri: Option<String>` (`None` when cleared) |

Unlike `renamed`, `metadata` can follow an `approved` event: the URI is not
frozen by verification, so an approved NGO keeps its published reference
current. The value is at most `MAX_METADATA_URI_LEN` (512 bytes).

### `approved`

Emitted by `approve_ngo` when an admin marks a registered NGO as verified.

| | |
|---|---|
| Topics | `("approved", ngo_owner: Address)` |
| Data | `()` (no payload) |

When a verification period is configured, approval also stamps
`Ngo.verified_until` on the entry; the event payload itself is unchanged, so
read `get_ngo` for the expiry. A lapsed verification is reported as
unverified by `get_ngo` / `is_verified`, and is revived with `renew_ngo`
(see the `renew` event).

### `revoked`

Emitted by `revoke_ngo` when an admin reverses a prior approval, marking a
verified NGO as unverified again. This is the direct counterpart to
`approved` for the same `ngo_owner` — an indexer should treat a `revoked`
event as cancelling the most recent `approved` event for that address.

| | |
|---|---|
| Topics | `("revoked", ngo_owner: Address)` |
| Data | `()` (no payload) |

### `removed`

Emitted by `remove_ngo` when an admin deletes an unverified application
outright, without waiting for its owner to `unregister`. Like `unregist` this
is the owner-initiated counterpart, but admin-initiated and carrying the same
payload. A verified NGO cannot be removed directly: it must be revoked first,
so a `removed` event never follows an un-revoked `approved` for the same
`owner`. `remove_ngo` is the reason-less variant of `reject_ngo`; prefer
`reject_ngo` when the applicant should learn why.

| | |
|---|---|
| Topics | `("removed", owner: Address)` |
| Data | `()` (no payload) |

### `rejected`

Emitted by `reject_ngo` when the admin — or a listed approver — declines a
pending application, removing the entry and recording why. The `reason_code`
is the caller-supplied integer passed to `reject_ngo`: an opaque,
off-chain-defined code the applicant's client can map to a human explanation.
The contract keeps no other record of the decision.

| | |
|---|---|
| Topics | `("rejected", owner: Address)` |
| Data | `reason_code: u32` |

Like `removed`, a `rejected` event only follows an unapproved entry: a verified
NGO must be `revoke`d first, so `reject_ngo` never drops a live verification and
never emits for an already-verified address (it fails with `AlreadyVerified`).

### `init`

Emitted by the registry constructor and by `init` when the admin is first set,
so an indexer can learn the contract's admin from deployment events alone.

| | |
|---|---|
| Topics | `("init",)` |
| Data | `admin: Address` |

### `upgrade`

Emitted by `upgrade` when an admin replaces the registry's Wasm bytecode.

| | |
|---|---|
| Topics | `("upgrade",)` |
| Data | `new_wasm_hash: BytesN<32>` |

### `apadd`

Emitted by `add_approver` when the admin adds an address to the approver list,
granting it the right to call `approve_ngo`, `batch_approve`, `reject_ngo`, and
`revoke_ngo`.

| | |
|---|---|
| Topics | `("apadd", approver: Address)` |
| Data | `()` (no payload) |

An indexer can maintain the current approver set from `apadd`/`aprem` events
without polling `list_approvers`. The admin is always able to approve and
revoke and does not need to be — and is never added to — the list, so no
`apadd` event is emitted for it.

### `aprem`

Emitted by `remove_approver` when the admin removes an address from the
approver list, revoking that right.

| | |
|---|---|
| Topics | `("aprem", approver: Address)` |
| Data | `()` (no payload) |

Removing an address that is not on the list fails with `NotApprover` and
emits nothing.

### `renew`

Emitted by `renew_ngo` when the admin extends an NGO's verification, moving
its expiry forward from the current ledger time by the configured verification
period. The `data` is the new expiry as a ledger timestamp, and it is
readable as `Ngo.verified_until` from `get_ngo`.

| | |
|---|---|
| Topics | `("renew", owner: Address)` |
| Data | `verified_until: u64` |

A verification that has lapsed can be renewed as long as its stored flag is
still set; one that was revoked must be approved again first.

### `verperiod`

Emitted by `set_verify_period` when the admin changes the verification period.
`data` is the new period in seconds; `0` means approvals never expire. The
period applies to `approve_ngo` / `batch_approve` and `renew_ngo` from that
point on, and does not alter entries already approved.

| | |
|---|---|
| Topics | `("verperiod",)` |
| Data | `verify_period: u64` |

## `donation-vault`

### `init`

Emitted by the vault constructor and by `init` when the admin is first set, so
an indexer can learn the contract's admin from deployment events alone.

| | |
|---|---|
| Topics | `("init",)` |
| Data | `admin: Address` |

### `propadmin`

Emitted by `propose_admin` when the current admin nominates a new admin.
The transfer is not complete until the nominated address calls
`accept_admin` and an `acptadmin` event is emitted.

| | |
|---|---|
| Topics | `("propadmin",)` |
| Data | `new_admin: Address` |

### `acptadmin`

Emitted by `accept_admin` when the nominated admin accepts the transfer.
After this event the address in `data` is the active admin; the previous
admin has no further authority.

| | |
|---|---|
| Topics | `("acptadmin",)` |
| Data | `new_admin: Address` |

### `canceladm`

Emitted by `cancel_admin_proposal` when the current admin clears a pending
admin transfer.

| | |
|---|---|
| Topics | `("canceladm",)` |
| Data | `()` (no payload) |

### `renounce`

Emitted by `renounce_admin` when admin control is permanently disabled.

| | |
|---|---|
| Topics | `("renounce",)` |
| Data | `()` (no payload) |

### `pause`

Emitted by `pause` when an admin halts stream creation, withdrawal, top-up,
and rate changes.

| | |
|---|---|
| Topics | `("pause",)` |
| Data | `()` (no payload) |

### `unpause`

Emitted by `unpause` when an admin lifts a pause.

| | |
|---|---|
| Topics | `("unpause",)` |
| Data | `()` (no payload) |

### `feeset`

Emitted by `set_fee_bps` when an admin changes the protocol fee.

| | |
|---|---|
| Topics | `("feeset",)` |
| Data | `fee_bps: u32` (the new fee, in basis points) |

The fee is capped at `MAX_FEE_BPS` (1,000 / 10%); calls above the cap fail
with `FeeTooHigh` and emit nothing. The event carries the full new value
(not a delta), so an indexer can track the fee without polling `fee_bps`.
"Accepted" here means stored, not effective: the fee only affects payouts
once a treasury is set, so pair this with `set_treasury`/`treasury()` when
deriving an actual split.

### `tokfeeset`

Emitted by `set_token_fee_bps` when an admin configures a per-token fee override.

| | |
|---|---|
| Topics | `("tokfeeset", token: Address)` |
| Data | `fee_bps: u32` (the token fee override, in basis points) |

The override takes precedence over the global `fee_bps` for streams using this token.
An explicit override of `0` means zero fees for this token even when a global fee is set.

### `tokfeeclr`

Emitted by `clear_token_fee_bps` when an admin removes a per-token fee override.

| | |
|---|---|
| Topics | `("tokfeeclr", token: Address)` |
| Data | `()` (no payload) |

After clearing, streams using this token fall back to the global `fee_bps` setting.
If no override existed for the token, the call is a clean no-op and no event is emitted.

### `treasset`

Emitted by `set_treasury` when an admin changes where protocol fees are paid.

| | |
|---|---|
| Topics | `("treasset",)` |
| Data | `treasury: Address` (the new treasury address) |

The event carries the new treasury address so an off-chain indexer can track
where protocol fees will flow without polling `treasury()`. Protocol fees only
affect payouts once a treasury is configured, so pair this with `feeset`/`fee_bps()`
when deriving an actual split.

### `treclr`

Emitted by `clear_treasury` when an admin removes the configured treasury.

| | |
|---|---|
| Topics | `("treclr",)` |
| Data | `()` (no payload) |

Clearing the treasury is rejected while a non-zero `fee_bps` is set, so a
`treclr` event never follows a `feeset` with a non-zero value without an
intervening `feeset` of `0`. After a `treclr` event, protocol fees stop
accruing until a new `treasset` event appears.

### `mindep`

Emitted by `set_min_deposit` when an admin changes the minimum stream deposit.

| | |
|---|---|
| Topics | `("mindep",)` |
| Data | `min_deposit: i128` (the new minimum) |

The event carries the full new value so an indexer can track the minimum
without polling `min_deposit()`. A negative value fails with `InvalidAmount`
and emits nothing.

### `maxstrm`

Emitted by `set_max_streams_per_donor` when the lifetime per-donor creation
cap changes.

| | |
|---|---|
| Topics | `("maxstrm",)` |
| Data | `limit: u64` |

The cap is checked before the donor's token transfer. Existing streams are not
deleted when the cap changes; the stored counter is a lifetime creation count.

### `grceset`

Emitted by `set_cancel_grace_ledgers` when an admin changes the extra ledger
retention window applied to cancelled stream records.

| | |
|---|---|
| Topics | `("grceset",)` |
| Data | `grace_ledgers: u32` (the new window, in ledgers) |

### `tokcap`

Emitted when an admin sets or clears the maximum committed balance for a token.

| | |
|---|---|
| Topics | `("tokcap", token: Address)` |
| Data | `cap: Option<i128>` (`None` is XDR void and removes the cap) |

### `maxrate`

Emitted by `set_max_rate` when an admin caps the per-second rate accepted by
`create_stream` and `modify_rate`.

| | |
|---|---|
| Topics | `("maxrate",)` |
| Data | `max_rate: i128` (the new ceiling, in token units per second) |

Rates above this ceiling are rejected with `RateTooHigh`. A ceiling of `0` or
less fails with `InvalidAmount` and emits nothing. Until a ceiling is set, the
event never appears and rates are uncapped.

### `maxrtclr`

Emitted by `clear_max_rate` when an admin removes the per-second rate ceiling.

| | |
|---|---|
| Topics | `("maxrtclr",)` |
| Data | `()` |

After this event, `create_stream` and `modify_rate` accept any positive rate
again, the same as if `set_max_rate` had never been called.

### `mindur`

Emitted by `set_min_duration` when an admin changes the shortest stream
lifetime — the shortest time to depletion — accepted by `create_stream` and
`modify_rate`.

| | |
|---|---|
| Topics | `("mindur",)` |
| Data | `min_duration: u64` (the new minimum, in seconds; `0` disables the check) |

A stream whose deposit would drain in fewer than this many seconds is rejected
with `StreamTooShort`, a code distinct from `InvalidAmount`. Until an admin
sets a value, the event never appears and the minimum is `0` (no floor), so the
check is skipped entirely.

### `regset`

Emitted by `set_registry` when an admin points the vault at an NGO registry
contract.

| | |
|---|---|
| Topics | `("regset",)` |
| Data | `registry: Address` (the new registry address) |

Once set, `create_stream` verifies the NGO against this registry. To clear the
registry pointer back to `None`, the admin calls `clear_registry` (emitting `regclr`).

### `regclr`

Emitted by `clear_registry` when an admin clears the configured NGO registry contract.

| | |
|---|---|
| Topics | `("regclr",)` |
| Data | `()` |

Clearing the registry removes the stored registry address (`registry()` returns `None`),
so subsequent `create_stream` calls accept any recipient address without registry
verification.

### `payoutset`

Emitted by `set_payout_address` when an NGO points its withdrawals at a
different address.

| | |
|---|---|
| Topics | `("payoutset", ngo: Address)` |
| Data | `payout: Address` (the new payout address) |

`withdraw`, `withdraw_batch`, and `withdraw_all` pay the net amount to this
address while the
NGO owner key continues to authorise those calls. Until an NGO sets one,
`payout_address()` returns `None` and payouts go to the owner address itself.
An indexer can track a given NGO's payout destination from these events without
polling `payout_address`.

### `upgrade`

Emitted by `upgrade` and by `execute_upgrade` when an admin replaces the
vault's Wasm bytecode.

| | |
|---|---|
| Topics | `("upgrade",)` |
| Data | `new_wasm_hash: BytesN<32>` |

`upgrade` only publishes this while the upgrade timelock is disabled (a
configured `upgrade_delay` of zero). With a non-zero delay it returns
`Error::UpgradeTimelockEnabled`, and the code change happens through
`execute_upgrade` instead (#343).

### `config_snapshot`

Emitted by `execute_upgrade` just before its `upgrade` event, carrying the
full configuration in force as the upgraded code takes over. Settings newly
introduced by an upgrade take their defaults silently; this event gives an
indexer a known starting point for the new contract version instead of
leaving it to infer when a value changed (#443).

| | |
|---|---|
| Topics | `("config_snapshot",)` |
| Data | `config: Config` (the same struct returned by `get_config`) |

The payload includes `paused`, `treasury`, `fee_bps`, `min_deposit`,
`max_streams_per_donor`, `max_rate`, `min_duration`, `registry`,
`cancel_grace_ledgers`, `total_donated`, `total_withdrawn`,
`total_fees_collected`, `pending_admin`, and `renounced`. It is published only
by `execute_upgrade`; the immediate `upgrade` entry point does not emit one.

### `uppropose`

Emitted by `propose_upgrade` when an admin records the Wasm hash to upgrade to
and the timestamp from which the upgrade may be executed.

| | |
|---|---|
| Topics | `("uppropose", new_wasm_hash: BytesN<32>)` |
| Data | `executable_at: u64` |

`executable_at` is the ledger timestamp before which `execute_upgrade` returns
`Error::UpgradeDelayNotElapsed`. Proposing again replaces any pending proposal
and restarts the timer (#343).

### `upcancel`

Emitted by `cancel_upgrade` when an admin withdraws a pending proposal before
it is executed.

| | |
|---|---|
| Topics | `("upcancel",)` |
| Data | `new_wasm_hash: BytesN<32>` |

The hash is the one that was pending, so an indexer can correlate the
cancellation with the earlier `uppropose` (#343).

### `updelay`

Emitted by `set_upgrade_delay` when an admin changes how long the upgrade
timelock lasts.

| | |
|---|---|
| Topics | `("updelay",)` |
| Data | `delay: u64` (the new delay in seconds; `0` disables the timelock) |

### `sweep`

Emitted by `sweep_surplus` when an admin moves tokens that were sent to the
vault by mistake to the treasury.

| | |
|---|---|
| Topics | `("sweep", token: Address)` |
| Data | `(amount: i128, treasury: Address)` |

`amount` never exceeds the surplus above the token's committed balance (the sum
of stream balances the vault holds), so a sweep can never touch funds belonging
to streams. `treasury` is the payout address the surplus went to. An indexer can
reconstruct the recoverable surplus at any time as the vault's token balance
minus `committed_balance(token)` (#346).

### `rescue`

Emitted by `rescue_stream` when the paused-vault emergency path refunds a
stream's remaining balance to its donor and permanently cancels it.

| | |
|---|---|
| Topics | `("rescue", stream_id: u64)` |
| Data | `refund: i128` |

The call is admin-only and fails with `NotPaused` unless the vault is paused.

### `created`

Emitted by `create_stream` when a donor opens a new stream.

| | |
|---|---|
| Topics | `("created", stream_id: u64)` |
| Data | `(donor: Address, ngo: Address, token: Address, deposit: i128, rate: i128, memo: Bytes)` |

`deposit` is the amount pulled from the donor into the vault; `rate` is how
much of it accrues to the NGO per second (see [`math::accrued`](../contracts/donation-vault/src/math.rs)).
`memo` is the donor's optional free-form tag, e.g. a campaign id, capped at
`MAX_MEMO_BYTES` (32) bytes and empty when none was supplied (#341). It is a
short label only — never truncate or reinterpret it.

### `withdraw`

Emitted by `withdraw` when an NGO claims everything accrued on a stream
since the last checkpoint. `withdraw_batch` and `withdraw_all` emit one of
these per contributing stream, so an indexer handles single and paginated
withdrawals identically.

| | |
|---|---|
| Topics | `("withdraw", stream_id: u64)` |
| Data | `(accrued: i128, fee: i128, net: i128, payout: Address)` |

`accrued` is the gross amount released from the stream's balance. `fee` is the
protocol fee paid to the treasury (or `0` if no treasury is set or the effective
fee is 0%). `net` is the net amount actually transferred to the NGO's payout
destination. `payout` is where that net amount was sent: the NGO's configured
payout address if one is set via `set_payout_address`, otherwise the NGO owner
address itself. The invariant `accrued = fee + net` holds for every payout.

The `withdraw` entry point returns that same `net` amount, so callers displaying
the payout receive the exact token units transferred to the NGO.

### `drained`

Emitted once when a stream's balance reaches zero and its status flips to
`Drained`. It marks the stream as finished for an indexer, which otherwise has
to recompute the transition from the `withdraw` events (#344).

| | |
|---|---|
| Topics | `("drained", stream_id: u64)` |
| Data | `(total_paid: i128)` |

`total_paid` is the stream's cumulative `withdrawn` total at the moment it
emptied — the lifetime amount paid to the NGO, not just the final
withdrawal. It is emitted whenever the drain comes from settling a stream:
`withdraw`, `withdraw_batch` (one per emptied stream), `modify_rate`,
`transfer_stream`, and `pause_stream`. For a plain `withdraw` it follows that
call's `withdraw` event; for the other entry points it is published while the
stream is settled, so it precedes their own `ratemod` / `transfer` /
`strmpause` event. `cancel_stream` and `top_up` never emit it — they settle a
stream only to reclassify it (Cancelled, or reactivated Active), which is not
a drain.

### `cancel`

Emitted by `cancel_stream` when a donor stops a stream for good.
`cancel_streams` emits one of these per stream it cancels, with the same
topics and data, so an indexer handles single and batch cancellations
identically.

| | |
|---|---|
| Topics | `("cancel", stream_id: u64)` |
| Data | `(accrued: i128, refund: i128)` |

`accrued` is whatever had accrued to the NGO and was paid out (subject to
the same protocol-fee split as `withdraw`) as part of settling the stream;
`refund` is the untouched remainder returned to the donor. The stream
record is kept with `balance` and `rate` zeroed, not deleted.

### `topup`

Emitted by `top_up` when a donor adds more funds to an existing stream.

| | |
|---|---|
| Topics | `("topup", stream_id: u64)` |
| Data | `(amount: i128, settled: i128)` |

`amount` is the newly added deposit. Any balance already accrued at the time of the
top-up is settled to the NGO first (as its own implicit payout, without emitting a
separate `withdraw` event) before the deposit is added; `settled` reports that gross
settled amount.

### `ratemod`

Emitted by `modify_rate` when a donor changes a stream's per-second accrual
rate.

| | |
|---|---|
| Topics | `("ratemod", stream_id: u64)` |
| Data | `(old_rate: i128, new_rate: i128, settled: i128)` |

As with `top_up`, any balance already accrued at the old rate is settled to
the NGO first, so the new rate only ever applies going forward. `settled` reports
the gross amount settled to the NGO at the old rate. The event records both rates
so an indexer can calculate the change without another state query.

### `transfer_stream`

Emitted by `transfer_stream` when a donor reassigns an ongoing stream to a
different NGO.

| | |
|---|---|
| Topics | `("transfer", stream_id: u64)` |
| Data | `(old_ngo: Address, new_ngo: Address, settled: i128)` |

Anything accrued to the old NGO before the transfer is settled to it as
part of the call (subject to the same protocol-fee split as `withdraw`),
without emitting a separate `withdraw` event; `settled` reports that gross
amount settled to `old_ngo`. After this event, future accrual goes to the
`new_ngo` in `data`. The donor and the stream's balance and rate are unchanged.
