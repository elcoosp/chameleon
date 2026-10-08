//! DCFR discount schedules (Brown & Sandholm 2019).
//!
//! Convention: `t` is the 1-based iteration counter. `t = 0` is not a
//! valid iteration and both discount functions return 0 at `t = 0`
//! (their formulas evaluate to `0/1 = 0`); callers start at `t = 1`.
//!
//! The three schedules:
//!
//! - positive-regret discount: `t^alpha / (t^alpha + 1)`
//! - negative-regret discount: `t^beta  / (t^beta  + 1)`
//! - strategy-sum weight:      `(t / (t + 1))^gamma`
//!
//! With alpha = 1.5, beta = 0.0, gamma = 2.0 (design doc §DCFR updates):
//! at `t = 1` the positive discount is 0.5, the negative discount is
//! 0.5 (beta = 0 makes it constant), and the strategy weight is 0.25.

/// Positive-regret discount `t^alpha / (t^alpha + 1)`.
#[inline]
pub fn positive_discount(t: u64, alpha: f64) -> f64 {
    if t == 0 {
        return 0.0;
    }
    let ta = (t as f64).powf(alpha);
    ta / (ta + 1.0)
}

/// Negative-regret discount `t^beta / (t^beta + 1)`.
#[inline]
pub fn negative_discount(t: u64, beta: f64) -> f64 {
    if t == 0 {
        return 0.0;
    }
    let tb = (t as f64).powf(beta);
    tb / (tb + 1.0)
}

/// Strategy-sum weight `(t / (t + 1))^gamma`.
#[inline]
pub fn strategy_weight(t: u64, gamma: f64) -> f64 {
    if t == 0 {
        return 0.0;
    }
    let x = t as f64 / (t as f64 + 1.0);
    x.powf(gamma)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-12;

    #[test]
    fn positive_discount_t1_alpha15_is_half() {
        assert!((positive_discount(1, 1.5) - 0.5).abs() < EPS);
    }

    #[test]
    fn positive_discount_increases_with_t() {
        // t^alpha / (t^alpha + 1) -> 1 as t -> inf.
        let a = positive_discount(10, 1.5);
        let b = positive_discount(1000, 1.5);
        assert!(b > a);
        assert!(b < 1.0);
    }

    #[test]
    fn negative_discount_beta0_is_constant_half() {
        for t in [1u64, 2, 100, 100_000] {
            assert!((negative_discount(t, 0.0) - 0.5).abs() < EPS);
        }
    }

    #[test]
    fn strategy_weight_t1_gamma2_is_quarter() {
        assert!((strategy_weight(1, 2.0) - 0.25).abs() < EPS);
    }

    #[test]
    fn strategy_weight_increases_toward_one() {
        // (t/(t+1))^gamma is the RETENTION factor from DCFR: it
        // multiplies the accumulated strategy sum, decaying old mass
        // LESS as t grows. Monotone increasing toward 1.
        let a = strategy_weight(10, 2.0);
        let b = strategy_weight(1000, 2.0);
        assert!(b > a, "expected weight to increase: a={a} b={b}");
        assert!(b < 1.0, "weight must stay below 1: b={b}");
        assert!(a > 0.0);
    }

    #[test]
    fn t_zero_is_degenerate_zero() {
        assert_eq!(positive_discount(0, 1.5), 0.0);
        assert_eq!(negative_discount(0, 0.0), 0.0);
        assert_eq!(strategy_weight(0, 2.0), 0.0);
    }
}
