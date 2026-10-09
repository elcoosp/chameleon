//! Plan gate (Phase B): VBR(uniform) ≫ VBR(shipped).
//!
//! The plan's Phase B gate reads: "VBR(uniform) ≫ VBR(shipped) ≫ 0".
//! The shipped-blueprint number is 5.77 ± 0.55 (this session). This test
//! runs the SAME full-game VBR walker against a uniform-random villain
//! and asserts the resulting VBR is much larger.
//!
//! Why this matters: the walker is the honest ruler. If it cannot
//! distinguish "villain plays uniform noise" from "villain plays the
//! trained blueprint", the metric is not responsive and every prior D1
//! claim is suspect. The plan says to check this explicitly, and no
//! prior session did.

use cham_core::card::{Card, Hand2};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::ActionSeq;
use cham_engine::ladder::ActionLadder;
use cham_search::fullgame::FullGameVbr;
use cham_search::pubtree::PublicTree;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};
const HERO_SEAT: usize = 1;

fn board_from_seed(seed: u64) -> [Card; 5] {
    let mut rng = rng_from_seed(seed);
    let mut deck: Vec<u8> = (0..52).collect();
    for i in (1..52).rev() {
        let j = (cham_core::rng::next_f64(&mut rng) * (i + 1) as f64) as usize;
        deck.swap(i, j);
    }
    [
        Card(deck[0]),
        Card(deck[1]),
        Card(deck[2]),
        Card(deck[3]),
        Card(deck[4]),
    ]
}

fn split_ranges(board: &[Card; 5], n: usize) -> (Vec<[u8; 2]>, Vec<[u8; 2]>) {
    let mut avail: Vec<u8> = Vec::new();
    for c in 0..52u8 {
        if !board.iter().any(|b| b.idx() as usize == c as usize) {
            avail.push(c);
        }
    }
    let half = avail.len() / 2;
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
    (take(&avail[..half], n), take(&avail[half..], n))
}

#[test]
#[ignore = "plan gate; run with --ignored --nocapture"]
fn uniform_vbr_far_exceeds_shipped() {
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);
    let tree = PublicTree::build(CFG, &ladder, 100_000);
    assert!(tree.len() > 1);

    let n_boards: u64 = std::env::var("CHAM_VBR_UBOARDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let n_combos: usize = 30;

    let mut values: Vec<f64> = Vec::new();
    for bseed in 0..n_boards {
        let board = board_from_seed(0xB000 + bseed);
        let (hero, vill) = split_ranges(&board, n_combos);
        if hero.len() < n_combos || vill.len() < n_combos {
            continue;
        }

        let rank = |c: &[u8; 2]| -> u32 {
            (cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &board) * 1e6)
                as u32
        };
        let hero_rank: Vec<u32> = hero.iter().map(rank).collect();
        let vill_rank: Vec<u32> = vill.iter().map(rank).collect();
        let hw = vec![1.0 / hero.len() as f64; hero.len()];
        let vw = vec![1.0 / vill.len() as f64; vill.len()];

        // Uniform policy: the villain ignores state and plays every action
        // with equal probability. The walker queries this ONLY at villain
        // nodes; hero nodes take the max.
        let mut uniform = |_st: &State,
                           _path: &[Action],
                           _seq: &ActionSeq,
                           na: usize,
                           _combo: usize|
         -> Vec<f64> {
            if na == 0 {
                Vec::new()
            } else {
                vec![1.0 / na as f64; na]
            }
        };

        let mut vbr = FullGameVbr {
            tree: &tree,
            ladder: &ladder,
            hero_range: &hero,
            hero_rank: &hero_rank,
            hero_w: &hw,
            villain_range: &vill,
            villain_rank: &vill_rank,
            villain_w: &vw,
            cfg: CFG,
            hero_seat: HERO_SEAT,
            policy: &mut uniform,
        };
        if let Some(v) = vbr.best_response(&board) {
            values.push(v);
            eprintln!("  board {bseed}: uniform-villain VBR = {:.3} bb", v);
        }
    }

    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n.max(1.0);
    let var =
        (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0)).max(0.0);
    let se = (var / n.max(1.0)).sqrt();

    eprintln!();
    eprintln!("=== VBR(uniform villain) ===");
    eprintln!("  boards:  {}", values.len());
    eprintln!("  mean:    {:.3} +/- {:.3} bb/hand", mean, se);
    eprintln!("  shipped: 5.77 +/- 0.55 bb/hand (D1, 180 boards)");
    eprintln!();
    eprintln!("  plan gate: VBR(uniform) ≫ VBR(shipped) ≫ 0");
    eprintln!("  check:     uniform is {}x the shipped", mean / 5.77);
    eprintln!();

    assert!(
        values.len() >= 3,
        "not enough boards (got {})",
        values.len()
    );
    // The plan's gate. A uniform villain should be much more exploitable
    // than a trained blueprint — the blueprint was trained to be robust,
    // uniform noise is not. Assert a clear margin, not a tight bound.
    assert!(
        mean > 2.0 * 5.77,
        "uniform-villain VBR {mean:.3} is not ≫ shipped 5.77; \
         the metric is not responsive to policy quality",
    );
    // And both must be strictly positive (the plan's "≫ 0").
    assert!(mean > 0.0, "VBR must be positive");
}
