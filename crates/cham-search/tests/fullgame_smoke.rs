//! Smoke test for the full-game VBR walker (plan §8 step 2).
//!
//! History. The first version (commit 619fba1) split hero and villain
//! ranges by taking alternating entries of the combo list. With the
//! board from seed 0xB0, card 0 is off-board, so the first ~46 combos
//! in that list all contain card 0 — hero (even indices) and villain
//! (odd indices) ended up sharing card 0 in every combo. Every hero
//! combo therefore overlapped every villain combo, `showdown_cfv_two`
//! returned 0 for every pair, and the walker correctly reported 0.
//!
//! This version splits the non-board card pool in half and draws hero
//! combos from the first half and villain combos from the second, so
//! the two ranges are card-disjoint by construction.
//!
//! This is still only a smoke test: it proves the walker runs and
//! returns a finite, non-degenerate number on one board. Correctness
//! is proved by the river-reduction test (design doc §Testing).

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

/// Split the non-board cards in half; hero draws combos from the first
/// half, villain from the second. Zero card overlap by construction.
fn split_ranges(board: &[Card; 5], n: usize) -> (Vec<[u8; 2]>, Vec<[u8; 2]>) {
    let mut avail: Vec<u8> = Vec::new();
    for c in 0..52u8 {
        let on_board = board.iter().any(|b| b.idx() as usize == c as usize);
        if !on_board {
            avail.push(c);
        }
    }
    let half = avail.len() / 2;
    let hero_pool = &avail[..half];
    let vill_pool = &avail[half..];

    fn take(pool: &[u8], n: usize) -> Vec<[u8; 2]> {
        let mut out = Vec::new();
        'outer: for i in 0..pool.len() {
            for j in (i + 1)..pool.len() {
                out.push([pool[i], pool[j]]);
                if out.len() == n {
                    break 'outer;
                }
            }
        }
        out
    }

    (take(hero_pool, n), take(vill_pool, n))
}

#[test]
fn fullgame_smoke() {
    let ladder = ActionLadder::new(&AbstractionConfig::tiny());
    let tree = PublicTree::build(CFG, &ladder, 100_000);
    assert!(tree.len() > 1, "PublicTree collapsed (len = {})", tree.len());

    let board = board_from_seed(0xB0);
    let (hero, vill) = split_ranges(&board, 20);
    assert_eq!(hero.len(), 20, "hero range short");
    assert_eq!(vill.len(), 20, "villain range short");

    // Card-disjointness self-check. If this ever trips, the smoke test
    // is back to the degenerate regime where showdown EV is 0 for the
    // wrong reason.
    let hero_cards: Vec<u8> = hero.iter().flat_map(|c| c.iter().copied()).collect();
    for v in &vill {
        for hc in &hero_cards {
            assert!(*hc != v[0] && *hc != v[1], "hero/villain card overlap");
        }
    }

    let rank = |c: &[u8; 2], board: &[Card; 5]| -> u32 {
        (cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), board) * 1e6) as u32
    };
    let hero_rank: Vec<u32> = hero.iter().map(|c| rank(c, &board)).collect();
    let vill_rank: Vec<u32> = vill.iter().map(|c| rank(c, &board)).collect();
    let hw = vec![1.0 / hero.len() as f64; hero.len()];
    let vw = vec![1.0 / vill.len() as f64; vill.len()];

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
    eprintln!("fullgame smoke (board 0xB0, disjoint ranges): {:?}", v);
    let v = v.expect("walker returned None");
    assert!(v.is_finite(), "walker returned non-finite EV: {v}");
    assert!(
        v.abs() > 1e-6,
        "walker returned degenerate EV {v} — this smoke test is not exercising the walker"
    );
    assert!(
        v.abs() < 1000.0,
        "walker EV wildly out of range: {v} (expected |v| < 1000 bb)"
    );
}
