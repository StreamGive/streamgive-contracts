# Protocol fee calculation

The donation vault applies a protocol fee only when a treasury address has
been configured. The fee is taken from accrued stream payouts to the NGO; the
original donation deposit and refunds from a cancelled stream are not charged.

## Formula

The administrator configures `fee_bps` in basis points. One basis point is
one ten-thousandth, so the fee for an accrued payout is:

```text
fee = floor(payout * fee_bps / 10,000)
ngo_amount = payout - fee
```

The contract performs integer division, so fractional token units are rounded
down. `fee_bps` is capped at 1,000 basis points (10%), and the calculated fee
can never exceed the payout. If no treasury is configured, the fee is zero
even when a non-zero fee rate has been stored.

## Examples

At 500 basis points (5%), an accrued payout of 1,000 token units produces a
50-unit fee and 950 units for the NGO:

```text
floor(1,000 * 500 / 10,000) = 50
```

At the same rate, an accrued payout of 19 token units produces no fee because
the fractional result is rounded down:

```text
floor(19 * 500 / 10,000) = floor(0.95) = 0
```

The treasury must be set before the fee can be transferred. Without one, the
full accrued payout goes to the NGO.
