//! Diagnostic: where does the ~1 s/call in `try_solve_combo_gadget` go?
//!
//! Measures: (a) clone cost, (b) `strategy()` cost warm vs cold,
//! (c) tree build cost. Not a pass/fail test.

use cham_blueprint::policy::BlueprintPolicy;
use cham_core::card::{Card, Deck, Hand2};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State, Street};
use cham_core::obs::{Observables, Player, is_legal};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};
use cham_engine::ladder::ActionLadder;
use cham_search::pubtree::PublicTree;
use std::path::Path;
use std::time::Instant;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

#[test]
#[ignore = "diagnostic; needs bundle"]
fn where_does_the_time_go() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let b = root.join("artifacts/agent-honest-19dim");
    let cfg = std::fs::read_to_string(b.join("abstraction.toml"))
        .ok()
        .and_then(|t| cham_engine::config::parse_config(&t).ok())
        .unwrap_or_else(AbstractionConfig::tiny);
    let encoder = Encoder::from_artifacts_dir(&b.join("buckets"), cfg.clone())
        .unwrap_or_else(|_| Encoder::cfg_only(cfg.clone()).expect("enc"));
    let robust = BlueprintPolicy::load(&b.join("robust"), 0).expect("load");
    let ladder = ActionLadder::new(&cfg);

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
    let mut st = State::new(CFG, Deck::with_prefix(&prefix)).expect("state");
    let mut seq = ActionSeq::default();
    while st.street() != Street::River && !st.is_terminal() {
        let p = st.to_act();
        let obs = Observables::view(&st, Player::from_usize(p));
        let a = if is_legal(&obs, Action::Check) {
            Action::Check
        } else {
            Action::Call
        };
        cham_engine::ladder::record_action(&ladder, &obs, Player::from_usize(p), a, &mut seq);
        st.apply(a).expect("apply");
    }

    // 1. Tree build cost.
    let t = Instant::now();
    let tree = PublicTree::build_from_state(st, seq, &ladder, 200_000);
    let dt_build = t.elapsed().as_secs_f64();
    eprintln!("tree build ({} nodes): {:.4}s", tree.len(), dt_build);

    // 2. Encoder clone cost (cold encoder, small cache).
    let t = Instant::now();
    let n = 100;
    for _ in 0..n {
        let _ = encoder.clone();
    }
    let dt_clone = t.elapsed().as_secs_f64() / n as f64;
    eprintln!("encoder clone (cold): {:.6}s each", dt_clone);

    // 3. strategy() call cost warm vs cold.
    let obs = Observables::view(&st, Player::from_usize(1));
    let mut enc_warm = encoder.clone();
    // Prime it: hash the 45-combo range once.
    let range: Vec<[u8; 2]> = (0..15).map(|k| [10 + k as u8, 30 + k as u8]).collect();
    for c in &range {
        let h = Hand2::new(Card(c[0]), Card(c[1]));
        let o = obs.with_hole(h);
        let _ = robust.strategy(&o, &mut enc_warm, &seq);
    }
    let t = Instant::now();
    let n = 100;
    for _ in 0..n {
        for c in &range {
            let h = Hand2::new(Card(c[0]), Card(c[1]));
            let o = obs.with_hole(h);
            let _ = robust.strategy(&o, &mut enc_warm, &seq);
        }
    }
    let dt_warm = t.elapsed().as_secs_f64() / (n * range.len()) as f64;
    eprintln!(
        "strategy() warm: {:.6}s each ({:.3}us)",
        dt_warm,
        dt_warm * 1e6
    );

    // Cold: fresh clone each time.
    let t = Instant::now();
    let n2 = 20;
    for _ in 0..n2 {
        for c in &range {
            let mut enc = encoder.clone();
            let h = Hand2::new(Card(c[0]), Card(c[1]));
            let o = obs.with_hole(h);
            let _ = robust.strategy(&o, &mut enc, &seq);
        }
    }
    let dt_cold = t.elapsed().as_secs_f64() / (n2 * range.len()) as f64;
    eprintln!(
        "strategy() cold (clone+call): {:.6}s each ({:.3}us)",
        dt_cold,
        dt_cold * 1e6
    );

    eprintln!();
    eprintln!("=== composition for one pipeline call (45 combos, ~200 nodes) ===");
    let nodes = tree.len() as f64;
    let combos = 45.0;
    let est_warm = dt_build + nodes * combos * dt_warm;
    let est_cold = dt_build + nodes * combos * dt_cold;
    eprintln!("  tree build:           {:.3}s", dt_build);
    eprintln!("  strategy calls (warm): {:.3}s", nodes * combos * dt_warm);
    eprintln!("  strategy calls (cold): {:.3}s", nodes * combos * dt_cold);
    eprintln!("  ESTIMATE warm total:  {:.3}s", est_warm);
    eprintln!("  ESTIMATE cold total:  {:.3}s", est_cold);
    eprintln!("  (measured actual is ~1.0s)");
    eprintln!(
        "  dominant: {}",
        if est_cold > 2.0 * dt_build {
            "strategy calls (clone)"
        } else {
            "tree build"
        }
    );
}
