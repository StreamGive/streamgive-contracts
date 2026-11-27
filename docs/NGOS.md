# A guide for NGOs using StreamGive

This guide is for the people who run an NGO and want to receive donations
through StreamGive. It covers how to register, what verification means and how
to get it, how donations reach you, how to withdraw them, and what happens if
verification is revoked. It assumes you have a Stellar account and can sign
transactions from your NGO's wallet.

You do not need to read the contract code to follow this guide, but every
section points at the contract function that runs the step, so a developer on
your team can verify it.

## The two contracts you will interact with

StreamGive is two contracts, and only one of them is yours to write to:

- **The NGO registry** (`ngo-registry`) holds your organisation's entry: name,
  verified flag, metadata URI, categories, verification expiry. You write to
  this one as the NGO owner.
- **The donation vault** (`donation-vault`) holds donor money and releases it
  to you over time. You read from this one all the time; you write to it only
  for withdrawals and to set your payout address.

The registry is the gatekeeper. The vault calls the registry to check whether
you are a verified NGO before it pays you. If the registry says no, the vault
will not pay out.

## Registration

You register yourself as the owner of your NGO entry:

```text
ngo_registry.register(owner, name)
```

  second registration with the same name fails.

Two things to know before you call it:

1. **Registrations can be paused.** The registry admin can pause new
   applications. If `registrations_paused()` returns true, `register` fails
   and you will have to wait or contact the StreamGive team.
2. **Registration is not verification.** A fresh entry has `verified = false`.
   Donors can see your name and read your metadata, but the vault will not
   release money to an unverified NGO.

After `register` you can update your own entry:

| What you want to change | Function | Notes |
| --- | --- | --- |
| Display name | `update_name(owner, new_name)` | Only while unverified. |
| Public profile URI | `set_metadata_uri(owner, uri)` | A link to a page or JSON describing your work; set or clear any time. |
| Categories | `set_categories(owner, bitmask)` | Only while unverified. The mask is a `u32` of category flags; an invalid mask is rejected. |

If you change your mind entirely you can `unregister(owner)`. Like the fields
above, that only works while you are still unverified — once you are approved
you need an approver to revoke you first.

## Verification

Verification is done **by an approver**, not by you. There is nothing for you
to sign to get verified — you register, then an approver reviews your
application off-chain and approves you:

```text
ngo_registry.approve_ngo(approver, ngo_owner)
```

- `approver` signs the transaction; it must be the admin or one of the
  addresses in the approver set.
- `ngo_owner` is your address.


- `get_ngo(ngo_owner)` shows `verified = true` and an expiry timestamp.
- You can now receive streams and withdraw from them.
seconds) set by the admin. `verification_status(ngo_owner)` tells you exactly
where you stand — whether you are verified, and if so when your current
verification runs out. Before that expiry an admin calls:

```text
ngo_registry.renew_ngo(ngo_owner)
```

to extend it. **If nobody renews and the period elapses, you are effectively
unverified** even though the `verified` flag may still read true — always
check `verification_status` rather than `is_verified` when it matters.

Three other things can happen to your application:

  unverified.
  transaction; the effect is identical to individual approvals.
- **Approver set changes.** The admin can add or remove approvers. This does

## Receiving funds

You do not "receive" a donation at the moment it is made. A donor creates a
**stream**: a lump sum held by the vault, released to you at a fixed rate per
second. The donor can pause, slow down, or cancel the stream at any time.

For each stream:

  withdrawn so far, accrual status.
- `pending_accrual(stream_id)` shows how much has built up for you since the
  last withdrawal.
- `pending_payout(stream_id)` shows what you would receive if you withdrew
  right now (the accrual minus the platform fee).
- `streams_by_ngo(ngo, start, limit)` lists every stream paying you, paged.

donor's guide (`DONORS.md`) explains the arithmetic. For you the important
thing is: the accrual runs continuously, whether or not you withdraw.

## Withdrawing

payout address.

Three forms:

| Function | Use it for |
| --- | --- |
| `withdraw(stream_id)` | Withdraw one stream's accrual. |
| `withdraw_batch(stream_ids)` | Withdraw several streams in one transaction (up to the batch limit). |
| `withdraw_all(ngo, ...)` | Withdraw every stream paying you, in one transaction. |

All three:

- Callable only by your address (the owner). Each call is signed by you.
- Must not be made while the vault is paused. `paused()` tells you whether it
  is.
- Send the funds to your **payout address** (next section).
retry. If one stream in a batch is missing or already depleted, the whole
transaction fails and nothing is withdrawn — you are not left in a half-state.

Withdrawals are audited on-chain. Events are emitted per withdrawal so your
developer can reconcile your bank account against them via `docs/EVENTS.md`.

## The payout address

The vault sends withdrawals to the payout address associated with your NGO,
not to whatever address happens to call the withdraw function. This is
deliberate: it lets you run withdrawals from a hot wallet (an operator's
wallet) while the actual money lands in a cold or treasury address you
control.

Set and read it with:

```text
donation_vault.set_payout_address(ngo, payout)
donation_vault.payout_address(ngo)

- `ngo` is your address; it must sign the transaction.
- `payout` is the Stellar address that will receive withdrawals. It can be
  the same as `ngo`, or different.

Two rules:

1. **Set it before you withdraw the first time.** If `payout_address(ngo)`
   returns nothing, withdrawal will fail.
2. **Changing it takes effect immediately** for every subsequent withdrawal
   from every one of your streams. Funds already withdrawn are not affected.

## What happens if verification is revoked

An approver can revoke your verification with:

```text
ngo_registry.revoke_ngo(approver, ngo_owner)
```

Read carefully, because the practical effects are not symmetric:

**What changes immediately:**

- `is_verified(ngo_owner)` returns false.
- `verification_status(ngo_owner)` reports revoked.
- New streams to your NGO cannot be created. The vault checks the registry
  before opening a stream; if you are not verified, `create_stream` for you
- Your entry stays in the registry, with your name and metadata. You are not
  removed — donors can still see who you were.

**What does not change:**

- **Streams that already exist keep accruing to your payout address.** Revoking
  you does not claw back donations a donor has already committed. The donor
  refunds them the unaccrued balance, but the vault does not do that on the
  registry's behalf.
- **You can still withdraw what has accrued** to an existing stream. The
  withdrawal path checks the vault's pause flag, not the registry's verified
  flag. If the StreamGive team needs to freeze payments to a revoked NGO, they
  use `pause()` on the vault, which pauses all withdrawals for everyone until
  it is unpaused.
keep their commitments until they decide otherwise, and the admin retains a
blunt emergency brake (`pause`) for the case where that is not acceptable.

**If your verification was revoked by mistake**, the approver can approve you
again with `approve_ngo`. Nothing about your old streams or accrued balance is
existing ones from the moment you are re-approved.

## What you cannot do

To save you a support ticket:

- You cannot approve yourself. Verification is done by an approver.
- You cannot change your name or categories once you are verified. Contact the
- You cannot withdraw while the vault is paused.
  call. Change the payout address first.
- You cannot see a list of donor names. Streams are public on-chain, but the
  donor's identity is only what their address reveals.


- `docs/ROLES.md` — every contract function grouped by role, including the
  registry and vault operations you do not call.
- `docs/EVENTS.md` — every event the registry and vault emit, so your
  developer can build notifications.
- `docs/ARCHITECTURE.md` — how the two contracts fit together and why the
- `docs/THREAT_MODEL.md` — the security assumptions behind verification,
  revocation, and the emergency pause.

If a step in this guide fails, the specific contract function's error name
(for example `RegistrationsPaused` or `NotVerified`) tells you which of the
rules above it is enforcing.
