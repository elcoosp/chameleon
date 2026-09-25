//! Contractual proofs for cham-proofs (SPECS/11 M-1): P-1..P-4.

use cham_proofs::{
    proof_bayes_mixture, proof_es_mccfr_kuhn, proof_one_sided_br, proof_solver_matches_lp, run_all,
};

#[test]
fn p1_es_mccfr_validity() {
    let r = proof_es_mccfr_kuhn(20_000);
    assert!(r.passed, "P-1: {r:?}");
    // the gap to the Nash game value shrinks with more iterations
    let r_late = proof_es_mccfr_kuhn(60_000);
    assert!(
        r_late.value.abs() <= r.value.abs() + 0.05,
        "converging toward the Nash value: {} → {}",
        r.value,
        r_late.value
    );
}

#[test]
fn p2_one_sided_br_convergence() {
    let r = proof_one_sided_br();
    assert!(r.passed, "P-2: {r:?}");
}

#[test]
fn p3_bayes_mixture_beats_single() {
    let r = proof_bayes_mixture();
    assert!(r.passed, "P-3: {r:?}");
}

#[test]
fn p4_solver_matches_lp() {
    let r = proof_solver_matches_lp();
    assert!(r.passed, "P-4: {r:?}");
}

#[test]
fn gate_m1_all_green() {
    let results = run_all(30_000);
    for r in &results {
        assert!(r.passed, "gate G-M-1 requires all proofs green: {r:?}");
    }
    assert_eq!(results.len(), 4);
}

#[test]
fn kuhn_game_value_sanity() {
    // the Nash game value for seat 0 is −1/18; our exploitability bound is looser
    // but the counterfactual value of the uniform profile must be near it
    let (regrets, strat_sum) = cham_proofs::kuhn_mccfr(10_000, 0x51EED);
    assert!(!regrets.is_empty());
    let ev0 = cham_proofs::kuhn_profile_ev(&strat_sum, 0);
    assert!(
        (ev0 - cham_proofs::KUHN_GAME_VALUE_P0).abs() < 0.2,
        "trained profile EV near the Nash game value: {ev0}"
    );
}
