//! PCS module smoke test: DCFR math + sampler + table, all wired
//! through the public API. This is a scaffold check, not the
//! correctness gate (which is the small-deck reduction).

use cham_blueprint::pcs::dcfr;
use cham_blueprint::pcs::sampling;
use cham_blueprint::pcs::table::RegretTable;
use cham_core::rng::rng_from_seed;
use std::collections::HashSet;

#[test]
fn dcfr_defaults_are_the_design_values() {
    // Design doc: alpha=1.5, beta=0.0, gamma=2.0.
    // At t=1: positive=0.5, negative=0.5, weight=0.25.
    assert!((dcfr::positive_discount(1, 1.5) - 0.5).abs() < 1e-12);
    assert!((dcfr::negative_discount(1, 0.0) - 0.5).abs() < 1e-12);
    assert!((dcfr::strategy_weight(1, 2.0) - 0.25).abs() < 1e-12);
}

#[test]
fn sampler_and_table_compose() {
    let mut rng = rng_from_seed(0x42);
    let mut table = RegretTable::new();
    let mut seen: HashSet<[u8; 5]> = HashSet::new();
    for iter in 1..=50u64 {
        let board = sampling::sample_board(&mut rng);
        let key = (iter << 32) | 0x8000_0000;
        let row = table.row_mut(key, 3);
        row.visits += 1;
        row.regret[0] += 1.0;
        let s = row.current_strategy();
        assert!((s.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        let mut sorted: [u8; 5] = board.map(|c| c.idx());
        sorted.sort();
        seen.insert(sorted);
    }
    assert_eq!(table.len(), 50);
    assert!(
        seen.len() > 45,
        "sampler degenerate: {} distinct",
        seen.len()
    );
}

#[test]
fn default_dcfr_gamma_is_2_0() {
    assert_eq!(cham_blueprint::default_dcfr_gamma(), 2.0);
}
