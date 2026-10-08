//! Diagnostic benchmark for the PCS walk. Identifies the dominant cost
//! at the current commit. Run with:
//!
//!     cargo test -p cham-blueprint --test pcs_walk_bench --no-run
//!     target/debug/deps/pcs_walk_bench-<hash> --ignored --nocapture
//!
//! Report the printed timings; the numbers guide the optimization.

use cham_blueprint::pcs::sampling::sample_board;
use cham_blueprint::pcs::table::RegretTable;
use cham_blueprint::pcs::walk::PcsIteration;
use cham_core::card::{Card, Deck, Hand2};
use cham_core::engine::State;
use cham_core::engine::config::EngineConfig;
use cham_core::obs::{Observables, Player};
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};
use cham_engine::ladder::ActionLadder;
use cham_search::pubtree::PublicTree;
use std::time::Instant;

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

fn build_tree() -> (AbstractionConfig, ActionLadder, PublicTree) {
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);
    let tree = PublicTree::build(CFG, &ladder, 100_000);
    (cfg, ladder, tree)
}

/// How fast is a single `Encoder::key_for` call? At each of `n` combos,
/// build `Observables::with_hole` and hash. This is the unit the walk
/// repeats at every non-terminal node.
#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn key_for_throughput() {
    let (cfg, ladder, _tree) = build_tree();
    let mut encoder = Encoder::cfg_only(cfg.clone()).expect("enc");

    // Preflop state, wide-open action set (4 slots).
    let b = [Card(40), Card(41), Card(42), Card(43), Card(44)];
    let prefix = [
        Card(0),
        Card(1),
        Card(2),
        Card(3),
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
    ];
    let st = State::new(CFG, Deck::with_prefix(&prefix)).expect("state");
    let obs = Observables::view(&st, Player::from_usize(0));
    let seq = ActionSeq::default();
    let slots = ladder.slots(&obs, &seq);

    let n_holes = 40usize;
    let iters = 200_000usize;

    // Key derivation alone.
    let t0 = Instant::now();
    let mut sink = 0u64;
    for i in 0..iters {
        let a = (i % n_holes) as u8;
        let b2 = ((i + 1) % n_holes) as u8;
        let hole = Hand2::new(Card(a), Card(b2));
        let obs_i = obs.with_hole(hole);
        sink ^= encoder.key_for(&obs_i, &seq, &slots).0;
    }
    let dt = t0.elapsed().as_secs_f64();
    eprintln!(
        "key_for: {:.0} keys/s  ({:.3} us/key, sink={:#x})",
        iters as f64 / dt,
        dt * 1e6 / iters as f64,
        sink
    );

    // Bucket alone (no FNV). Same input distribution.
    let mut encoder2 = Encoder::cfg_only(cfg.clone()).expect("enc");
    let t1 = Instant::now();
    let mut sink2 = 0u16;
    for i in 0..iters {
        let a = (i % n_holes) as u8;
        let b2 = ((i + 1) % n_holes) as u8;
        let hole = Hand2::new(Card(a), Card(b2));
        let obs_i = obs.with_hole(hole);
        sink2 = sink2.wrapping_add(encoder2.bucket(&obs_i));
    }
    let dt1 = t1.elapsed().as_secs_f64();
    eprintln!(
        "bucket : {:.0} buckets/s ({:.3} us/bucket, sink={})",
        iters as f64 / dt1,
        dt1 * 1e6 / iters as f64,
        sink2
    );
}

/// How fast is `State::new` + a fresh deal from a random board? The walk
/// constructs one per iteration.
#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn state_construction_throughput() {
    let iters = 50_000usize;
    let mut rng = rng_from_seed(0xAB);
    let t0 = Instant::now();
    let mut sink = 0i64;
    for _ in 0..iters {
        let board = sample_board(&mut rng);
        let prefix = [
            Card(0),
            Card(1),
            Card(2),
            Card(3),
            board[0],
            board[1],
            board[2],
            board[3],
            board[4],
        ];
        let st = State::new(CFG, Deck::with_prefix(&prefix)).expect("state");
        sink = sink.wrapping_add(st.pot());
    }
    let dt = t0.elapsed().as_secs_f64();
    eprintln!(
        "State::new: {:.0} states/s ({:.3} us/state, sink={})",
        iters as f64 / dt,
        dt * 1e6 / iters as f64,
        sink
    );
}

/// Full walk throughput at several range sizes. This is the number that
/// decides whether PCS training is feasible.
#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn walk_throughput() {
    let (cfg, ladder, tree) = build_tree();
    eprintln!();
    eprintln!("=== walk throughput (all preflop actions) ===");
    eprintln!(
        "{:>6}  {:>10}  {:>10}  {:>10}",
        "n/side", "s/iter", "iter/s", "rows"
    );
    for n in [4usize, 8, 16, 30] {
        let (hero, vill) = split_ranges(n);
        let hero_rank: Vec<u32> = hero
            .iter()
            .map(|c| (c[0] as u32) << 8 | c[1] as u32)
            .collect();
        let vill_rank: Vec<u32> = vill
            .iter()
            .map(|c| (c[0] as u32) << 8 | c[1] as u32)
            .collect();
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
        let mut encoder = Encoder::cfg_only(cfg.clone()).expect("enc");
        let mut table = RegretTable::new();
        let mut rng = rng_from_seed(0x42);
        let iters = 3u64;
        let t0 = Instant::now();
        for t in 1..=iters {
            let board = sample_board(&mut rng);
            iter.run(&mut encoder, &mut table, board, t, 1.5, 0.0, 2.0);
        }
        let dt = t0.elapsed().as_secs_f64();
        eprintln!(
            "{:>6}  {:>10.4}  {:>10.1}  {:>10}",
            n,
            dt / iters as f64,
            iters as f64 / dt,
            table.len()
        );
    }
    eprintln!();
    eprintln!("Full range (200h / 200v) is ~50x n=4 by combination count.");
    eprintln!("Estimate s/iter at 200/side by extrapolation from the last row.");
}
