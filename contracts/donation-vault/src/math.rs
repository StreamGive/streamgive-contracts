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
}
