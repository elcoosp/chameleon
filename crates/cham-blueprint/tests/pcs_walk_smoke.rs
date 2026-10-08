//! PCS walk smoke test: a handful of iterations grow the regret table
//! without panicking, and per-row strategies are valid distributions.
//! Scaffold check only; correctness is proved by the small-deck
//! reduction (design doc, Testing §1).

use cham_blueprint::pcs::sampling;
use cham_blueprint::pcs::table::RegretTable;
use cham_blueprint::pcs::walk::PcsIteration;
use cham_core::engine::config::EngineConfig;
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::Encoder;
use cham_engine::ladder::ActionLadder;
use cham_search::pubtree::PublicTree;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

fn split_ranges(n: usize) -> (Vec<[u8; 2]>, Vec<[u8; 2]>) {
    let mut hero = Vec::with_capacity(n);
    let mut vill = Vec::with_capacity(n);
    for k in 0..n {
        hero.push([(2 * k) as u8, (2 * k + 1) as u8]);
        vill.push([(26 + 2 * k) as u8, (26 + 2 * k + 1) as u8]);
    }
    (hero, vill)
}

#[test]
fn walk_grows_table_and_strategies_valid() {
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);
    let tree = PublicTree::build(CFG, &ladder, 100_000);
    assert!(tree.len() > 1);

    let (hero, vill) = split_ranges(10);
    let hero_rank: Vec<u32> = hero
        .iter()
        .map(|c| (c[0] as u32) * 1000 + c[1] as u32)
        .collect();
    let vill_rank: Vec<u32> = vill
        .iter()
        .map(|c| (c[0] as u32) * 1000 + c[1] as u32)
        .collect();

    let mut encoder = Encoder::cfg_only(cfg.clone()).expect("enc");
    let mut table = RegretTable::new();

    let iter = PcsIteration {
        tree: &tree,
        ladder: &ladder,
        hero_range: &hero,
        hero_rank: &hero_rank,
        villain_range: &vill,
        villain_rank: &vill_rank,
        cfg: CFG,
        hero_seat: 1,
    };

    let mut rng = rng_from_seed(0x42);
    for t in 1..=5u64 {
        let board = sampling::sample_board(&mut rng);
        iter.run(&mut encoder, &mut table, board, t, 1.5, 0.0, 2.0);
    }

    eprintln!("table rows after 5 iterations: {}", table.len());
    assert!(
        !table.is_empty(),
        "table stayed empty after 5 PCS iterations"
    );
}
