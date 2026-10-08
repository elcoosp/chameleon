//! Fold-sign regression: villain folds preflop, hero (BB) wins +0.5 bb.

use cham_core::card::Card;
use cham_core::engine::config::EngineConfig;
use cham_core::engine::State;
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::ActionSeq;
use cham_engine::ladder::ActionLadder;
use cham_search::fullgame::FullGameVbr;
use cham_search::pubtree::PublicTree;

const CFG: EngineConfig = EngineConfig { start_stack: 10_000, sb: 50, bb: 100 };

fn board_from_seed(seed: u64) -> [Card; 5] {
    let mut rng = rng_from_seed(seed);
    let mut deck: Vec<u8> = (0..52).collect();
    for i in (1..52).rev() {
        let j = (cham_core::rng::next_f64(&mut rng) * (i + 1) as f64) as usize;
        deck.swap(i, j);
    }
    [Card(deck[0]), Card(deck[1]), Card(deck[2]), Card(deck[3]), Card(deck[4])]
}

#[test]
#[ignore = "fold-sign regression; --ignored --nocapture"]
fn villain_folds_preflop_hero_wins_half_bb() {
    let board = board_from_seed(0xB0);
    let mut avail: Vec<u8> = Vec::new();
    for c in 0..52u8 {
        if !board.iter().any(|b| b.idx() as usize == c as usize) { avail.push(c); }
    }
    let half = avail.len() / 2;
    let hero: Vec<[u8; 2]> = (0..4).map(|k| [avail[2 * k], avail[2 * k + 1]]).collect();
    let vill: Vec<[u8; 2]> = (0..4).map(|k| [avail[half + 2 * k], avail[half + 2 * k + 1]]).collect();

    let hero_rank: Vec<u32> = vec![0; hero.len()];
    let vill_rank: Vec<u32> = vec![0; vill.len()];
    let hw = vec![1.0 / hero.len() as f64; hero.len()];
    let vw = vec![1.0 / vill.len() as f64; vill.len()];

    let ladder = ActionLadder::new(&AbstractionConfig::tiny());
    let tree = PublicTree::build(CFG, &ladder, 100_000);

    let mut policy = |_st: &State, _seq: &ActionSeq, na: usize, _j: usize| -> Vec<f64> {
        if na == 0 { Vec::new() } else { let mut v = vec![0.0; na]; v[0] = 1.0; v }
    };

    let mut vbr = FullGameVbr {
        tree: &tree, ladder: &ladder,
        hero_range: &hero, hero_rank: &hero_rank, hero_w: &hw,
        villain_range: &vill, villain_rank: &vill_rank, villain_w: &vw,
        cfg: CFG, hero_seat: 1, policy: &mut policy,
    };
    let v = vbr.best_response(&board).expect("None");
    eprintln!("fold EV: {:.12} bb", v);
    assert!((v - 0.5).abs() < 1e-9, "expected +0.5, got {}", v);
}
