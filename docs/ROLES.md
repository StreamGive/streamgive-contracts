# Roles and access control

Who can call each public entry point of `donation-vault` and `ngo-registry`.
The same rules are written in each function's doc comment and enforced in
`lib.rs`; this page puts them in one place.

The tables are checked by tests. `roles_doc_lists_every_entry_point` in each
contract's `src/test.rs` fails if a public entry point has no row here, or if a
row names a function that no longer exists. When you add, rename, or remove an
entry point, update the table in the same PR.

## Roles

| Role | Who it is | How it is checked |
| --- | --- | --- |
| **Admin** | The address stored as the contract's admin. Set at deploy, changed only through `propose_admin` / `accept_admin`, and gone for good after `renounce_admin` (vault only). | `require_admin`: the stored admin must sign. Fails with `NotInitialized` if no admin is stored and, in the vault, `AdminRenounced` after a renounce. |
| **Admin-to-be** | The `admin` argument of the legacy `init`, before any admin is stored. | That address must sign, so nobody can initialize a contract to an admin they don't control. |
| **Pending admin** | The address named by the last `propose_admin`. | The pending address must sign `accept_admin`. |
| **Approver** (registry) | The admin, or an address the admin added with `add_approver`. | `require_approver`: the `approver` argument must sign and be the admin or on the approver list, otherwise `NotApprover`. |
| **Donor** | The address that funded a stream (`stream.donor`), or the `donor` argument when creating one. | That address must sign. |
| **NGO** | The stream's recipient (`stream.ngo`), or the `ngo` argument of the vault's NGO-scoped calls. In the registry, the `owner` of an NGO entry. | That address must sign. The NGO's payout address (`set_payout_address`) receives funds but cannot authorize anything. |
| **Deployer** | Whoever deploys the contract. | `__constructor` runs once, inside the deploy transaction, and cannot be called again. |
| **Anyone** | Any caller. | No `require_auth`. Views only read state. `extend_stream` and `touch_ngo` only extend TTLs. |

There is no **treasury** role. The treasury address receives protocol fees and
`sweep_surplus` transfers, but no entry point requires its signature. Only the
admin can set or clear it (`set_treasury`, `clear_treasury`).

Being allowed to call a function doesn't mean the call will succeed. Pause
state, stream status, and limits still apply. See the [README](../README.md#pausing)
for what works while the vault is paused, and the
[error tables](../README.md#error-codes) for every failure.

## `donation-vault`

<!-- roles:donation-vault:start -->

| Function | Role | Notes |
| --- | --- | --- |
| `__constructor` | Deployer | Stores `admin` at deploy time. |
| `init` | Admin-to-be | Legacy initializer for a vault deployed without a constructor. The `admin` argument must sign. Fails if an admin already exists or was renounced. |
| `renounce_admin` | Admin | Permanent. Clears the active and pending admin. |
| `propose_admin` | Admin | Step 1 of the admin handshake. |
| `accept_admin` | Pending admin | Step 2 of the admin handshake. |
| `cancel_admin_proposal` | Admin | |
| `pause` | Admin | |
| `unpause` | Admin | |
| `set_treasury` | Admin | |
| `clear_treasury` | Admin | |
| `set_fee_bps` | Admin | |
| `set_token_fee_bps` | Admin | |
| `clear_token_fee_bps` | Admin | |
| `set_min_deposit` | Admin | |
| `set_cancel_grace_ledgers` | Admin | |
| `set_max_streams_per_donor` | Admin | |
| `set_max_rate` | Admin | |
| `clear_max_rate` | Admin | |
| `set_max_deposit_per_stream` | Admin | |
| `clear_max_deposit_per_stream` | Admin | |
| `set_min_duration` | Admin | |
| `set_registry` | Admin | |
| `clear_registry` | Admin | |
| `rescue_stream` | Admin | Only while the vault is paused. |
| `sweep_surplus` | Admin | Sends to the configured treasury. |
| `sweep_surplus_to` | Admin | Sends to an explicit destination; no treasury needed. |
| `upgrade` | Admin | Only while `upgrade_delay` is zero. |
| `set_upgrade_delay` | Admin | |
| `propose_upgrade` | Admin | |
| `execute_upgrade` | Admin | After `upgrade_delay` has passed. |
| `cancel_upgrade` | Admin | |
| `create_stream` | Donor | The `donor` argument signs and pays the deposit. |
| `create_stream_with_start` | Donor | As `create_stream`. |
| `create_stream_with_memo` | Donor | As `create_stream`. |
| `set_memo` | Donor | The stream's donor. Rejected on a cancelled stream. |
| `top_up` | Donor | The stream's donor. |
| `modify_rate` | Donor | The stream's donor. |
| `transfer_stream` | Donor | The stream's donor. |
| `pause_stream` | Donor | The stream's donor. |
| `resume_stream` | Donor | The stream's donor. |
| `cancel_stream` | Donor | The stream's donor. Works while the vault is paused. |
| `cancel_streams` | Donor | Every listed stream must belong to the same donor, which signs once. All-or-nothing. Works while the vault is paused. |
| `withdraw` | NGO | The stream's NGO. |
| `withdraw_batch` | NGO | All streams must share one NGO, which signs. |
| `withdraw_all` | NGO | The `ngo` argument signs, once the page contains a stream. An empty page needs no signature and moves nothing. |
| `set_payout_address` | NGO | The `ngo` argument signs. |
| `extend_stream` | Anyone | TTL bump only. |
| `total_donated` | Anyone | View. |
| `total_withdrawn` | Anyone | View. |
| `total_fees_collected` | Anyone | View. |
| `admin` | Anyone | View. |
| `pending_admin` | Anyone | View. |
| `get_stream` | Anyone | View. |
| `get_streams` | Anyone | View. |
| `streams_by_donor` | Anyone | View. |
| `donor_totals` | Anyone | View. |
| `streams_by_ngo` | Anyone | View. |
| `active_streams_by_ngo` | Anyone | View. Pages only `Active` and `Paused` streams. |
| `stream_count` | Anyone | View. |
| `pending_accrual` | Anyone | View. |
| `depletion_time` | Anyone | View. |
| `preview_stream` | Anyone | View. Estimates duration, token fee and validation errors before creation. |
| `pending_payout` | Anyone | View. |
| `pending_refund` | Anyone | View. |
| `preview_stream` | Anyone | View. Estimates duration, token fee and validation errors before creation. |
| `paused` | Anyone | View. |
| `get_config` | Anyone | View. |
| `treasury` | Anyone | View. |
| `fee_bps` | Anyone | View. |
| `max_fee_bps` | Anyone | View. |
| `token_fee_bps` | Anyone | View. |
| `allowed_tokens` | Anyone | View. |
| `min_deposit` | Anyone | View. |
| `cancel_grace_ledgers` | Anyone | View. |
| `max_streams_per_donor` | Anyone | View. |
| `max_rate` | Anyone | View. |
| `max_deposit_per_stream` | Anyone | View. |
| `min_duration` | Anyone | View. |
| `registry` | Anyone | View. |
| `payout_address` | Anyone | View. |
| `upgrade_delay` | Anyone | View. |
| `pending_upgrade` | Anyone | View. |
| `committed_balance` | Anyone | View. |
| `surplus` | Anyone | View. |

<!-- roles:donation-vault:end -->

## `ngo-registry`

<!-- roles:ngo-registry:start -->

| Function | Role | Notes |
| --- | --- | --- |
| `__constructor` | Deployer | Stores `admin` at deploy time. |
| `init` | Admin-to-be | Legacy initializer. The `admin` argument must sign. Fails if an admin already exists. |
| `propose_admin` | Admin | Step 1 of the admin handshake. |
| `accept_admin` | Pending admin | Step 2 of the admin handshake. |
| `cancel_admin_proposal` | Admin | |
| `add_approver` | Admin | |
| `remove_approver` | Admin | |
| `remove_ngo` | Admin | Unverified entries only. |
| `set_verify_period` | Admin | |
| `renew_ngo` | Admin | |
| `pause_registrations` | Admin | Prevents `register` from accepting new applications. |
| `resume_registrations` | Admin | Re-opens `register` after a pause. |
| `upgrade` | Admin | |
| `set_upgrade_delay` | Admin | |
| `propose_upgrade` | Admin | |
| `execute_upgrade` | Admin | After `upgrade_delay` has passed. |
| `cancel_upgrade` | Admin | |
| `approve_ngo` | Approver | The `approver` argument signs. |
| `batch_approve` | Approver | The `approver` argument signs. |
| `reject_ngo` | Approver | The `approver` argument signs. Unverified entries only. |
| `revoke_ngo` | Approver | The `approver` argument signs. |
| `register` | NGO | The `owner` argument signs. |
| `unregister` | NGO | The `owner` argument signs. Unverified entries only. |
| `update_name` | NGO | The `owner` argument signs. Unverified entries only. |
| `set_metadata_uri` | NGO | The `owner` argument signs. |
| `set_categories` | NGO | The `owner` argument signs. Unverified entries only. |
| `propose_owner_transfer` | NGO | The `current_owner` argument signs. Step 1 of an owner transfer. |
| `cancel_owner_transfer` | NGO | The `owner` argument signs. |
| `accept_owner_transfer` | Pending NGO owner | The `new_owner` argument signs. Step 2 of an owner transfer. |
| `touch_ngo` | Anyone | TTL bump only. |
| `admin` | Anyone | View. |
| `pending_admin` | Anyone | View. |
| `total_ngos` | Anyone | View. |
| `is_approver` | Anyone | View. |
| `list_approvers` | Anyone | View. |
| `list_ngos` | Anyone | View. |
| `list_ngos_page` | Anyone | View. |
| `metadata_uri` | Anyone | View. |
| `owner_by_name` | Anyone | View. |
| `categories` | Anyone | View. |
| `get_ngo` | Anyone | View. |
| `is_verified` | Anyone | View. |
| `ngo_count` | Anyone | View. |
| `verified_count` | Anyone | View. |
| `verify_period` | Anyone | View. |
| `registrations_paused` | Anyone | View. |
| `verification_status` | Anyone | View. |
| `upgrade_delay` | Anyone | View. |
| `pending_upgrade` | Anyone | View. |

<!-- roles:ngo-registry:end -->
