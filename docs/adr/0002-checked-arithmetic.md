# 0002. Use checked arithmetic and fail with `ArithmeticOverflow` rather than wrap or panic

- **Status:** Accepted
- **Date:** 2026-11-27 (recorded after the fact)
- **Applies to:** `donation-vault` and `ngo-registry`

## Context

The vault does arithmetic on money: balances, withdrawn totals, fees, lifetime
counters. Amounts are `i128` and rates multiply by elapsed seconds, so a large
rate or a long time can exceed the type's range. Rust's default behavior on
overflow depends on build settings, and a silent wrap in an accounting value
could mint or destroy tokens in the books, while a bare panic gives callers no
usable error code.

## Decision

Arithmetic that updates stored state or decides a payout is checked, and an
overflow becomes a typed contract error instead of a wrap or panic.

- **Stored totals use `checked_add`, `checked_sub`, and `checked_mul`** and map
  failure to `Error::ArithmeticOverflow` (code 9 in the vault, code 10 in the
  registry). Examples are `record_payout`, the lifetime totals, the donor
  totals, the stream id counter, and TTL arithmetic.
- **Accrual saturates, because it is capped.** `math::accrued` uses
  `saturating_mul` for `rate * elapsed` and then takes the minimum with the
  remaining `balance`. Saturating is safe here: whatever the product would have
  been, the result can never exceed what is left in the stream.
- **Fees avoid intermediate overflow.** The fee calculation is arranged so that
  `amount * fee_bps` cannot overflow for large amounts, and `fee_from_payout`
  returns `ArithmeticOverflow` rather than report a fee of zero if its
  invariant is ever broken. That aborts the transaction instead of publishing
  a misleading `withdraw` event.
- **Helpers avoid hidden overflow too.** `seconds_to_deplete` computes a ceiling
  division without the `balance + rate - 1` form that can overflow.
- **The release profile keeps `overflow-checks = true`** in `Cargo.toml` as a
  backstop for any arithmetic that is not explicitly checked.

## Consequences

- An overflow aborts the whole transaction atomically. No partial state is
  written, and the caller gets a stable error number instead of a trap.
- Integrators can handle `ArithmeticOverflow` like any other error. Because
  error codes are append-only, code 9 and code 10 can never be reused. See
  [UPGRADE_POLICY.md](../UPGRADE_POLICY.md).
- Checked operations cost a little more compute than unchecked ones. The
  contracts accept this for money-handling paths.
- New code that touches balances or totals must use checked operations, or
  document why saturation is safe as `math::accrued` does. The tests in
  `math.rs` sweep boundary values to keep these properties.

## References

- `math::accrued`, `math::seconds_to_deplete`, `record_payout`,
  `fee_from_payout` in `contracts/donation-vault/src`
- `[profile.release]` in the workspace `Cargo.toml`
- [UPGRADE_POLICY.md](../UPGRADE_POLICY.md), [FEES.md](../FEES.md)
