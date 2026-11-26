# Storage keys and TTL policy

Which `DataKey` lives in which Soroban storage type, and how its TTL is
managed, directly affects fees and archival risk. This is written out here
so that isn't something you have to reconstruct from the constants in both
contracts' `lib.rs` files.

## Storage types, briefly

Soroban has two storage types relevant here (a third, temporary, isn't
used by either contract):

- **Instance** storage lives alongside the contract instance itself. It's
  cheap to read/write and its TTL is extended as a single unit — bumping
  one instance entry's TTL bumps them all.
- **Persistent** storage is per-entry: each entry has its own TTL and must
  be extended independently, or it can be archived once its TTL expires
  (still recoverable on-chain, but at extra cost to restore).

Both contracts extend TTLs eagerly on every state-changing call, so an
entry only expires from prolonged inactivity, not normal use. Ledger
counts below assume a 5-second average ledger close time
(`DAY_IN_LEDGERS = 17,280`, defined in each contract).

## `donation-vault`

| Key | Storage | Bump / threshold | Purpose |
| --- | --- | --- | --- |
| `Admin` | Instance | 30d / 29d | The address that can pause/unpause, set the fee, and manage the treasury. |
| `PendingAdmin` | Instance | 30d / 29d | The pending admin address (`Address`) proposed by `propose_admin`, awaiting `accept_admin`. |
| `NextStreamId` | Instance | 30d / 29d | Auto-incrementing counter handed out by `create_stream`. |
| `Paused` | Instance | 30d / 29d | Emergency-brake flag checked by `require_not_paused`. |
| `Treasury` | Instance | 30d / 29d | Address that receives the protocol fee cut on withdrawal. |
| `FeeBps` | Instance | 30d / 29d | Protocol fee, in basis points, capped at `MAX_FEE_BPS` (1,000 / 10%). |
| `TokenFeeBps(Address)` | Persistent | 30d / 29d | Optional per-token fee override; absent means use `FeeBps`. |
| `TotalDonated` | Instance | 30d / 29d | Lifetime sum of successful deposits and top-ups. |
| `TotalWithdrawn` | Instance | 30d / 29d | Lifetime gross amount settled to NGOs. |
| `TotalFeesCollected` | Instance | 30d / 29d | Lifetime protocol fees sent to the treasury. |
| `AllowedTokens` | Instance | 30d / 29d | Token addresses exposed to clients as the configured allowlist. |
| `MaxStreamsPerDonor` | Instance | 30d / 29d | Lifetime stream-creation cap per donor; default is 100. |
| `DonorStreamCount(Address)` | Persistent | 90d / 89d | Number of streams created by a donor, used by the cap. |
| `MinDeposit` | Instance | 30d / 29d | Minimum deposit accepted by stream creation. |
| `CancelGraceLedgers` | Instance | 30d / 29d | Additional ledgers to retain cancelled stream records for indexing. |
| `Registry` | Instance | 30d / 29d | Optional NGO registry consulted during stream creation and transfer. |
| `AdminRenounced` | Instance | 30d / 29d | Permanent marker that disables all admin-gated calls after renunciation. |
| `Stream(u64)` | Persistent | 90d / 89d | One donor→NGO stream record, keyed by stream id. Extended on every `create_stream`, `withdraw`, `top_up`, `cancel_stream`, or rate change touching that stream. |

All instance keys share one TTL (bumped to 30 days, refreshed once it
would otherwise drop below 29 days remaining) via `extend_instance_ttl`,
called on every state-changing entry point. Each `Stream(u64)` entry gets
its own 90-day TTL via `extend_stream_ttl`, called whenever that specific
stream is touched — an untouched stream can still expire independently of
the instance and of other streams.

On cancellation, the stream TTL is bumped to the normal 90-day retention
period plus the configured `cancel_grace_ledgers`. This gives indexers a
configurable window to observe the cancellation before the record becomes
eligible for archival.

## `ngo-registry`

| Key | Storage | Bump / threshold | Purpose |
| --- | --- | --- | --- |
| `Admin` | Instance | 30d / 29d | The address that can `approve_ngo` / `revoke_ngo`. |
| `PendingAdmin` | Instance | 30d / 29d | Proposed replacement admin awaiting acceptance. |
| `NgoCount` | Instance | 30d / 29d | Lifetime count of NGO registrations. |
| `TotalNgos` | Instance | 30d / 29d | Reserved legacy key retained for storage-layout compatibility; callers use `NgoCount`. |
| `Ngo(Address)` | Persistent | 90d / 89d | One NGO's registry entry (name, verified flag), keyed by its owner address. |
| `OwnerByName(String)` | Persistent | 90d / 89d | The owner that claimed an NGO name, read by `owner_by_name`. Set by `register` and `update_name`, cleared by `update_name`, `unregister`, `remove_ngo` and `reject_ngo`; refreshed by `touch_ngo`. |
| `OwnerIndexCount` | Instance | 30d / 29d | Current number of owners in the registry index. |
| `OwnerByIndex(u32)` | Persistent | 90d / 89d | Owner address at an index in registration order. |
| `IndexByOwner(Address)` | Persistent | 90d / 89d | Reverse index from owner address to its registration index. |
| `Approvers` | Instance | 30d / 29d | Addresses authorized to approve, reject, and revoke NGOs. |
| `VerifiedCount` | Instance | 30d / 29d | Stored count of entries whose verification flag is set. |
| `VerifyPeriod` | Instance | 30d / 29d | Verification lifetime in seconds; zero means no expiry. |
| `RegistrationsPaused` | Instance | 30d / 29d | Whether new registrations are paused by the admin. Absent or `false` means registrations are open. |

As with `donation-vault`, the instance TTL is refreshed on every
state-changing call via `extend_instance_ttl`. Each `Ngo(Address)` entry
gets its own 90-day TTL via `extend_ngo_ttl`, refreshed by `register`,
`approve_ngo`, and `revoke_ngo` for that specific entry — an NGO that
registers once and is never approved, revoked, or re-touched can still
have its entry archived independently of the registry's admin data.

## Implication for fee estimation

Instance data is cheap to keep alive since one bump covers every instance
key at once. Persistent per-entry data (`Stream` and `Ngo` records) is
where inactivity risk concentrates — an old stream or NGO entry nobody
interacts with for 90 days becomes eligible for archival, and reading it
back afterward costs a restore in addition to the read.

## TTL expiry and keep-alive entry points

### Expiry risk for idle entries

Soroban manages on-chain storage retention through Time-To-Live (TTL) measured
in closed ledgers. Assuming an average 5-second ledger close time (`DAY_IN_LEDGERS = 17,280`):

- **Instance storage**: Bumped to 30 days (`518,400` ledgers) on every state-changing
  call. Because all instance entries share a single TTL, normal contract usage
  keeps instance state alive continuously.
- **Persistent storage**: Each `Stream(u64)` and `Ngo(Address)` entry has an
  independent 90-day TTL (`1,555,200` ledgers).

If a stream or NGO record receives no interactions for 90 consecutive days, its TTL
reaches zero and the network **archives** the entry.

#### Consequences of archival

1. **Inaccessibility**: Any invocation that reads or mutates an archived key
   (such as `get_stream`, `withdraw`, `top_up`, or NGO verification lookups)
   will fail to access the data.
2. **Restoration overhead**: Recovering an archived entry requires constructing
   and submitting a Soroban state restoration footprint transaction and paying
   network restoration fees before standard contract operations can resume.
3. **High-risk operational profiles**:
   - **Long-duration, low-rate streams**: A stream scheduled over several months or
     years with infrequent or deferred withdrawals risks expiring before all funds
     have been claimed.
   - **Dormant verified NGOs**: Verified NGOs that experience no administrative
     updates (`approve_ngo`, `revoke_ngo`, `update_name`) risk having their persistent
     registry entries archived, which could prevent new streams from being created
     for them if registry verification is enforced.

### Keep-alive entry points

To prevent idle entries from expiring without requiring state modifications, fund transfers,
or admin keys, both contracts provide dedicated, permissionless keep-alive functions:

#### 1. `donation-vault`: `extend_stream(stream_id: u64)`

- **Authorization**: None required (`permissionless`). Callable by donors, recipient
  NGOs, off-chain bots, indexers, or any third party.
- **Behavior**: Verifies that `DataKey::Stream(stream_id)` exists in persistent storage
  and extends its TTL to 90 days (`STREAM_BUMP_AMOUNT = 1,555,200` ledgers) whenever
  remaining lifetime is below `STREAM_LIFETIME_THRESHOLD = 1,537,920` ledgers.
- **State safety**: Does not alter the stream's accrued balance, rate, or status, and
  transfers zero tokens.
- **Paused state**: Can be executed even while the contract is paused via `pause()`.
- **Errors**: Returns `Error::StreamNotFound` (code 3) if no stream exists for `stream_id`.

```rust
// Refresh an idle stream's TTL back to 90 days:
client.extend_stream(&stream_id);
```

#### 2. `ngo-registry`: `touch_ngo(owner: Address)`

- **Authorization**: None required (`permissionless`). Callable by any account.
- **Behavior**: Verifies that `DataKey::Ngo(owner)` exists, bumps instance storage
  to 30 days, and extends the NGO's persistent storage TTL to 90 days
  (`NGO_BUMP_AMOUNT = 1,555,200` ledgers).
- **State safety**: Leaves the NGO's name and verification status unmodified.
- **Errors**: Returns `Error::NotRegistered` (code 4) if `owner` has no registry entry.

```rust
// Refresh a registered NGO entry's TTL back to 90 days:
client.touch_ngo(&ngo_owner_address);
```

### Standard lifecycle entry points that refresh TTL

The following table summarizes all entry points that automatically bump storage TTLs:

| Contract | Function | Storage Key(s) Bumped | Authorization Required |
| --- | --- | --- | --- |
| `donation-vault` | `create_stream` | Instance (30d), `Stream(id)` (90d) | Donor |
| `donation-vault` | `withdraw` | Instance (30d), `Stream(id)` (90d) | NGO |
| `donation-vault` | `withdraw_batch` | Instance (30d), every `Stream(id)` (90d) | NGO |
| `donation-vault` | `withdraw_all` | Instance (30d), `Stream(id)` (90d) — one page of the NGO's streams | NGO |
| `donation-vault` | `rescue_stream` | Instance (30d), `Stream(id)` (90d + cancellation grace) | Admin (paused vault) |
| `donation-vault` | `top_up` | Instance (30d), `Stream(id)` (90d) | Donor |
| `donation-vault` | `modify_rate` | Instance (30d), `Stream(id)` (90d) | Donor |
| `donation-vault` | `cancel_stream` | Instance (30d), `Stream(id)` (90d + `cancel_grace_ledgers`) | Donor |
| `donation-vault` | `transfer_stream` | Instance (30d), `Stream(id)` (90d), old/new NGO indexes (90d) | Donor |
| `donation-vault` | `extend_stream` | `Stream(id)` (90d) | **None (Permissionless)** |
| `ngo-registry` | `register` | Instance (30d), `Ngo(owner)` (90d) | Owner |
| `ngo-registry` | `approve_ngo` | Instance (30d), `Ngo(owner)` (90d) | Admin or approver |
| `ngo-registry` | `revoke_ngo` | Instance (30d), `Ngo(owner)` (90d) | Admin or approver |
| `ngo-registry` | `add_approver` | Instance (30d) | Admin |
| `ngo-registry` | `remove_approver` | Instance (30d) | Admin |
| `ngo-registry` | `update_name` | Instance (30d), `Ngo(owner)` (90d) | Owner |
| `ngo-registry` | `touch_ngo` | Instance (30d), `Ngo(owner)` (90d) | **None (Permissionless)** |

The complete state-changing entry-point matrix is below; entries not listed in
the earlier lifecycle rows touch only instance storage. This includes admin
handshake and upgrade operations, which are often missed when estimating TTL
refreshes.

| Contract entry point(s) | TTL refreshed | Authorization |
|---|---|---|
| vault: `__constructor`, `init`, `renounce_admin`, `propose_admin`, `accept_admin`, `cancel_admin_proposal` | Instance (30d) | deployment/admin/pending admin as applicable |
| vault: `pause`, `unpause`, `set_treasury`, `clear_treasury`, `set_fee_bps`, `set_token_fee_bps`, `clear_token_fee_bps`, `set_min_deposit`, `set_cancel_grace_ledgers`, `set_max_streams_per_donor`, `set_max_rate`, `clear_max_rate`, `set_min_duration`, `set_registry`, `clear_registry`, `set_payout_address` | Instance (30d); `TokenFeeBps(token)` and payout keys persistent (90d) | admin, or NGO owner for payout |
| vault: `pause_stream`, `resume_stream`, `top_up`, `modify_rate` | Instance (30d), `Stream(id)` (90d), relevant donor/NGO index (90d) | donor |
| vault: `set_upgrade_delay`, `propose_upgrade`, `execute_upgrade`, `cancel_upgrade`, `upgrade`, `sweep_surplus` | Instance (30d); committed-balance key for `sweep_surplus` (30d) | admin |
| registry: `__constructor`, `init`, `propose_admin`, `accept_admin`, `cancel_admin_proposal`, `add_approver`, `remove_approver`, `set_verify_period`, `upgrade` | Instance (30d) | deployment/admin/pending admin as applicable |
| registry: `register`, `unregister`, `remove_ngo`, `reject_ngo`, `update_name`, `set_metadata_uri`, `approve_ngo`, `batch_approve`, `revoke_ngo`, `renew_ngo` | Instance (30d), affected `Ngo(owner)` and owner-index entries (90d) | owner, admin, or approver as applicable |

Constructor and initialization calls bump instance storage. Admin/configuration
calls (`renounce_admin`, the admin handshake, pause controls, fee/limit/
registry/payout configuration, and upgrade configuration) bump the instance
TTL. Read-only views do not bump TTL. `cancel_stream`, `rescue_stream`, and
`transfer_stream` are listed explicitly above because they also refresh the
affected persistent stream and index entries. `withdraw_batch` refreshes every
stream it inspects successfully; `withdraw_all` refreshes the page's streams.

