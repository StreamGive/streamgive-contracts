# 0001. Compute stream accrual lazily instead of writing it every second

- **Status:** Accepted
- **Date:** 2026-11-27 (recorded after the fact)
- **Applies to:** `donation-vault`

## Context

A stream releases a donor's deposit to an NGO at a fixed rate per second. The
amount an NGO is owed therefore changes every second, but a Soroban contract
only runs when someone sends a transaction. Nothing on the chain can wake the
vault up each second, and writing every stream's state on every ledger would be
both impossible without an external caller and expensive in fees.

## Decision

The vault stores the facts needed to derive the accrued amount and computes it
only when it is needed. A `Stream` keeps its `rate`, remaining `balance`,
`withdrawn` total, `start`, and `last_update` checkpoint. The amount owed is:

```text
accrued = min(rate * (now - max(last_update, start)), balance)
```

This is `math::accrued`, fed by the `accrual_base` helper. A few rules follow
from it:

- **`settle_core` is the one place accrual becomes a payout.** Every operation
  that changes a stream's balance or rate (`withdraw`, `top_up`, `modify_rate`,
  `cancel_stream`) first settles what has accrued up to now, then moves the
  `last_update` checkpoint to the current time. This keeps the arithmetic from
  drifting between entry points.
- **A paused stream accrues nothing.** `settle_core` and `pending_accrual` both
  return zero for a `Paused` stream.
- **A start time acts as a cliff.** Because the accrual base is the later of
  `last_update` and `start`, nothing accrues before `start`, even if the stream
  was checkpointed earlier.
- **Reads never write.** `pending_accrual` and `pending_payout` mirror the same
  formula without changing state, so clients can show a live balance without
  sending a transaction.

## Consequences

- No keeper, cron job, or off-chain bot is needed for streams to "run". The
  accrual is correct at any moment someone asks.
- Cost scales with activity, not with time. An idle stream costs nothing until
  it is touched.
- The NGO is owed money continuously but is paid only when it withdraws, so
  nothing forces it to withdraw on a schedule.
- Any new operation that changes `rate`, `balance`, or the paused state must
  settle first. Skipping that step would change the amount owed retroactively.
- The rate is a whole number of token units per second, so per-second rounding
  is a choice the donor makes when picking a rate. See
  [STREAM_LIFECYCLE.md](../STREAM_LIFECYCLE.md) for converting a monthly amount.
- Fees are computed at settlement time, on the amount being paid out. See
  [FEES.md](../FEES.md).

## Alternatives considered

- **Pushing a payout every second or ledger.** Not possible without an external
  caller, and far more costly. Lazy computation gives the same result for the
  NGO whenever it chooses to collect.

## References

- `math::accrued`, `accrual_base`, `settle_core`, `settle`, and
  `pending_accrual` in `contracts/donation-vault/src`
- [STREAM_LIFECYCLE.md](../STREAM_LIFECYCLE.md), [GLOSSARY.md](../GLOSSARY.md)
- [NGOS.md](../NGOS.md), section "Receiving funds"
