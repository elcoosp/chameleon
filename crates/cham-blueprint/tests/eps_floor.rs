//! Tests for the RM+ exploration floor (2026-09-29).
//!
//! Covers the four commits that added the floor without a test:
//!   e16451e  feat(cham-blueprint): sigma_rms_eps
//!   82a9bdb  feat(cham-blueprint): process-wide training exploration floor
//!   cab55fc  feat(cham-blueprint): read CHAM_TRAIN_EPS in trainer startup
//!   3469042  feat(cham-blueprint): record train_explore_eps in RunProvenance
//!
//! Motivated by docs/plans/RM-PLUS-FREEZE-2026-09-29.md: RM+ floors
//! regrets at zero, so at high iteration counts the current iterate
//! freezes one-hot. The floor keeps every action's regret channel alive.
//!
//! Everything here is unit-level. None of it invokes the trainer.

use cham_blueprint::table::{
    RegretTable, ThreadMode, set_train_explore_eps, train_explore_eps,
};

/// Build a table with one row whose regrets are set explicitly.
/// Returns (table, offset, width).
fn table_with_regrets(regrets: &[f32]) -> (RegretTable, u32, usize) {
    let mut t = RegretTable::new(ThreadMode::Deterministic);
    let key = 0xCAFE_BABE_0000_0001u64;
    let (off, w) = t.entry_or_insert(key, regrets.len());
    for (i, &r) in regrets.iter().enumerate() {
        // regret_add_cfr_plus floors at 0, so a negative initial value
        // lands at 0. Pass 0 and add positive values directly.
        if r > 0.0 {
            t.regret_add_cfr_plus(off, i, r);
        }
    }
    (t, off, w)
}

/// Every distribution `sigma_rms_eps` returns must sum to 1 and be
/// non-negative for every action. Kills any mutant that drops a term,
/// divides by the wrong total, or forgets to normalize.
#[test]
fn sigma_rms_eps_distributions_are_valid() {
    let (t, off, w) = table_with_regrets(&[3.0, 1.0, 0.5, 0.0]);
    for eps in [0.0, 0.001, 0.01, 0.02, 0.1, 0.5, 0.99] {
        let sigma = t.sigma_rms_eps(off, w, eps);
        assert_eq!(sigma.len(), w, "eps={eps}: wrong length");
        let total: f64 = sigma.iter().sum();
        assert!(
            (total - 1.0).abs() < 1e-9,
            "eps={eps}: sum {total} != 1.0"
        );
        for (i, &p) in sigma.iter().enumerate() {
            assert!(p >= 0.0, "eps={eps}: action {i} has negative prob {p}");
            assert!(p.is_finite(), "eps={eps}: action {i} not finite: {p}");
        }
    }
}

/// With `eps = 0` the result must equal the historical RM+ strategy:
/// the positive regrets divided by their sum. This is the "no behavior
/// change" contract.
#[test]
fn sigma_rms_eps_zero_matches_rm_plus() {
    let (t, off, w) = table_with_regrets(&[3.0, 1.0, 0.5, 0.0]);
    let sigma = t.sigma_rms_eps(off, w, 0.0);
    // regrets: 3, 1, 0.5, 0 → sum 4.5
    let expected = [3.0 / 4.5, 1.0 / 4.5, 0.5 / 4.5, 0.0];
    for i in 0..w {
        assert!(
            (sigma[i] - expected[i]).abs() < 1e-9,
            "eps=0 action {i}: got {}, want {}",
            sigma[i],
            expected[i]
        );
    }
}

/// With `eps = 0`, `sigma_rms` (no eps arg) must be bit-identical —
/// it reads the process-global floor, which defaults to 0.0.
#[test]
fn sigma_rms_defaults_to_zero_floor() {
    // Ensure the process-global is 0.0 for this test. Other tests in the
    // binary may have set it; reset explicitly.
    set_train_explore_eps(0.0);
    assert_eq!(train_explore_eps(), 0.0);

    let (t, off, w) = table_with_regrets(&[2.0, 1.0]);
    let via_sigma_rms = t.sigma_rms(off, w);
    let via_eps_zero = t.sigma_rms_eps(off, w, 0.0);
    assert_eq!(
        via_sigma_rms, via_eps_zero,
        "sigma_rms must equal sigma_rms_eps(.., 0.0) when the global floor is 0"
    );
}

/// With a positive `eps`, every action must receive at least `eps / w`
/// probability, even the ones whose regret is exactly zero.
#[test]
fn sigma_rms_eps_floors_every_action() {
    let (t, off, w) = table_with_regrets(&[10.0, 0.0, 0.0]);
    for eps in [0.01, 0.05, 0.1, 0.5] {
        let sigma = t.sigma_rms_eps(off, w, eps);
        let floor = eps / w as f64;
        for (i, &p) in sigma.iter().enumerate() {
            assert!(
                p >= floor - 1e-12,
                "eps={eps}: action {i} got {p}, floor is {floor}"
            );
        }
    }
}

/// When all regrets are zero, the result is uniform regardless of eps.
/// (The original code already returned uniform here; eps must not change
/// that.)
#[test]
fn sigma_rms_eps_uniform_on_all_zero_regret() {
    let (t, off, w) = table_with_regrets(&[0.0, 0.0, 0.0, 0.0]);
    for eps in [0.0, 0.02, 0.5, 0.99] {
        let sigma = t.sigma_rms_eps(off, w, eps);
        for (i, &p) in sigma.iter().enumerate() {
            assert!(
                (p - 1.0 / w as f64).abs() < 1e-12,
                "eps={eps}: all-zero-regret action {i} should be uniform, got {p}"
            );
        }
    }
}

/// `eps` values >= 1.0 must be clamped down to 0.99 so the "free" mass
/// `(1 - eps)` stays positive; a caller passing 2.0 must not produce a
/// distribution that sums to 2 or goes negative.
#[test]
fn sigma_rms_eps_clamps_extreme_eps() {
    let (t, off, w) = table_with_regrets(&[5.0, 1.0]);
    for eps in [1.0, 2.0, f64::INFINITY] {
        let sigma = t.sigma_rms_eps(off, w, eps);
        let total: f64 = sigma.iter().sum();
        assert!(
            (total - 1.0).abs() < 1e-9,
            "eps={eps} produced sum {total}"
        );
        assert!(sigma.iter().all(|&p| p >= 0.0 && p.is_finite()));
    }
}

/// `set_train_explore_eps` and `train_explore_eps` round-trip a value.
#[test]
fn set_and_get_train_explore_eps_roundtrip() {
    for v in [0.0, 0.005, 0.02, 0.1, 0.5] {
        set_train_explore_eps(v);
        assert!(
            (train_explore_eps() - v).abs() < 1e-12,
            "set_train_explore_eps({v}) → {}",
            train_explore_eps()
        );
    }
    set_train_explore_eps(0.0);
}

/// `set_train_explore_eps` clamps to [0, 0.5]. A caller passing 10.0
/// must not actually set a 10.0 floor.
#[test]
fn set_train_explore_eps_clamps_out_of_range() {
    set_train_explore_eps(10.0);
    assert!(
        train_explore_eps() <= 0.5,
        "value above 0.5 must be clamped, got {}",
        train_explore_eps()
    );
    set_train_explore_eps(-1.0);
    assert!(
        train_explore_eps() >= 0.0,
        "negative value must be clamped to 0, got {}",
        train_explore_eps()
    );
    set_train_explore_eps(0.0);
}

/// Setting the process-wide floor must actually change `sigma_rms`'s
/// output. This is the wiring that makes `CHAM_TRAIN_EPS` effective.
#[test]
fn sigma_rms_honors_process_wide_floor() {
    set_train_explore_eps(0.0);
    let (t, off, w) = table_with_regrets(&[100.0, 1.0, 1.0]);
    let no_floor = t.sigma_rms(off, w);

    set_train_explore_eps(0.2);
    let with_floor = t.sigma_rms(off, w);

    // Reset for any other test in this binary.
    set_train_explore_eps(0.0);

    // The floored distribution must be more uniform (max prob lower).
    let max_no = no_floor.iter().copied().fold(0.0f64, f64::max);
    let max_with = with_floor.iter().copied().fold(0.0f64, f64::max);
    assert!(
        max_with < max_no,
        "floor must flatten the distribution: no-floor max {max_no}, with-floor max {max_with}"
    );
    // Every action must clear the floor.
    let expected_floor = 0.2 / w as f64;
    for (i, &p) in with_floor.iter().enumerate() {
        assert!(
            p >= expected_floor - 1e-12,
            "action {i} got {p}, floor {expected_floor}"
        );
    }
}

/// Two different eps values must give different distributions — a
/// regression to "always return sigma_rms" would fail this.
#[test]
fn different_eps_give_different_distributions() {
    let (t, off, w) = table_with_regrets(&[4.0, 1.0]);
    let small = t.sigma_rms_eps(off, w, 0.01);
    let large = t.sigma_rms_eps(off, w, 0.3);
    let diff: f64 = small
        .iter()
        .zip(large.iter())
        .map(|(a, b)| (a - b).abs())
        .sum();
    assert!(
        diff > 1e-6,
        "eps=0.01 and eps=0.3 must differ, but delta is only {diff}"
    );
    // Direction: larger eps → closer to uniform.
    let uniform = 1.0 / w as f64;
    let dev_small: f64 = small.iter().map(|&p| (p - uniform).abs()).sum();
    let dev_large: f64 = large.iter().map(|&p| (p - uniform).abs()).sum();
    assert!(
        dev_large < dev_small,
        "larger eps should be closer to uniform: {dev_large} vs {dev_small}"
    );
}
