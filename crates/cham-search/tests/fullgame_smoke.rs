//! Smoke test for the full-game VBR walker.

use cham_core::card::{Card, Hand2};
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

fn split_ranges(board: &[Card; 5], n: usize) -> (Vec<[u8; 2]>, Vec<[u8; 2]>) {
    let mut avail: Vec<u8> = Vec::new();
    for c in 0..52u8 {
        if !board.iter().any(|b| b.idx() as usize == c as usize) { avail.push(c); }
    }
    let half = avail.len() / 2;
    fn take(pool: &[u8], n: usize) -> Vec<[u8; 2]> {
        let mut out = Vec::new();
        'outer: for i in 0..pool.len() {
            for j in (i + 1)..pool.len() {
                out.push([pool[i], pool[j]]);
                if out.len() == n { break 'outer; }
            }
        }
        out
    }
    (take(&avail[..half], n), take(&avail[half..], n))
}

#[test]
fn fullgame_smoke() {
    let ladder = ActionLadder::new(&AbstractionConfig::tiny());
    let tree = PublicTree::build(CFG, &ladder, 100_000);
    assert!(tree.len() > 1);

    let board = board_from_seed(0xB0);
    let (hero, vill) = split_ranges(&board, 20);
    assert_eq!(hero.len(), 20);
    assert_eq!(vill.len(), 20);
    let hero_cards: Vec<u8> = hero.iter().flat_map(|c| c.iter().copied()).collect();
    for v in &vill {
        for hc in &hero_cards {
            assert!(*hc != v[0] && *hc != v[1], "hero/villain card overlap");
        }
    }

    let rank = |c: &[u8; 2]| -> u32 {
        (cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &board) * 1e6) as u32
    };
    let hero_rank: Vec<u32> = hero.iter().map(rank).collect();
    let vill_rank: Vec<u32> = vill.iter().map(rank).collect();
    let hw = vec![1.0 / hero.len() as f64; hero.len()];
    let vw = vec![1.0 / vill.len() as f64; vill.len()];

    let mut policy = |_st: &State, _seq: &ActionSeq, na: usize, _j: usize| -> Vec<f64> {
        if na == 0 { Vec::new() } else { vec![1.0 / na as f64; na] }
    };

    let mut vbr = FullGameVbr {
        tree: &tree, ladder: &ladder,
        hero_range: &hero, hero_rank: &hero_rank, hero_w: &hw,
        villain_range: &vill, villain_rank: &vill_rank, villain_w: &vw,
        cfg: CFG, hero_seat: 1, policy: &mut policy,
    };
    let v = vbr.best_response(&board).expect("None");
    eprintln!("smoke: {:.6} bb", v);
    assert!(v.is_finite());
    assert!(v.abs() > 1e-6);
    assert!(v.abs() < 1000.0);
}
