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

/// Build n disjoint combos for hero from cards [0,26) and n for villain
/// from [26,52). Max n is C(26,2) = 325 per side. The naïve
/// `[2k, 2k+1]` construction overflows 52 at n > 13 (the bench crash at
/// n = 16); enumerating pairs within a half-deck stays valid.
fn split_ranges(n: usize) -> (Vec<[u8; 2]>, Vec<[u8; 2]>) {
    assert!(n <= 325, "max 325 combos per side (C(26,2))");
    let mut hero = Vec::with_capacity(n);
    let mut vill = Vec::with_capacity(n);
    'hero: for a in 0..26u8 {
        for b in (a + 1)..26u8 {
            hero.push([a, b]);
            if hero.len() == n {
                break 'hero;
            }
        }
    }
    'vill: for a in 26..52u8 {
        for b in (a + 1)..52u8 {
            vill.push([a, b]);
            if vill.len() == n {
                break 'vill;
            }
        }
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
        // Dummies must be disjoint from the board: `Deck::with_prefix`
        // panics on a duplicate prefix entry. This is the same class of
        // bug the walk had at 22ce120 (fixed in b8d8ad4).
        let mut used = [false; 52];
        for c in &board {
            used[c.idx() as usize] = true;
        }
        let mut free = [0u8; 4];
        let mut k = 0usize;
        for cc in 0..52u8 {
            if !used[cc as usize] {
                free[k] = cc;
                k += 1;
                if k == 4 {
                    break;
                }
            }
        }
        let prefix = [
            Card(free[0]),
            Card(free[1]),
            Card(free[2]),
            Card(free[3]),
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
    for n in [4usize, 16, 64, 256] {
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
        use std::io::Write;
        let _ = std::io::stderr().flush();
    }
    eprintln!();
    eprintln!("Full range (200h / 200v) is ~50x n=4 by combination count.");
    eprintln!("Estimate s/iter at 200/side by extrapolation from the last row.");
}

#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn profile_phase_split() {
    use cham_blueprint::pcs::walk::{profile_enable, profile_take};

    let (cfg, ladder, tree) = build_tree();
    for n in [16usize, 64] {
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
        let iters = 20u64;

        profile_enable();
        let t0 = Instant::now();
        for t in 1..=iters {
            let board = sample_board(&mut rng);
            iter.run(&mut encoder, &mut table, board, t, 1.5, 0.0, 2.0);
        }
        let total_ns = t0.elapsed().as_nanos() as u64;
        let pc = profile_take();

        let other = total_ns
            .saturating_sub(pc.key_ns)
            .saturating_sub(pc.agg_ns)
            .saturating_sub(pc.terminal_ns);
        let pct = |x: u64| 100.0 * x as f64 / total_ns.max(1) as f64;

        eprintln!();
        eprintln!("=== phase split at n={n}/side, {iters} iters ===");
        eprintln!("  total       : {:>8.3} ms", total_ns as f64 / 1e6);
        eprintln!(
            "  key deriv   : {:>8.3} ms  ({:>5.1}%)",
            pc.key_ns as f64 / 1e6,
            pct(pc.key_ns)
        );
        eprintln!(
            "  aggregation : {:>8.3} ms  ({:>5.1}%)",
            pc.agg_ns as f64 / 1e6,
            pct(pc.agg_ns)
        );
        eprintln!(
            "  terminal    : {:>8.3} ms  ({:>5.1}%)",
            pc.terminal_ns as f64 / 1e6,
            pct(pc.terminal_ns)
        );
        eprintln!(
            "  other       : {:>8.3} ms  ({:>5.1}%)",
            other as f64 / 1e6,
            pct(other)
        );
        eprintln!(
            "  ms/iter     : {:>8.3}",
            total_ns as f64 / 1e6 / iters as f64
        );
    }
}

/// Compare `Encoder::key_for` cost across streets. The isolated bench
/// in `key_for_throughput` uses a preflop state, which measures only
/// the preflop bucket (`preflop_bucket(hole)`, no board). River nodes
/// call `river_equity(hole, board)` on a cache miss. If river key_for
/// is 5-10x preflop, the walk's "key deriv 60%" share is entirely
/// river-node cost, and the fix is a proper memo — not another
/// data-structure refactor.
#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn key_for_by_street() {
    use cham_core::engine::{Action, Street};

    let (cfg, ladder, _tree) = build_tree();
    let mut encoder = Encoder::cfg_only(cfg.clone()).expect("enc");

    // Board with 5 fixed cards. Preflop uses none of them; river uses all 5.
    let board = [Card(40), Card(41), Card(42), Card(43), Card(44)];
    // Prefix: seat0_h1, seat1_h1, seat0_h2, seat1_h2, board...
    let prefix = [
        Card(2),
        Card(3),
        Card(4),
        Card(5),
        board[0],
        board[1],
        board[2],
        board[3],
        board[4],
    ];

    // Fresh state — check/call to each street. We do NOT need the walk,
    // only a State at each street.
    let st_pre = State::new(CFG, Deck::with_prefix(&prefix)).expect("pre");
    assert_eq!(st_pre.street(), Street::Preflop);

    // Advance to flop: SB Call, BB Check.
    let mut st_flop = st_pre;
    st_flop.apply(Action::Call).expect("sb call");
    st_flop.apply(Action::Check).expect("bb check");
    assert_eq!(st_flop.street(), Street::Flop);

    // Advance to turn.
    let mut st_turn = st_flop;
    st_turn.apply(Action::Check).expect("sb check");
    st_turn.apply(Action::Check).expect("bb check");
    assert_eq!(st_turn.street(), Street::Turn);

    // Advance to river.
    let mut st_riv = st_turn;
    st_riv.apply(Action::Check).expect("sb check");
    st_riv.apply(Action::Check).expect("bb check");
    assert_eq!(st_riv.street(), Street::River);

    let n_holes = 40usize;
    let iters = 200_000usize;

    for (label, st) in [
        ("preflop", &st_pre),
        ("flop", &st_flop),
        ("turn", &st_turn),
        ("river", &st_riv),
    ] {
        let obs = Observables::view(st, Player::from_usize(0));
        let seq = ActionSeq::default();
        let slots = ladder.slots(&obs, &seq);

        // WARM the caches by hashing every combo once, then measure.
        // This isolates the "steady-state" (repeated lookups) from the
        // "cold" (first-time) cost, since the walk re-derives keys for
        // the same combos across iterations and should benefit from the
        // per-encoder eq_cache / fallback_cache.
        let mut warm = 0u64;
        for i in 0..n_holes {
            let a = (i % 40) as u8;
            let b = ((i + 1) % 40) as u8;
            let hole = Hand2::new(Card(a), Card(b));
            let obs_i = obs.with_hole(hole);
            warm ^= encoder.key_for(&obs_i, &seq, &slots).0;
        }
        let _ = warm;

        let t0 = Instant::now();
        let mut sink = 0u64;
        for i in 0..iters {
            let a = (i % n_holes) as u8;
            let b = ((i + 1) % n_holes) as u8;
            let hole = Hand2::new(Card(a), Card(b));
            let obs_i = obs.with_hole(hole);
            sink ^= encoder.key_for(&obs_i, &seq, &slots).0;
        }
        let dt = t0.elapsed().as_secs_f64();
        eprintln!(
            "key_for[{:>7}]: {:>10.0} keys/s  ({:>8.3} ns/key, sink={:#x})",
            label,
            iters as f64 / dt,
            dt * 1e9 / iters as f64,
            sink
        );
    }

    // Same sweep but WITHOUT the warm pass, to show cold cost.
    eprintln!();
    eprintln!("(cold, fresh encoder per street)");
    for (label, st) in [("preflop", &st_pre), ("river", &st_riv)] {
        let mut cold = Encoder::cfg_only(cfg.clone()).expect("enc");
        let obs = Observables::view(st, Player::from_usize(0));
        let seq = ActionSeq::default();
        let slots = ladder.slots(&obs, &seq);
        let t0 = Instant::now();
        let mut sink = 0u64;
        // 2000 distinct hole pairs (no repeat → cache never helps)
        for i in 0..2000usize {
            let a = ((i * 7) % 52) as u8;
            let b = (((i * 7) + 13) % 52) as u8;
            if a == b {
                continue;
            }
            let hole = Hand2::new(Card(a), Card(b));
            let obs_i = obs.with_hole(hole);
            sink ^= cold.key_for(&obs_i, &seq, &slots).0;
        }
        let dt = t0.elapsed().as_secs_f64();
        eprintln!(
            "cold key_for[{:>7}]: {:.3} us/key  (sink={:#x})",
            label,
            dt * 1e6 / 2000.0,
            sink
        );
    }
}

/// Decompose the walk's key-derivation block into its sub-costs, using
/// the walk's exact access pattern (many combos at one node, wide range).
/// Runs five "stages" that cumulatively add one operation; the deltas
/// between consecutive stages are the component costs.
///
/// No instrumentation of the walk itself, so no timer-in-timer overhead.
#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn key_loop_breakdown() {
    let (cfg, ladder, _tree) = build_tree();
    let mut encoder = Encoder::cfg_only(cfg.clone()).expect("enc");

    let board = [Card(40), Card(41), Card(42), Card(43), Card(44)];
    let prefix = [
        Card(2),
        Card(3),
        Card(4),
        Card(5),
        board[0],
        board[1],
        board[2],
        board[3],
        board[4],
    ];
    let st = State::new(CFG, Deck::with_prefix(&prefix)).expect("state");
    let obs_base = Observables::view(&st, Player::from_usize(0));
    let seq = ActionSeq::default();
    let slots = ladder.slots(&obs_base, &seq);

    // Match the walk: 64 combos, wide enough that any size effect shows.
    let n = 64usize;
    let mut combos: Vec<[u8; 2]> = Vec::with_capacity(n);
    'outer: for a in 0..26u8 {
        for b in (a + 1)..26u8 {
            combos.push([a, b]);
            if combos.len() == n {
                break 'outer;
            }
        }
    }
    assert_eq!(combos.len(), n, "not enough combos in half-deck");
    let iters = 20_000usize;

    // Pre-build the table with rows for every combo's key so `row_mut`
    // and `current_strategy` operate on populated rows. This mirrors
    // the walk's steady state (all keys inserted by iteration 1).
    let mut table = RegretTable::new();
    for c in &combos {
        let hole = Hand2::new(Card(c[0]), Card(c[1]));
        let obs_i = obs_base.with_hole(hole);
        let k = encoder.key_for(&obs_i, &seq, &slots).0;
        table.row_mut(k, 4);
    }

    let mut scratch_keys = vec![0u64; n];
    let mut scratch_strat: Vec<Vec<f64>> = (0..n).map(|_| vec![0.0; 4]).collect();

    // Stage 0: build holes only (baseline allocation pattern).
    let t0 = Instant::now();
    let mut sink0 = 0u64;
    for _ in 0..iters {
        for i in 0..n {
            let hole = Hand2::new(Card(combos[i][0]), Card(combos[i][1]));
            sink0 = sink0.wrapping_add(hole.0 as u64);
        }
    }
    let s0 = t0.elapsed().as_nanos() as u64;

    // Stage 1: holes + with_hole
    let t1 = Instant::now();
    let mut sink1 = 0u64;
    for _ in 0..iters {
        for i in 0..n {
            let hole = Hand2::new(Card(combos[i][0]), Card(combos[i][1]));
            let oi = obs_base.with_hole(hole);
            sink1 = sink1.wrapping_add(oi.hole.0 as u64);
        }
    }
    let s1 = t1.elapsed().as_nanos() as u64;

    // Stage 2: + key_for
    let t2 = Instant::now();
    let mut sink2 = 0u64;
    for _ in 0..iters {
        for i in 0..n {
            let hole = Hand2::new(Card(combos[i][0]), Card(combos[i][1]));
            let oi = obs_base.with_hole(hole);
            sink2 ^= encoder.key_for(&oi, &seq, &slots).0;
        }
    }
    let s2 = t2.elapsed().as_nanos() as u64;

    // Stage 3: + row_mut (with keys written to scratch)
    let t3 = Instant::now();
    let mut sink3 = 0u64;
    for _ in 0..iters {
        for i in 0..n {
            let hole = Hand2::new(Card(combos[i][0]), Card(combos[i][1]));
            let oi = obs_base.with_hole(hole);
            let k = encoder.key_for(&oi, &seq, &slots).0;
            scratch_keys[i] = k;
            let row = table.row_mut(k, 4);
            sink3 = sink3.wrapping_add(row.visits);
        }
    }
    let s3 = t3.elapsed().as_nanos() as u64;

    // Stage 4: + current_strategy (full block, but into scratch Vecs)
    let t4 = Instant::now();
    let mut sink4 = 0u64;
    for _ in 0..iters {
        for i in 0..n {
            let hole = Hand2::new(Card(combos[i][0]), Card(combos[i][1]));
            let oi = obs_base.with_hole(hole);
            let k = encoder.key_for(&oi, &seq, &slots).0;
            scratch_keys[i] = k;
            let s = table.row_mut(k, 4).current_strategy();
            scratch_strat[i].clear();
            scratch_strat[i].extend_from_slice(&s);
            sink4 = sink4.wrapping_add(s.len() as u64);
        }
    }
    let s4 = t4.elapsed().as_nanos() as u64;

    let _ = (sink0, sink1, sink2, sink3, sink4);

    let per = |total: u64| -> f64 { total as f64 / (iters * n) as f64 };
    let n_ns =
        |prev: u64, cur: u64| -> f64 { (cur.saturating_sub(prev)) as f64 / (iters * n) as f64 };

    eprintln!();
    eprintln!(
        "=== key-loop component breakdown ({} combos, {} iters) ===",
        n, iters
    );
    eprintln!(
        "  stage 0 (hole only)            : {:>7.1} ns/combo",
        per(s0)
    );
    eprintln!(
        "  stage 1 (+ with_hole)          : {:>7.1} ns/combo  (+{:.1})",
        per(s1),
        n_ns(s0, s1)
    );
    eprintln!(
        "  stage 2 (+ key_for)            : {:>7.1} ns/combo  (+{:.1})",
        per(s2),
        n_ns(s1, s2)
    );
    eprintln!(
        "  stage 3 (+ row_mut)            : {:>7.1} ns/combo  (+{:.1})",
        per(s3),
        n_ns(s2, s3)
    );
    eprintln!(
        "  stage 4 (+ current_strategy)   : {:>7.1} ns/combo  (+{:.1})",
        per(s4),
        n_ns(s3, s4)
    );
    eprintln!("  delta s1-s0 = with_hole cost   : {:.1} ns", n_ns(s0, s1));
    eprintln!("  delta s2-s1 = key_for cost     : {:.1} ns", n_ns(s1, s2));
    eprintln!("  delta s3-s2 = row_mut cost     : {:.1} ns", n_ns(s2, s3));
    eprintln!("  delta s4-s3 = cur_strat cost   : {:.1} ns", n_ns(s3, s4));
}
