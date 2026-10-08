//! Smoke test for the full-game VBR walker (plan §8 step 2).
//!
//! Not a correctness gate — the D1 test is. This test proves the
//! walker terminates, produces finite EV, and returns a plausible
//! number on a single sampled board with a fixed policy. If this test
//! fails, the walker is structurally broken (NaN, infinite loop, sign
//! error of many orders of magnitude). If this test passes but D1's
//! river-reduction test fails, the walker's math is wrong at a subtler
//! level.

use cham_core::card::{Card, Hand2};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::Action;
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
    let mut used = [false; 52];
    for c in board {
        used[c.idx() as usize] = true;
    }
    let mut all: Vec<[u8; 2]> = Vec::new();
    for a in 0..52u8 {
        if used[a as usize] {
            continue;
        }
        for b in (a + 1)..52u8 {
            if used[b as usize] {
                continue;
            }
            all.push([a, b]);
        }
    }
    // Deterministic split: even indices to hero, odd to villain.
    let hero: Vec<[u8; 2]> = all.iter().step_by(2).take(n).copied().collect();
    let vill: Vec<[u8; 2]> = all.iter().skip(1).step_by(2).take(n).copied().collect();
    (hero, vill)
}

#[test]
fn fullgame_smoke() {
    let ladder = ActionLadder::new(&AbstractionConfig::tiny());
    let tree = PublicTree::build(CFG, &ladder, 100_000);
    assert!(tree.len() > 1, "PublicTree collapsed (len = {})", tree.len());

    let board = board_from_seed(0xB0);
    let (hero, vill) = split_ranges(&board, 20);
    assert!(!hero.is_empty() && !vill.is_empty(), "empty ranges");

    let rank = |c: &[u8; 2], board: &[Card; 5]| -> u32 {
        (cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), board) * 1e6) as u32
    };
    let hero_rank: Vec<u32> = hero.iter().map(|c| rank(c, &board)).collect();
    let vill_rank: Vec<u32> = vill.iter().map(|c| rank(c, &board)).collect();
    let hw = vec![1.0 / hero.len() as f64; hero.len()];
    let vw = vec![1.0 / vill.len() as f64; vill.len()];

    // Uniform policy: four slots (check, bet-small, bet-big, jam) with
    // equal mass. The walker's `probs.get(i).unwrap_or(0.0)` truncates
    // to the node's actual action count. Villain's effective probability
    // per node does NOT normalise to 1 in general — the walker assumes
    // the policy returns the correct length. For this smoke test that is
    // acceptable: the walker must still terminate and return a finite
    // number, which is what we assert.
    let mut policy = |_hist: &[Action], _seq: &ActionSeq, _j: usize| -> Vec<f64> {
        vec![0.25, 0.25, 0.25, 0.25]
    };

    let mut vbr = FullGameVbr {
        tree: &tree,
        hero_range: &hero,
        hero_rank: &hero_rank,
        hero_w: &hw,
        villain_range: &vill,
        villain_rank: &vill_rank,
        villain_w: &vw,
        cfg: CFG,
        hero_seat: 1,
        policy: &mut policy,
    };
    let v = vbr.best_response(&board);
    eprintln!("fullgame smoke (board 0xB0): {:?}", v);
    let v = v.expect("walker returned None");
    assert!(v.is_finite(), "walker returned non-finite EV: {v}");
    assert!(
        v.abs() < 1000.0,
        "walker EV wildly out of range: {v} (expected |v| < 1000 bb)"
    );
}
