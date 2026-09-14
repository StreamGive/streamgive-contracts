/// Computes how much of `balance` has unlocked given a constant per-second
/// `rate` sustained over `elapsed` seconds, capped so it can never exceed
/// what's actually left in the stream.
///
/// Returns 0 for a non-positive rate/balance or zero elapsed time, and
/// saturates instead of overflowing/panicking if `rate * elapsed` would
/// exceed i128's range.
pub fn accrued(rate: i128, elapsed: u64, balance: i128) -> i128 {
    if rate <= 0 || balance <= 0 || elapsed == 0 {
        return 0;
    }

    let unlocked = rate.saturating_mul(elapsed as i128);
    unlocked.min(balance)
}

#[cfg(test)]
mod test {
    use super::accrued;

    #[test]
    fn zero_rate_accrues_nothing() {
        assert_eq!(accrued(0, 100, 1_000), 0);
    }

    #[test]
    fn negative_rate_accrues_nothing() {
        assert_eq!(accrued(-5, 100, 1_000), 0);
    }

    #[test]
    fn zero_elapsed_accrues_nothing() {
        assert_eq!(accrued(10, 0, 1_000), 0);
    }

    #[test]
    fn zero_balance_accrues_nothing() {
        assert_eq!(accrued(10, 100, 0), 0);
    }

    #[test]
    fn accrues_rate_times_elapsed_under_balance() {
        assert_eq!(accrued(10, 5, 1_000), 50);
    }

    #[test]
    fn caps_at_remaining_balance() {
        assert_eq!(accrued(10, 1_000, 500), 500);
    }

    #[test]
    fn saturates_instead_of_overflowing() {
        assert_eq!(accrued(i128::MAX, u64::MAX, i128::MAX), i128::MAX);
    }

    #[test]
    fn saturates_at_various_overflow_boundaries() {
        // rate * elapsed overflows i128 well before either operand hits its
        // own max — these combinations all overflow the raw multiplication
        // and must saturate to `balance`, not panic or wrap.
        assert_eq!(accrued(i128::MAX, 2, 1_000), 1_000);
        assert_eq!(accrued(i128::MAX / 2, u64::MAX, i128::MAX), i128::MAX);
        assert_eq!(accrued(1_000_000_000_000, u64::MAX, 500), 500);
    }

    #[test]
    fn rapid_cancel_ticks_never_go_negative_or_panic() {
        // Cancelling one ledger (or zero) after creation is the minimal
        // "rapid cancel" case — accrual over 0 or 1 second should be tiny
        // (or zero) and never negative.
        for &rate in &[0i128, 1, 1_000, i128::MAX] {
            for &balance in &[0i128, 1, 1_000, i128::MAX] {
                assert_eq!(accrued(rate, 0, balance), 0);
                let one_tick = accrued(rate, 1, balance);
                assert!(one_tick >= 0);
                assert!(one_tick <= balance.max(0));
            }
        }
    }

    /// Deterministic stand-in for a property test: sweeps a grid of rates,
    /// balances, and elapsed durations (including the zero-rate,
    /// near-overflow, and zero/near-zero-elapsed edges) and checks the
    /// invariants that must hold for every input rather than a handful of
    /// hand-picked examples.
    #[test]
    fn invariants_hold_across_a_grid_of_inputs() {
        let rates = [0i128, 1, 7, 10_000, 1_000_000_000, i128::MAX / 2, i128::MAX];
        let balances = [0i128, 1, 999, 1_000_000, i128::MAX];
        let elapsed_steps = [0u64, 1, 2, 100, 10_000, u64::MAX];

        for &rate in &rates {
            for &balance in &balances {
                let mut prev = 0i128;
                for &elapsed in &elapsed_steps {
                    let a = accrued(rate, elapsed, balance);

                    // Never negative, never more than what's left in the stream.
                    assert!(
                        a >= 0,
                        "negative accrual: rate={rate} elapsed={elapsed} balance={balance}"
                    );
                    assert!(
                        a <= balance.max(0),
                        "accrual exceeds balance: rate={rate} elapsed={elapsed} balance={balance}"
                    );

                    // More elapsed time never accrues less (accrual is
                    // monotonic non-decreasing in elapsed, even once capped).
                    assert!(
                        a >= prev,
                        "accrual decreased as elapsed grew: rate={rate} elapsed={elapsed} balance={balance}"
                    );
                    prev = a;
                }
            }
        }
    }

    #[test]
    fn monotonic_in_rate_for_fixed_elapsed_and_balance() {
        let rates = [0i128, 1, 5, 50, 500, i128::MAX];
        let balance = 10_000i128;
        let elapsed = 10u64;

        let mut prev = 0i128;
        for &rate in &rates {
            let a = accrued(rate, elapsed, balance);
            assert!(a >= prev, "accrual decreased as rate grew: rate={rate}");
            prev = a;
        }
    }
}
