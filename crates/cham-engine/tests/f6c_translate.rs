//! F6c (2026-10-01): unit tests for the pseudo-harmonic translation
//! primitives added to `ladder.rs`. These pin the report's reference
//! values without needing a live engine state — `ph_prob_lower` is a
//! pure function of two abstract fractions and one real fraction.
//!
//! See `docs/plans/chameleon-competitiveness-report.md` §F6(c,d) for
//! the derivation.

use cham_engine::ladder::ph_prob_lower;

/// At the lower bracket boundary (x == a), the mapping must put ALL
/// mass on the lower slot: P = 1.0 exactly.
#[test]
fn ph_prob_lower_at_lower_boundary_is_one() {
    let a = 0.5;
    let b = 1.0;
    let x = a;
    let p = ph_prob_lower(a, b, x);
    assert!((p - 1.0).abs() < 1e-12, "expected 1.0, got {p}");
}

/// At the upper bracket boundary (x == b), the mapping must put ZERO
/// mass on the lower slot.
#[test]
fn ph_prob_lower_at_upper_boundary_is_zero() {
    let a = 0.5;
    let b = 1.0;
    let x = b;
    let p = ph_prob_lower(a, b, x);
    assert!(p.abs() < 1e-12, "expected 0.0, got {p}");
}

/// The report's worked example: A=0.5, B=1.0, x=0.6 → 0.750. The
/// repo's old `1/(0.01+(x−f)²)` kernel returned 0.895 here; the
/// correction is the whole point of F6(d).
#[test]
fn ph_prob_lower_report_example_0_750() {
    let a = 0.5;
    let b = 1.0;
    let x = 0.6;
    let p = ph_prob_lower(a, b, x);
    assert!((p - 0.750).abs() < 1e-12, "expected 0.750, got {p}");
}

/// Monotonicity: as x sweeps from a to b, P(lower) must be strictly
/// decreasing. Sample 11 interior points.
#[test]
fn ph_prob_lower_is_strictly_decreasing_in_x() {
    let a = 0.25;
    let b = 2.0;
    let mut prev = f64::INFINITY;
    for k in 0..=10 {
        let t = k as f64 / 10.0;
        let x = a + t * (b - a);
        let p = ph_prob_lower(a, b, x);
        assert!(p < prev + 1e-12, "not monotone at x={x}: p={p} prev={prev}");
        prev = p;
    }
}

/// Sanity: on any interior bracket the result stays inside [0, 1].
#[test]
fn ph_prob_lower_stays_in_unit_interval() {
    let cases = [(0.5, 1.0, 0.6), (1.0, 3.0, 2.0), (0.1, 0.9, 0.5)];
    for (a, b, x) in cases {
        let p = ph_prob_lower(a, b, x);
        assert!(
            (0.0..=1.0).contains(&p),
            "out of range for {a},{b},{x}: {p}"
        );
    }
}
