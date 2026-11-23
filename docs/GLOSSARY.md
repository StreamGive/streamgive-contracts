# StreamGive Contract Glossary

This glossary defines the domain terms, lifecycle states, and storage concepts used across the `donation-vault` and `ngo-registry` Soroban smart contracts and their companion documentation.

---

## Core Stream & Accounting Terms

### Accrual
The passive accumulation of funds owed to an NGO over elapsed ledger time. Nothing moves on-chain between checkpoints; accrual is computed lazily on demand as:
```text
accrued = min(rate * elapsed, balance)
```
where `elapsed = max(0, now - max(last_update, start))` (implemented in `contracts/donation-vault/src/math.rs`).
- **Example:** A stream with `rate = 100` stroops/sec and `balance = 10_000` accrues `6_000` stroops after `60` seconds of elapsed ledger time past `start`.

### Settle (Checkpoint)
The operation of realizing pending accrual at the current ledger timestamp (`now`), deducting `accrued` from the stream's remaining `balance`, adding it to `withdrawn`, transferring the net payout to the NGO (and any protocol fee to the `Treasury`), and advancing `last_update = now`.
- **Example:** Calling `withdraw`, `withdraw_batch`, `update_rate`, `transfer_stream`, or `cancel_stream` first settles any accrued balance at the old parameters before applying state changes.

### Cliff (`start`)
The scheduled ledger timestamp (in Unix seconds) at which a stream begins accruing. For streams created via `create_stream` or `create_stream_with_memo`, `start == created_at`. For streams created via `create_stream_with_start`, `start` may be up to `MAX_START_DELAY_SECONDS` (1 year) in the future; before `now > start`, `pending_accrual` is `0` and `withdraw` reverts with `Error::NothingToWithdraw`.
- **Example:** A donor creates a stream at `t = 1_000` with `start = 2_000`. At `t = 1_500`, `pending_accrual` is `0`, and cancelling refunds 100% of the deposit to the donor.

### Depletion Time
The projected ledger timestamp at which an active stream's remaining `balance` will reach zero at its current `rate`:
```text
anchor = max(last_update, start)
depletion_time = anchor + ceil(balance / rate)
```
- **Example:** If `balance = 1_000`, `rate = 300`, and `last_update = 5_000` (with `start <= 5_000`), `depletion_time` is `5_000 + 4 = 5_004`.

### Drained
The terminal `StreamStatus::Drained` state reached automatically when a withdrawal (or checkpoint settlement) consumes the entire remaining `balance` (`balance == 0`). A drained stream cannot accrue further unless the donor re-opens it via `top_up_and_resume`.
- **Example:** After `100` seconds at `rate = 10` on a `1_000` deposit, calling `withdraw` pays out `1_000` (minus fees), sets `balance = 0`, and transitions `status` from `Active` to `Drained`.

### Cancelled
The terminal `StreamStatus::Cancelled` state (`cancelled = true`, `balance = 0`, `rate = 0`) entered when the donor calls `cancel_stream` or the admin calls `rescue_stream`. Unlike a `Drained` stream, a `Cancelled` stream is permanently closed and cannot be resumed with `top_up_and_resume`.
- **Example:** A donor calls `cancel_stream(stream_id)` mid-stream; the vault settles accrued funds to the NGO, refunds the unaccrued `balance` to the donor, and marks the stream `Cancelled`.

### Rescue (`rescue_stream`)
An emergency admin-only recovery path available **only while the vault is paused**. It refunds 100% of the stream's remaining `balance` directly to the donor without settling or transferring to the NGO (used when an NGO's token trustline is frozen/revoked or its key is compromised, which would otherwise block normal `cancel_stream` settlement).
- **Example:** While the vault is paused, the admin invokes `rescue_stream(stream_id)`; the remaining `balance` is transferred back to `stream.donor` and `status` becomes `Cancelled`.

### Surplus Sweep (`sweep_surplus`)
An admin operation that recovers unallocated tokens sent directly to the `donation-vault` contract address outside of `create_stream` / `top_up`. It transfers at most `token_balance(vault) - TotalLocked(token)`, guaranteeing that active stream deposits (`TotalLocked`) can never be swept.
- **Example:** Someone accidentally transfers `500` XLM directly to the vault address via a standard token transfer; the admin calls `sweep_surplus(token, recipient)` to recover the `500` XLM without affecting any stream's backing balance.

---

## Fees, Governance & Registry Terms

### Protocol Fee (`fee_bps`)
The optional fee charged on settled NGO payouts, expressed in basis points (`1 bps = 0.01%`), capped at `MAX_FEE_BPS = 500` (`5%`). Computed with floor division as `fee = (accrued * fee_bps) / 10_000` and `net = accrued - fee`. Refunds to donors on `cancel_stream` or `rescue_stream` are never charged a fee.
- **Example:** With `fee_bps = 100` (1%) and `accrued = 10_000`, the treasury receives `100` and the NGO receives `net = 9_900`.

### Two-Step Admin Transfer (`propose_admin` / `accept_admin`)
The ownership handover mechanism used in both `donation-vault` and `ngo-registry`. The current admin proposes a `PendingAdmin` address via `propose_admin`, and the proposed address must explicitly sign `accept_admin` before becoming `Admin`. The current admin can cancel a pending proposal via `cancel_admin_transfer`.
- **Example:** Admin `GAAA...` calls `propose_admin(GBBB...)`; until `GBBB...` calls `accept_admin()`, `GAAA...` remains the active admin.

### Upgrade Timelock (`propose_upgrade` / `execute_upgrade`)
A delayed Wasm code upgrade mechanism in `donation-vault`. When a nonzero `upgrade_Delay` is configured, the admin must schedule a `new_wasm_hash` with `propose_upgrade` and wait until `now >= executable_at` before calling `execute_upgrade`, giving donors time to inspect or cancel their streams before new code takes effect.
- **Example:** With a 48-hour upgrade delay, `propose_upgrade(hash)` at `t = 100_000` sets `executable_at = 272_800`; calling `execute_upgrade` early reverts with `Error::UpgradeTimelockActive`.

### Verification Expiry (`ngo-registry`)
An optional timestamp attached to an NGO's approval in `ngo-registry` via `approve_with_expiry(owner, expires_at)`. Once `env.ledger().timestamp() >= expires_at`, `is_verified(owner)` returns `false` automatically without requiring a manual `revoke` transaction.
- **Example:** An approver verifies an NGO for one year via `approve_with_expiry(ngo, now + 31_536_000)`; after that timestamp passes, `donation-vault` rejects new streams to that NGO until re-approved.

---

## Soroban Storage & TTL Terms

### Instance Storage vs. Persistent Storage
Soroban's two durable storage tiers used by the contracts:
- **Instance Storage** holds small singleton configuration values (`Admin`, `Paused`, `FeeBps`, `Treasury`, `Registry`, `MaxRate`, `StreamCount`) that share the contract instance's single TTL entry.
- **Persistent Storage** holds per-record and unbounded entries (`DataKey::Stream(id)`, `DataKey::DonorStreams(donor)`, `DataKey::NgoStreams(ngo)`, `DataKey::TotalLocked(token)`, and NGO records in `ngo-registry`) so contract instance rent stays small and constant regardless of total stream count.
- **Example:** Reading `get_admin()` only loads instance storage, whereas `get_stream(42)` loads only `DataKey::Stream(42)` from persistent storage.

### Grace Ledgers (`TTL_THRESHOLD` / `TTL_BUMP`)
The minimum remaining time-to-live (in ledgers, at ~5 seconds per ledger) below which a touch operation extends a storage entry's lifetime back up to `TTL_BUMP`:
- `TTL_THRESHOLD = 17_280` ledgers (~1 day)
- `TTL_BUMP = 518_400` ledgers (~30 days)
Whenever a stream or contract instance is read/mutated through a TTL-bumping entrypoint (or via `extend_stream_ttl(stream_id)`), Soroban extends its live-until ledger to at least `current_ledger + 518_400` whenever the remaining TTL is at or below `17_280` ledgers.
- **Example:** A long-running stream whose next withdrawal is 45 days away can be kept warm before its remaining TTL drops below `17_280` ledgers by calling `extend_stream_ttl(stream_id)`.
