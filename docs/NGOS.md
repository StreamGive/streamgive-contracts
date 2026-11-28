# A guide for NGOs using StreamGive

This guide is for the people who run an NGO and want to receive donations
through StreamGive. It covers how to register, what verification means and how
to get it, how donations reach you, how to withdraw them, and what happens if
verification is revoked. It assumes you have a Stellar account and can sign
transactions from your NGO's wallet.

You do not need to read the contract code to follow this guide, but every
section names the contract function that runs the step, so a developer on your
team can check it against the code. Function names are as of this writing;
[API.md](./API.md) is the full reference.

## The two contracts you will interact with

StreamGive is two contracts, and only one of them is yours to write to:

- **The NGO registry** (`ngo-registry`) holds your organization's entry: name,
  verified flag, metadata URI, categories, and verification expiry. You write to
  this one as the NGO owner.
- **The donation vault** (`donation-vault`) holds donor money and releases it
  to you over time. You read from it all the time; you write to it only to
  withdraw and to set your payout address.

The registry is the gatekeeper for *new money*. When a donor creates a stream,
tops one up, moves one to your NGO, raises its rate, or resumes a paused one,
the vault asks the registry whether you are verified and refuses if you are
not. See [What happens if verification is revoked](#what-happens-if-verification-is-revoked)
for what the check does not cover.

## Registration

You register yourself as the owner of your NGO entry:

```text
ngo_registry.register(owner, name)
```

`owner` signs the transaction. The name can be at most 200 bytes, must contain
at least one visible character, and must not contain control characters. Names
are unique: you can look up who holds a name with `owner_by_name(name)`.

Two things to know before you call it:

1. **Registrations can be paused.** The registry admin can pause new
   applications. If `registrations_paused()` returns true, `register` fails
   and you will have to wait or contact the StreamGive team.
2. **Registration is not verification.** A fresh entry has `verified = false`.
   Donors can see your name and read your metadata, but the vault will not
   accept new streams for an unverified NGO.

After `register` you can update your own entry:

| What you want to change | Function | Notes |
| --- | --- | --- |
| Display name | `update_name(owner, name)` | Same rules as `register`. Locked once you are verified. |
| Public profile URI | `set_metadata_uri(owner, uri)` | A link to a page or JSON describing your work, at most 512 bytes. Pass nothing to clear it. |
| Categories | `set_categories(owner, categories)` | A `u32` made of category bit flags; `0` clears them. Reserved bits are rejected. Locked once you are verified. |

If you change your mind entirely you can `unregister(owner)`. Like category
changes, that only works while you are unverified: once you are approved, an
approver has to revoke you first.

## Verification

Verification is done **by an approver**, not by you. There is nothing for you
to sign to get verified. You register, then an approver reviews your
application off-chain and approves you:

```text
ngo_registry.approve_ngo(approver, ngo_owner)
```

- `approver` signs the transaction; it must be the admin or an address on the
  approver list.
- `ngo_owner` is your address.

Approvers can also approve many applicants at once with `batch_approve`, with
the same effect as individual approvals. An approver can instead decline your
application with `reject_ngo`, which removes your unverified entry; you can
register again afterwards.

Once approved, `get_ngo(ngo_owner)` shows `verified = true`, and the vault
accepts new streams for you.

### Verification can expire

The admin can configure a verification period with `set_verify_period(seconds)`.
With a period of `0` (the default), approvals never expire. With a period set,
each approval runs out that many seconds after it is granted, and only the
admin can extend it:

```text
ngo_registry.renew_ngo(ngo_owner)
```

Once an approval lapses, `is_verified` returns false and `get_ngo` reports
`verified = false`, so the vault treats you as unverified until you are
renewed. `verification_status(ngo_owner)` tells you exactly where you stand:

| Status | Meaning |
| --- | --- |
| `Unknown` | No entry exists for your address. |
| `Pending` | Registered, not yet reviewed. |
| `Verified` | Approved and not expired. |
| `Expired` | Approved, but the verification period has passed. Ask the admin to renew you. |
| `Revoked` | An approver or the admin explicitly revoked you. |

Check `verification_status` rather than `is_verified` when it matters, because
`is_verified` collapses all four non-verified states into `false`.

## Receiving funds

You do not "receive" a donation at the moment it is made. A donor creates a
**stream**: a lump sum held by the vault and released to you at a fixed rate per
second. The donor can pause, slow down, or cancel the stream at any time.

For each stream:

- `get_stream(stream_id)` returns its details: donor, rate, remaining balance,
  how much has been withdrawn so far, and its status.
- `pending_accrual(stream_id)` shows how much has built up for you since the
  stream was last settled, before any fee.
- `pending_payout(stream_id)` shows what you would receive if you withdrew
  right now, as a pair of the net amount and the fee.
- `streams_by_ngo(ngo, start, limit)` lists the ids of the streams paying you,
  one page at a time.

Nothing is sent to you while the stream runs. The amount you are owed grows with
every second, and it is paid out when you withdraw. The
[donor guide](./DONORS.md) explains the arithmetic; the design is recorded in
[ADR 0001](./adr/0001-lazy-accrual-model.md). For you the important thing is
that accrual runs continuously, whether or not you withdraw, and that you do
not need to withdraw on any schedule.

A stream stops accruing while the donor has it paused, and it ends when its
balance reaches zero or the donor cancels it. A donor can also schedule a
stream to start later, in which case nothing accrues before its start time.

## Withdrawing

The vault pays out when you ask it to. Each call is signed by your NGO address
(the owner), and the money goes to your [payout address](#the-payout-address).

| Function | Use it for |
| --- | --- |
| `withdraw(stream_id)` | Withdraw one stream's accrual. |
| `withdraw_batch(stream_ids)` | Withdraw from several streams in one transaction. |
| `withdraw_all(ngo, start, limit)` | Withdraw from every stream paying you, one page at a time. |

Rules that apply:

- **The vault must not be paused.** `paused()` tells you whether it is. A
  paused vault blocks withdrawals for everyone.
- **A stream the donor has paused cannot be withdrawn from** until it is
  resumed.
- **Batches are capped at 20 streams.** `withdraw_batch` fails with
  `BatchLimitExceeded` above that, and rejects the same id twice. `withdraw_all`
  silently limits each page to 20. It returns the amounts it paid and a `next`
  cursor; call it again with `start = next` until `next` reaches the number of
  streams you have.
- **Streams with nothing accrued are skipped** in a batch, so you can pass your
  whole list without filtering. If none of them has anything to withdraw, the
  call fails with `NothingToWithdraw`. A stream id that does not exist aborts
  the whole batch and nothing is withdrawn.
- **A protocol fee may apply.** If the admin has set a fee, it is taken from each
  payout and sent to the treasury. [FEES.md](./FEES.md) has worked examples.

Every withdrawal emits an event, so your developer can reconcile your records
against the chain using [EVENTS.md](./EVENTS.md).

## The payout address

By default the vault pays withdrawals to your NGO address. You can route them
somewhere else, for example a treasury or multisig wallet, while keeping a
separate signing key for day-to-day withdrawals:

```text
donation_vault.set_payout_address(ngo, payout)
donation_vault.payout_address(ngo)
```

- `ngo` is your address; it must sign the transaction. Nobody else can change
  where your money goes.
- `payout` is the Stellar address that will receive withdrawals. It can be the
  same as `ngo`.
- `payout_address(ngo)` returns the address you set, or nothing if you have
  not set one. Nothing means payments go to your NGO address.

Two rules to keep in mind:

1. **You do not have to set one.** If you never do, withdrawals are paid to
   your NGO address.
2. **A change applies to every later withdrawal.** It does not move money that
   has already been paid. The vault emits a `payoutset` event when you change it,
   so make sure the new address can hold the token you are paid in before you
   switch.

## What happens if verification is revoked

An approver can revoke your verification with:

```text
ngo_registry.revoke_ngo(approver, ngo_owner)
```

Read this section carefully, because the effects are not symmetric.

**What changes immediately:**

- `is_verified(ngo_owner)` returns false and `verification_status(ngo_owner)`
  reports `Revoked`.
- Donors cannot start new money flowing to you. The vault refuses
  `create_stream`, `top_up`, `transfer_stream` to your NGO, a rate increase, and
  resuming a paused stream, each with `NgoNotVerified`.
- Your entry stays in the registry with your name and metadata, so donors can
  still see who you were.

**What does not change:**

- **Streams that already exist keep accruing to you.** Revoking you does not
  claw back money a donor has already committed. Only the donor can cancel a
  stream and take back the part that has not accrued yet; the vault does not
  do that on the registry's behalf.
- **You can still withdraw what has accrued.** The withdrawal path checks the
  vault's pause flag, not the registry. A donor can still lower a stream's
  rate or cancel it at any time.
- If the StreamGive team needs to stop payments to a revoked NGO, the only
  blunt tool is `pause()` on the vault, which stops withdrawals for everyone
  until it is unpaused.

This is deliberate: donors keep control of money they have already committed,
and existing commitments are honored until the donor decides otherwise. The
reasoning and trade-offs are in [ADR 0003](./adr/0003-registry-check-in-vault.md),
and the residual risk is accepted as T-20 in [THREAT_MODEL.md](./THREAT_MODEL.md).

**If your verification was revoked by mistake**, an approver can approve you
again with `approve_ngo`. Nothing about your old streams or your accrued balance
is lost in the meantime. New streams and top-ups can reach you again from the
moment you are re-approved.

## What you cannot do

To save you a support ticket:

- **You cannot verify yourself.** Only the admin or an approver can.
- **You cannot rename, re-categorize, or unregister once verified.** Ask an
  approver to revoke you first.
- **You cannot withdraw while the vault is paused,** or from a stream the donor
  has paused.
- **You cannot stop a donor from cancelling.** A donor can cancel a stream at
  any time. What has accrued up to that moment is paid to you, and the rest
  goes back to the donor.
- **You cannot move a stream to a different NGO.** Only the donor can.

## Where to look next

- [DONORS.md](./DONORS.md): the same system from the donor's side.
- [STREAM_LIFECYCLE.md](./STREAM_LIFECYCLE.md): the life of a stream from
  creation to cancellation.
- [FEES.md](./FEES.md): how the protocol fee is calculated.
- [ROLES.md](./ROLES.md): who may call which function.
- [THREAT_MODEL.md](./THREAT_MODEL.md): what the contracts protect and which
  risks are accepted.
