# Threat model

`donation-vault` custodies donor funds and `ngo-registry` decides which NGOs a
donation may be pointed at, so both contracts are security-critical in a way
that the rest of the system isn't. [ARCHITECTURE.md](./ARCHITECTURE.md) covers
what the pieces do, [STORAGE.md](./STORAGE.md) covers what lives where, and
[EVENTS.md](./EVENTS.md) covers what observers see — but none of them answer
the questions that actually matter before a mainnet deploy: what are we
protecting, who is allowed to break it, what stops them today, and which risks
are knowingly accepted. This document is that answer, in one place.

How to read it:

- **Threats are identified** (`T-…` for `donation-vault`, `R-…` for
  `ngo-registry`, `C-…` for cross-cutting). Reference the ids in issues and
  PRs rather than restating the threat.
- **Each threat lists its mitigation and its residual risk.** "Mitigated" means
  the code enforces something; "accepted" means we know and have chosen not to
  fix it yet.
- **Anything not listed here is open, not safe.** This is a description of the
  system as it is today, not a proof of anything.

This is a threat model, not an audit: it was written from the contracts' source
and docs in this repo, not from a formal review, and no external party has
validated it.

## Scope

| In scope | Notes |
|---|---|
| `contracts/donation-vault` | Stream lifecycle, fee/treasury config, pause, admin transfer, upgrade, TTL policy. |
| `contracts/ngo-registry` | NGO registration, verification, revocation, upgrade, TTL policy. |
| Admin / upgrade trust model | What the admin key can and cannot do in each contract. |
| `scripts/deploy-*.sh`, `deployments.json` | Deployment + initialization path, and the artifact others consume. |

| Out of scope | Why |
|---|---|
| `streamgive-backend` (indexer + REST API) | Separate repo; it can misreport data it has already read, but it holds no keys and signs nothing. |
| `streamgive-frontend` | Separate repo; it never holds a private key, but it *is* in the phishing/impersonation path — see R-2 and C-6. |
| Stellar wallet extensions and Stellar Core / Soroban host | External trust anchors; we assume the signer shows what it signs and that the host executes Soroban correctly. |
| The donation token itself | We assume a well-behaved Stellar Asset Contract; see T-15 and the assumptions section. |
| Off-chain key custody and operator procedure | Where the admin key lives, who can use it, and how deploys are approved are operator concerns; the on-chain consequences are C-1. |

## Assets

| Asset | Lives in | If it's compromised or lost |
|---|---|---|
| Donor deposits held between `create_stream` and the last `withdraw`/`withdraw_batch`/`cancel_stream`/`rescue_stream` | The vault's own token balance | Direct, irreversible loss for donors and NGOs. |
| Stream accounting (`balance`, `withdrawn`, `rate`, `last_update`, `status`, `cancelled`) | `DataKey::Stream(u64)` | Double payment, revived cancelled streams, or a stream that can never be settled. |
| Admin authority (pause, global & per-token fees, treasury, registry pointer, min deposit, stream cap, grace, `rescue_stream`, `renounce_admin`, **upgrade**) | `DataKey::Admin`, `DataKey::AdminRenounced` (instance storage) | Full control of fund movement via `upgrade`, or permanent lockout if renounced while paused; see C-1 and T-24. |
| Per-token fee overrides | `DataKey::TokenFeeBps(Address)` (persistent storage) | Unexpected fallback to global `fee_bps` if archived (T-25). |
| NGO identity (`owner`, `name`, `verified`) | `DataKey::Ngo(Address)` | Donations routed to an attacker-controlled address (R-2). |
| Next stream id / donor stream counts | `DataKey::NextStreamId`, `DataKey::DonorStreamCount(Address)` | Id reuse (streams refuse to open when the counter is missing rather than guessing an id) or an exhausted per-donor cap; see T-11. |
| Published events | Horizon, consumed by `streamgive-backend` | Retroactive rewrites of donation totals in every downstream view. |
| The deployed wasm hash | `deployments.json` + the chain | Whoever controls it controls the logic that custodies funds (C-4). |

## Actors and trust

| Actor | Can do | Trust assumption |
|---|---|---|
| **Donor** | `create_stream`, `top_up`, `modify_rate`, `cancel_stream` on their own streams; keeps the accrued-but-unwithdrawn part of their deposit at risk until they cancel. | Not trusted. Every donor-initiated action is `donor.require_auth()`-gated. A donor can only affect their own streams (T-2). |
| **NGO owner** | `withdraw` on streams pointed at their address. | Not trusted. `withdraw` is `ngo.require_auth()`-gated and can only pay that same address (T-1). |
| **Admin (vault)** | `pause`/`unpause`, `set_fee_bps` (≤ 10%), `set_treasury`/`clear_treasury`, `set_registry`/`clear_registry`, `set_min_deposit`, `set_max_streams_per_donor`, `set_cancel_grace_ledgers`, `propose_admin`, `upgrade`. | **Trusted for liveness and partially for economics; fully trusted via `upgrade`.** Cannot drain stream balances directly, but can rewrite the logic that holds them. See C-1. |
| **Admin (registry)** | `approve_ngo`, `batch_approve`, `reject_ngo`, `revoke_ngo`, `add_approver`, `remove_approver`, `remove_ngo`, `upgrade`. | Trusted for identity. There is no admin-transfer path in `ngo-registry` — losing the key is unrecoverable (C-4). |
| **Approver (registry)** | `approve_ngo`, `batch_approve`, `reject_ngo`, `revoke_ngo` only. | Delegated review authority: trusted for identity to the same extent as the admin *for approval decisions*, but gains none of the admin's other powers (no `upgrade`, `remove_ngo`, or approver-list management). The admin adds and removes approvers; see R-9. |
| **Treasury address** | Receives the fee slice of every payout. | Chosen by the vault admin; receives funds, holds no authority. |
| **Anonymous attacker** | Any unauthenticated call: reads, `extend_stream`, `touch_ngo`, and any entry point whose auth check they can't satisfy. | Assumed to be able to see and react to the mempool of a deployment, and to control arbitrary accounts. |
| **Compromised admin** | Everything the admin can do, at any time, without delay. | The dominant risk in this system; C-1 enumerates the blast radius. |
| **Backend indexer / frontend** | Read all public state and events; display them. | Untrusted for correctness in the sense that they can *misreport*, but cannot move funds or forge signatures. |

## Trust boundaries

```mermaid
flowchart LR
    subgraph Untrusted["Untrusted"]
        DONOR["Donor wallet"]
        NGO["NGO wallet"]
        ATT["Anyone"]
    end

    subgraph Trusted["Trusted / privileged"]
        ADMIN["Admin key(s)"]
        OPS["Deploy + ops scripts"]
    end

    subgraph Onchain["On-chain (Stellar)"]
        DV["donation-vault"]
        NR["ngo-registry"]
        TOK["Donation token (SAC)"]
    end

    subgraph Offchain["Off-chain, re-derives state"]
        HZ["Horizon"]
        BE["streamgive-backend"]
        FE["streamgive-frontend"]
    end

    DONOR -->|"auth: donor"| DV
    NGO -->|"auth: ngo"| DV
    ATT -->|"no auth"| DV
    ATT -->|"no auth"| NR
    ADMIN -->|"auth: admin"| DV
    ADMIN -->|"auth: admin"| NR
    OPS -->|"deploy + init"| DV
    OPS -->|"deploy + init"| NR
    DV -->|"token transfer"| TOK
    DV -->|"try_get_ngo (fail-closed)"| NR
    DV --> HZ
    NR --> HZ
    HZ --> BE
    BE --> FE
```

Two boundaries carry the most weight:

1. **Untrusted → `donation-vault`.** Everything crossing it is individually
   `require_auth()`-gated, and the vault's own accounting is the only thing
   standing between a caller and someone else's deposit.
2. **Admin → `upgrade`.** This boundary is unbounded: an upgrade replaces the
   contract's logic in place while keeping its balance. Nothing in the
   contracts constrains, delays, or detects it (C-1, C-5).

## Security invariants

These are the properties the rest of this document reasons about. Each is
listed with where it is enforced.

1. **Only the NGO named on a stream can trigger a payout from it.**
   `withdraw` → `stream.ngo.require_auth()`; `withdraw_batch` verifies every
   stream in the batch targets the same NGO (`MixedNgo`), rejects duplicate
   ids (`DuplicateStreamId`), caps batch size at `MAX_BATCH_WITHDRAW_STREAMS`
   (20), and requires that NGO's auth.
2. **Only the donor named on a stream can add funds, change its rate, transfer
   it, or cancel it.** `top_up`, `modify_rate`, `transfer_stream`,
   `cancel_stream` → `stream.donor.require_auth()`.
3. **A payout can never exceed what was deposited on that stream.** `settle`,
   `withdraw`, and `withdraw_batch` go through `math::accrued`, which caps at
   `balance`; `record_payout` uses `checked_sub`/`checked_add` and returns
   `ArithmeticOverflow` instead of wrapping.
4. **A cancelled or rescued stream can never be revived.** `cancel_stream` and
   `rescue_stream` set `cancelled` permanently; `top_up`, `modify_rate`,
   `transfer_stream`, `cancel_stream`, and `rescue_stream` reject with
   `StreamCancelled`.
5. **The protocol fee can never exceed 10%.** Both `set_fee_bps` and
   `set_token_fee_bps` enforce `MAX_FEE_BPS = 1_000` and reject non-zero fees
   when no treasury is configured (`FeeRequiresTreasury`); `clear_token_fee_bps`
   restores fallback to the global `fee_bps`, and `compute_fee` returns 0
   whenever no treasury is set.
6. **While paused, no ordinary fund-moving or stream-mutating call succeeds**
   (`create_stream`, `withdraw`, `withdraw_batch`, `top_up`, `modify_rate`,
   `transfer_stream` all reject with `ContractPaused`) — except
   `cancel_stream`, which stays open so donors can always exit, and
   `rescue_stream` (admin-only), which *only* succeeds while paused
   (`NotPaused` when active) and refunds the stream's full remaining balance
   to the donor.
7. **A stream is only opened, topped up, or transferred toward a verified NGO
   *if* a registry is configured.** `create_stream`, `top_up`, and
   `transfer_stream` call `require_verified_ngo`; with `registry() == None`,
   any distinct address is accepted (T-19, T-20, T-21).
8. **A donor cannot be their own NGO.** `create_stream` and `transfer_stream`
   reject `donor == ngo` with `SelfStream`, so a stream cannot be counted as a
   committed donation while paying the donor back.
9. **Accrual is monotonic and never negative.** `math::accrued` returns 0 for
   non-positive rate/balance or zero elapsed, `saturating_mul`s, and is covered
   by a property-test grid.

## Threats: `donation-vault`

### Authorization and accounting

| ID | Threat | Mitigation | Residual risk |
|---|---|---|---|
| T-1 | A third party calls `withdraw` or `withdraw_batch` on someone else's stream and takes the payout. | `withdraw` and `withdraw_batch` → `stream.ngo.require_auth()`, checked against the address stored *on the stream*, not an argument (`withdraw_batch` also rejects mixed NGOs with `MixedNgo`). | None known. |
| T-2 | A third party cancels, tops up, re-rates, or transfers another donor's stream. | `cancel_stream`, `top_up`, `modify_rate`, `transfer_stream` → `stream.donor.require_auth()`, again against the stored donor. | None known. |
| T-3 | Double payout: two `withdraw`/`withdraw_batch` calls in one ledger, or `top_up`/`modify_rate`/`transfer_stream`/`cancel_stream` racing a `withdraw`, paying the same accrual twice. | `cancel_stream`, `top_up`, `modify_rate`, and `transfer_stream` share `settle`, while `withdraw` and `withdraw_batch` run their own payout loops using the same `math::accrued` + `record_payout` accounting and advancing `last_update` (and persisting before transferring); a second `withdraw`/`withdraw_batch` in the same ledger accrues 0 and returns `NothingToWithdraw`. | **Low / accepted:** accrual is per-second and truncated toward zero, so a sub-1-per-second stream under-pays by dust rather than over-paying. |
| T-4 | Integer overflow or wrap on stream balances, `withdrawn`, or the stream-id counter. | `record_payout`/`create_stream`/`withdraw_batch` use `checked_add`/`checked_sub` → `ArithmeticOverflow`; the release profile keeps `overflow-checks = true` (see README: Release profile) so a missed checked op aborts instead of wrapping. | Low: `math::accrued` deliberately *saturates* on `rate × elapsed` rather than erroring, and the result is then capped by `balance`. |
| T-5 | A cancelled or rescued stream is revived, or a `cancel` is replayed to inflate indexes. | `cancel_stream` and `rescue_stream` set `cancelled` permanently; `top_up`/`modify_rate`/`transfer_stream`/`cancel_stream`/`rescue_stream` reject it with `StreamCancelled`; the `cancel` event is only published when something actually moved. | None known. |
| T-6 | Donation-metric inflation through a stream that pays the donor back. | `create_stream` and `transfer_stream` reject `donor == ngo` (`SelfStream`) before any transfer. | Low: a donor can still stream to a second address they control (wash donation). No on-chain defense exists; this is an off-chain reporting concern. |
| T-7 | The fee silently takes up to 10% of a payout the NGO believed was gross. | `set_fee_bps` and `set_token_fee_bps` cap at `MAX_FEE_BPS` and require a treasury when non-zero; `withdraw` returns the net and emits `(accrued, fee, net)` so the exact split is visible on-chain; `pending_payout` previews `(net, fee)`. | **Medium / accepted:** the admin can raise the global or per-token fee at any time with no delay; implicit settlements in `topup`/`ratemod`/`transfer` report gross `settled` rather than `(accrued, fee, net)` (see C-7). |
| T-8 | The fee slice is redirected to an attacker-controlled treasury right before a large withdrawal. | `set_treasury` is admin-only and emits `treasset`; `clear_treasury` (emits `treclr`) reverts to paying the NGO in full once the global fee is 0. | Medium / accepted: exposure is bounded to the ≤ 10% fee; principal and stream balances cannot be redirected. |
| T-9 | Accrual is manipulated through the ledger timestamp. | Accrual reads `env.ledger().timestamp()`; neither donor nor NGO supplies it. | Low / accepted: bounded by network time and consensus rules; the contracts cannot do better than the ledger clock. |
| T-10 | Donor→NGO relationships, amounts, and rates are exposed. | None — inherent: `get_stream`, `get_streams`, `stream_count`, `pending_accrual`, `pending_payout`, `pending_refund` are public reads and every event is public. | Accepted: there is no privacy expectation on-chain. Frontends and docs must not imply otherwise. |

### Availability, storage economics, and lifecycle

| ID | Threat | Mitigation | Residual risk |
|---|---|---|---|
| T-11 | Stream flooding to bloat persistent storage and make others pay rent. | Per-donor cap (`DEFAULT_MAX_STREAMS_PER_DONOR = 100`, `set_max_streams_per_donor`), read *before* the token transfer so a rejected donor moves no funds. | **Medium:** the cap is per address, so funded accounts multiply it, and `DonorStreamCount` is never decremented — a legitimate heavy user who cancels a lot is eventually locked out of `create_stream`. `min_deposit` defaults to 0, so there is no dust floor until an admin sets one. |
| T-12 | Indefinite freeze of NGO payouts. | `pause`/`unpause` are admin-only and emit events; `cancel_stream` deliberately keeps working while paused so donors can always exit, and `rescue_stream` provides an admin-only emergency refund while paused. | **Medium / accepted:** no on-chain pause expiry, so a compromised key can freeze withdrawals indefinitely (and calling `renounce_admin` while paused makes the pause permanent; see T-24). Monitor `pause` and treat an unexplained pause as an incident. |
| T-13 | Anyone keeps storage alive (or a cancelled record visible) at someone else's expense. | `extend_stream` and `touch_ngo` are permissionless by design and only extend TTLs; the caller pays. | Low / intentional: this is the documented escape hatch against archival (STORAGE.md). |
| T-14 | A dormant stream is archived and its funds look unreachable. | 90-day stream TTL refreshed on every touch; `extend_stream` lets anyone keep a stream alive. | **Medium:** archival makes `get_stream`/`withdraw`/`cancel_stream` behave as `StreamNotFound` for that id until an explicit restore is paid for. Nothing is lost, but recovery needs tooling and an extra fee, and nothing obliges either party to keep a dormant stream alive. |
| T-15 | Reentrancy through a hostile `token` address. | Every value-moving path is auth-gated on the stored donor/NGO, so a hostile token cannot manufacture the auth those paths need; Soroban rolls back the whole transaction if any inner call traps. | **Low–Medium:** there is no reentrancy guard, `create_stream` accepts any token address, and in `cancel_stream` the NGO payout happens inside `settle` *before* the stream is written back. A guard or a token allowlist would remove the class. |
| T-16 | A token that doesn't deliver exactly what was sent (fee-on-transfer, rebasing, non-SAC) breaks the vault's accounting. | The vault tracks each stream's `balance` itself and assumes its real token balance covers the sum of all stream balances. | **Medium / accepted:** with a misbehaving token the vault can end up unable to pay the final withdrawer. Donations should use a plain Stellar Asset Contract; see Assumptions and [Supported tokens](./ARCHITECTURE.md#supported-tokens) for the behavior-by-behavior list. |
| T-17 | Donor funds stall because a payout destination cannot receive the token. `cancel_stream` settles to the NGO *before* refunding the donor, so a trapped NGO payout reverts the whole cancellation and the donor cannot recover the remainder — only the NGO can withdraw. | Registry verification plus UI-side address validation are the only guards. | **High for the affected stream and borne by the donor:** covers typo'd or otherwise unusable NGO addresses when no registry is configured (T-21), and asset trustline/blocklist failures. A code-level fix would make the NGO payout at cancel time best-effort. |
| T-18 | `StreamStatus` is not reset when a Drained stream is topped up again. | None: `withdraw` sets `Drained`, `cancel_stream` sets `Cancelled`, and no path ever sets `Active` after creation. | **Medium (correctness):** after `top_up`, `status` reads `Drained` while `rate` and `balance` are non-zero, so a client trusting `status` alone misreads a resumed stream. Fix by setting `Active` in `top_up`, or derive status from `balance`/`cancelled`. |
| T-19 | A bad registry pointer blocks *all* new streams, not just unverified NGOs. | `create_stream` maps every registry failure (wrong address, not a contract, archived entry, error) to "NGO not verified" — fail-closed, which is the safe default. | **Medium (mitigated):** fail-closed makes a misconfigured or archived registry a global outage for `create_stream`. Mitigated by `clear_registry`, which allows the admin to immediately unset a bad pointer and unblock stream creation; what remains is that clearing the registry switches off NGO verification for new streams until a correct registry is configured via `set_registry`. |
| T-20 | A revoked or unregistered NGO keeps receiving top-ups, transfers, or rate increases on live streams. | Registry verification is re-checked on `create_stream`, `top_up`, `transfer_stream`, and rate increases in `modify_rate` via `require_verified_ngo`. A revoked NGO cannot receive new streams, additional top-up funds, or a faster rate (`Error::NgoNotVerified`); a donor can still lower the rate. | **Accepted for existing stream accruals:** already-deposited funds continue accruing to the NGO until the donor calls `cancel_stream` or the stream drains. Donors stay in control and can cancel anytime without trapping funds. |
| T-21 | With no registry configured, a donation can be pointed at any address. | `registry()` returns `None` until an admin calls `set_registry`; with no pointer the verification branch is skipped and any `ngo != donor` is accepted. | **High if forgotten in production:** a fresh deploy accepts any NGO. Treat `set_registry` as a mandatory mainnet deploy step and alert when `registry()` is `None`. |

## Threats: `ngo-registry`

| ID | Threat | Mitigation | Residual risk |
|---|---|---|---|
| R-1 | An attacker marks themselves verified, or un-verifies a real NGO. | `approve_ngo`/`revoke_ngo` go through `require_admin`, which reads `DataKey::Admin` and calls `admin.require_auth()`. `revoke_ngo` additionally requires the entry to currently be verified. | None known. |
| R-2 | NGO impersonation: an attacker registers an address with the name of a real charity and collects donations. | `register` and `update_name` validate names: bounded at `MAX_NGO_NAME_LEN = 200` bytes, valid UTF-8, must contain at least one visible character (rejects empty names, Unicode whitespace, and zero-width/invisible format code points), and rejects control characters (`Error::InvalidName`). Names are **not unique** or reserved on-chain. Only the `verified` flag distinguishes a real NGO, set by an admin after off-chain review. | **High without a solid review process and a frontend that always renders `verified` plus the full address.** Bidirectional override characters are not filtered on-chain and remain a client-side spoofing risk. |
| R-3 | Bait-and-switch: an NGO renames itself *after* approval to something misleading. | `update_name` is owner-gated and blocked once `verified`; `unregister` is likewise blocked for verified entries, so a verified name is frozen. | Low: the admin approves a name that then cannot change. An admin who approves before a rename finishes is an operator mistake, not a code flaw. |
| R-4 | Verification is bypassed through `upgrade` or a compromised admin key. | `upgrade` is admin-gated (`update_current_contract_wasm`). | **High by design:** a compromised registry admin can mark arbitrary addresses verified — and the vault will then happily stream to them (C-1, C-8). |
| R-5 | `ngo_count()` / `total_ngos()` read as "number of live NGOs". | `register` increments `NgoCount` using checked addition, and both `unregister` and `remove_ngo` decrement it using checked subtraction (`Error::ArithmeticOverflow` on underflow/overflow). `total_ngos()` is a deprecated alias reading the same single stored counter (`NgoCount`), so both getters consistently report live registered NGOs. | None known. |
| R-6 | An archived NGO entry silently blocks new donations to that NGO. | 90-day entry TTL; `touch_ngo` is permissionless so anyone (including the backend) can keep an entry alive. | **Medium (liveness):** an archived entry makes the vault's lookup fail, and `create_stream` maps that to "not verified" (fail-closed) — so a real, still-verified NGO stops receiving *new* streams until someone pays to restore its entry. Existing streams are unaffected. |
| R-7 | The registry admin key is lost. | None: unlike the vault, `ngo-registry` has no `propose_admin`/`accept_admin` path (the mainnet deploy script documents this), so the key cannot be rotated on-chain. | **Medium–High operationally:** losing the key means no NGO can ever be approved or revoked again, and no rotation is possible. Use a multisig/backed-up key now; an admin-transfer path is the durable fix. |
| R-8 | Hostile content in `Ngo.name` is rendered by clients. | The contract stores an arbitrary up-to-200-byte `String` and publishes it in `register`/`renamed` events. | Low on-chain, **frontend-owned:** sanitization and display rules belong to the clients that render it; the registry will faithfully store and serve whatever an owner wrote. |
| R-9 | Delegating review to an approver widens the identity-trust boundary. | `add_approver`/`remove_approver` are admin-gated (`require_admin`), the list lives under `DataKey::Approvers`, and `require_approver` checks membership (or adminship) before `approver.require_auth()`. An approver may only call `approve_ngo`/`batch_approve`/`revoke_ngo`; `upgrade`, `remove_ngo`, and approver-list management stay admin-only. | **Accepted / bounded:** a rogue approver can verify arbitrary addresses — the same approval blast radius as a rogue admin (C-1) — and revoke real ones, but cannot upgrade code, delete entries, or manage the list. Keep the list small, review it, and monitor `apadd`/`aprem`. |

## Threats: cross-cutting (privilege, deployment, off-chain)

| ID | Threat | Mitigation | Residual risk |
|---|---|---|---|
| C-1 | Admin key compromise (either contract). | Bounded by design: no admin entry point pays out a stream balance, the fee is capped at 10% and only charged when a treasury is set, and `cancel_stream` keeps working while paused so donors can always exit. `propose_admin`/`accept_admin` make rotation two-step, and every admin action emits an event. | **High by design.** A compromised vault admin can pause (freezing payouts and creation), raise the fee to 10% on all future accruals, redirect the fee slice to itself, point the registry at a contract that verifies anything, raise `min_deposit` or zero `max_streams_per_donor` to block all new streams and — decisively — `upgrade` the contract to logic that drains the vault's token balance. A compromised registry admin can verify arbitrary addresses, revoke real ones, and delete unverified entries. Required controls: multisig (or an offline key) for both admins, monitoring on `propadmin`/`acptadmin`/`feeset`/`treasury`/`pause`/`maxstrm`/`init`/`upgrade`/`treclr`/`mindep`/`grceset`/`regset`/`regclr`, and a documented upgrade process (a timelock on `upgrade` would help most). |
| C-2 | A mistyped or unwanted admin transfer locks the vault's admin role. | `propose_admin` rejects the current admin (`InvalidAdmin`); `accept_admin` requires the *pending* address's auth, so a proposer cannot complete a transfer to someone who can't sign; `cancel_admin_proposal` withdraws a pending proposal. | Low: rotation is hygiene, not recovery — a compromised admin can propose and accept a key it controls (C-1). |
| C-3 | Anyone initializes a freshly deployed contract, becoming its admin. | `init(admin)` in **both** contracts takes the admin as an argument and requires **no auth**; it is once-only (`AlreadyInitialized`). Both deploy scripts call `init --admin` immediately after each deploy. | **High if deploy and init aren't effectively atomic:** first-caller-wins means an uninitialized deployment can be claimed by anyone, and with `upgrade` that is a full takeover. Deploy and initialize in one sitting, then verify `admin()` equals the intended address (the scripts record it in `deployments.json`). |
| C-4 | Deploy/artifact integrity: the wrong code or the wrong admin goes live. | `deploy-mainnet.sh` requires `--confirm`, pins the Public network passphrase, requires an explicit `G…` admin address, refuses to overwrite an existing entry, and records the contract ids under `mainnet` in `deployments.json`; CI runs `fmt`, `clippy -D warnings`, `cargo doc -D warnings`, a `wasm32v1-none` release build, a wasm-size check, and the test suite. | **Medium:** nothing verifies that the deployed wasm corresponds to a reviewed commit (no reproducible-build check, no source verification), and `deployments.json` is a plain file consumers must not trust blindly — confirm `admin()`, `registry()`, `fee_bps()`, `treasury()`, and `paused()` on-chain before using them. |
| C-5 | Admin configuration changes are invisible to event-only monitoring. | Every admin-gated configuration change now publishes a typed event: `upgrade` in **both** contracts, the registry's approver-list management (`add_approver` → `apadd`, `remove_approver` → `aprem`), the vault setters `set_min_deposit` (`mindep`), `set_registry` (`regset`), `clear_registry` (`regclr`), `set_cancel_grace_ledgers` (`grceset`), `clear_treasury` (`treclr`), and `init`/the constructor in both contracts (`init`), matching the existing `feeset`/`treasset`/`maxstrm` pattern. | **Low:** a detector built from events alone can now see every code and configuration change, but only *after* the transaction lands — there is no timelock, so this is detection, not prevention, and a compromised key can still `upgrade` and drain in a single ledger before anyone reacts. Keep polling `admin()`, the config getters, and the deployed wasm hash as a backstop, and consider a timelock (see C-1). |
| C-6 | Phishing: a fake client builds a `create_stream` toward an attacker's address while displaying the real NGO's name. | None on-chain — the wallet shows what it is asked to sign, and the contracts cannot know which frontend built the transaction. | **Human-factor risk, frontend-owned:** show the destination address and its `verified` state at signing time, keep an allow-listed origin, and make the contract ids easy to verify. |
| C-7 | Indexers derive wrong donation/payout totals from events. | `withdraw` (and `withdraw_batch`) events now carry `(accrued, fee, net)` directly, and `topup`, `ratemod`, and `transfer` include the gross `settled` amount in their payloads; `cancel` events carry `(accrued, refund)` and are suppressed when nothing moved. | **Low–Medium (off-chain correctness):** implicit settlements in `topup`, `ratemod`, `transfer`, and `cancel` report gross `settled`/`accrued` without the fee/net breakdown, so indexers deriving net NGO payouts for those paths must still combine them with the effective fee (`token_fee_bps`/`fee_bps`) and `treasury()` at transaction time (EVENTS.md documents each shape). |
| C-8 | The vault's dependency on `ngo-registry`'s interface. | The vault calls `try_get_ngo` and maps any failure to "not verified" (fail-closed), so an interface break blocks new streams rather than admitting unverified ones. | **Medium / accepted:** a registry upgrade that renames or removes that entry point breaks verification for all new streams, top-ups, and transfers, and a malicious registry can verify anything. Both admins are, today, the same operator — a trust concentration rather than an interface race. |

### Why the admin boundary dominates

Every other threat here is a bounded failure: a bad global or per-token fee
costs at most 10% of future accruals, a bad treasury only captures that fee, a
bad pause freezes liquidity but leaves `cancel_stream` open (while
`rescue_stream` only refunds `stream.donor`), and none of them can move a
stream's principal to an attacker. `upgrade` is different in kind — it replaces
the code that holds the funds while keeping the balance, and nothing on-chain
delays, limits, or detects it. So "how bad is a compromised admin?" is really
"as bad as the worst code they can deploy".

The cheapest real improvements, in order: put both admin keys behind a
multisig; monitor `propadmin`/`acptadmin`/`canceladm`/`renounce` alongside
`feeset`/`tokfeeset`/`tokfeeclr`/`treasset`/`treclr`/`pause`/`unpause`/`rescue`/`maxstrm`
and `init`/`upgrade`/`mindep`/`grceset`/`regset`/`regclr`; and add a timelock
to `upgrade` so a code change can be reviewed before it takes effect.

### Why stalled funds are the most likely user-visible loss

A stranger cannot take a donor's principal — but the contracts can make it
*unreachable*. Two paths do that: a stream archived after 90 idle days (T-14)
and a cancellation that can never complete because the NGO payout traps (T-17).
Both are recoverable in principle and neither is an accounting bug, but both
need explicit tooling and someone willing to pay, and T-17 leaves the donor's
remainder behind an NGO that may not exist. Keeping a registry configured
(T-21) and validating the destination at creation time are the real defenses
today.

## Residual risk register

Open items this model surfaces, roughly ordered by value. Each maps to the
threats above; most of them are not fixed today.

| # | Item | Threats |
|---|---|---|
| 1 | Put both admin keys behind a multisig and document an upgrade procedure (who approves, how the wasm hash is reviewed, what monitoring runs during the window). | C-1, R-4, R-7 |
| 2 | Wire admin code/configuration events (`init`, `upgrade`, `propadmin`, `acptadmin`, `canceladm`, `renounce`, `feeset`, `tokfeeset`, `tokfeeclr`, `treasset`, `treclr`, `mindep`, `grceset`, `regset`, `regclr`, `pause`, `unpause`, `rescue`, `maxstrm`) into alerting, and add a timelock on `upgrade` so detection becomes prevention. | C-1, C-5 |
| 3 | Verify `admin()` (both contracts) and `registry()` (vault) after every mainnet deploy, and ensure `STELLAR_REGISTRY_ADDRESS` is configured rather than opting out with `--allow-no-registry`. | C-3, T-21, R-6 |
| 4 | Decide and document the behavior when an NGO's payout destination can't receive the token: make the cancel-time payout best-effort, or reject the destination at creation time. | T-17, T-21, T-26 |
| 5 | Set a non-zero `min_deposit` in production and revisit whether `DonorStreamCount` should decrement on cancellation (it is currently a lifetime cap that can lock a heavy user out). | T-11 |
| 6 | Empty, whitespace-only, invisible-format-only, and control-character NGO names are now rejected in `register` and `update_name`; what remains is filtering bidirectional override characters if desired, documenting that names are neither unique nor reserved, and driving every client display off `verified` plus the full address. | R-2, C-6 |
| 7 | `clear_registry` now allows unsetting a bad registry pointer; what remains is documenting and alerting on the consequence that clearing disables NGO verification until repointed via `set_registry`. | T-19, T-21 |
| 8 | Add a reentrancy guard and/or an allowlist of donation tokens, persist stream state before transferring in `rescue_stream`, and persist before `settle`'s transfer so the token address is no longer an unbounded reentrancy surface. | T-15, T-16, T-23 |
| 9 | Guard `renounce_admin` against being called while the vault is paused (or document strict operator pre-checks), so the vault cannot be permanently frozen. | T-12, T-24 |
| 10 | Refresh `DataKey::TokenFeeBps(token)` TTL on payouts (or monitor its 30-day TTL) so per-token fee overrides cannot silently archive and fall back to global `fee_bps`, and decide whether `clear_treasury` should also reject when per-token fees are non-zero. | T-25 |
| 11 | Verify the deployed wasm against a reviewed commit (reproducible build + source verification) instead of trusting `deployments.json`. | C-4 |
| 12 | Keep indexer docs and the README error table in sync with the contracts (implicit settlement fee splits in `topup`/`ratemod`/`transfer`/`cancel`, and error codes 24–25). | C-7 |

## Assumptions this model relies on

If any of these stops holding, the corresponding threats get worse and this
document needs revisiting:

- **The Soroban host and Stellar consensus behave as specified**, including
  transaction atomicity: a failing inner call reverts every state change in the
  transaction. Several mitigations above (T-15, T-16, T-17) lean on that.
- **The donation token is a well-behaved Stellar Asset Contract** that moves
  exactly the requested amount and reverts instead of returning a failure
  signal. Nothing in the contracts validates the token (T-16). See
  [Supported tokens](./ARCHITECTURE.md#supported-tokens).
- **Auth is meaningful**: `require_auth()` reflects an actual signature or an
  authorized contract call by the address named, and wallets show the user what
  they are signing (C-6).
- **Ledger timestamps advance monotonically and are not attacker-chosen.** All
  accrual is a function of them (T-9).
- **The admin keys are held by the operator** and are not attacker-controlled
  in the normal case. C-1 covers what happens when that fails.
- **Off-chain components** (indexer, frontend) are not part of this system's
  security boundary for fund movement; they cannot sign, and their failures are
  reporting failures (C-6, C-7).

## Maintenance

Update this document in the same PR that changes the thing it describes. It is
stale the moment any of the following happens without a matching edit here:

- a new or changed entry point, error variant, or `DataKey`;
- a new admin lever, or a change to what an existing one can do;
- a change to storage placement, TTL policy, or archival behavior;
- a new or changed published event (the interface indexers rely on);
- a change to the admin model (multisig, timelock, added admin-transfer path);
- a mainnet deploy or a change to the deploy scripts.

Reference threat ids (`T-…`, `R-…`, `C-…`) in issues and pull requests instead
of restating a threat, and treat anything not listed here as open.

## References

- [README](../README.md) — entry points, pausing, error codes, release profile.
- [docs/ARCHITECTURE.md](./ARCHITECTURE.md) — system overview and data flow.
- [docs/STORAGE.md](./STORAGE.md) — storage keys, TTLs, archival risk.
- [docs/EVENTS.md](./EVENTS.md) — event topics and data for every state change.
- [contracts/donation-vault/src/lib.rs](../contracts/donation-vault/src/lib.rs),
  [contracts/donation-vault/src/math.rs](../contracts/donation-vault/src/math.rs) —
  vault logic, accrual math, and the property tests behind invariant 9.
- [contracts/ngo-registry/src/lib.rs](../contracts/ngo-registry/src/lib.rs) — registry logic.
- [scripts/](../scripts) and [.github/workflows/ci.yml](../.github/workflows/ci.yml) —
  deploy and CI paths referenced by C-4.
