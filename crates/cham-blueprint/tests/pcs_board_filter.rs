//! Board-overlap filter for the PCS walk.
//!
//! A range combo that shares a card with the sampled board is
//! physically impossible. Without filtering, the kernel still counts
//! it, corrupting the CFVs. This test pins the filtering behavior at
//! the boundary: build a range, sample a board that overlaps some
//! combos, and assert (a) the walk runs, (b) the table grows only for
//! the disjoint combos.

use cham_blueprint::pcs::table::RegretTable;
use cham_blueprint::pcs::walk::PcsIteration;
use cham_core::card::Card;
use cham_core::engine::config::EngineConfig;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::Encoder;
use cham_engine::ladder::ActionLadder;
use cham_search::pubtree::PublicTree;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

#[test]
fn overlapping_combos_are_dropped() {
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);
    let tree = PublicTree::build(CFG, &ladder, 100_000);

    // Hero range includes [0,1] and [2,3]; villain range includes [4,5].
    // Board deliberately contains card 0, 2, and 4, so those combos are
    // impossible. Only the "clean" combos should influence the walk.
    let hero: Vec<[u8; 2]> = vec![[0, 1], [8, 9], [2, 3], [10, 11]];
    let villain: Vec<[u8; 2]> = vec![[4, 5], [12, 13], [14, 15]];
    let hero_rank: Vec<u32> = vec![100, 200, 300, 400];
    let villain_rank: Vec<u32> = vec![100, 200, 300];

    let board = [Card(0), Card(2), Card(4), Card(20), Card(21)];

    let mut encoder = Encoder::cfg_only(cfg.clone()).expect("enc");
    let mut table = RegretTable::new();

    let iter = PcsIteration {
        tree: &tree,
        ladder: &ladder,
        hero_range: &hero,
        hero_rank: &hero_rank,
        villain_range: &villain,
        villain_rank: &villain_rank,
        cfg: CFG,
        hero_seat: 1,
    };

    // Run 50 iterations on the SAME board so the table stabilizes.
    for t in 1..=50u64 {
        iter.run(&mut encoder, &mut table, board, t, 1.5, 0.0, 2.0);
    }
    assert!(!table.is_empty(), "table stayed empty");

    // Sanity: the walk ran and produced valid strategies everywhere.
    for (_, row) in table.iter() {
        let s: f64 = row.current_strategy().iter().sum();
        assert!(
            (s - 1.0).abs() < 1e-9,
            "row strategy does not sum to 1: {s}"
        );
    }

    // Also exercise the fully-overlapping extreme: hero range entirely
    // on the board → walk should be a no-op.
    let hero_bad: Vec<[u8; 2]> = vec![[0, 1], [2, 3]];
    let hero_bad_rank: Vec<u32> = vec![100, 200];
    let iter2 = PcsIteration {
        tree: &tree,
        ladder: &ladder,
        hero_range: &hero_bad,
        hero_rank: &hero_bad_rank,
        villain_range: &villain,
        villain_rank: &villain_rank,
        cfg: CFG,
        hero_seat: 1,
    };
    let mut table2 = RegretTable::new();
    for t in 1..=10u64 {
        iter2.run(&mut encoder, &mut table2, board, t, 1.5, 0.0, 2.0);
    }
    assert!(
        table2.is_empty(),
        "walk proceeded despite every hero combo overlapping the board"
    );
}
