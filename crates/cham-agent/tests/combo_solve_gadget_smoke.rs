//! Integration smoke test for `try_solve_combo_gadget` — the combo-level
//! solver wired with a real blueprint-sized safe-resolving gadget.
//!
//! Requires the shipped bundle. Run with:
//!
//!     cargo test -p cham-agent --test combo_solve_gadget_smoke --no-run
//!     CHAM_D1_BP=$PWD/artifacts/agent-honest-19dim/robust \
//!     CHAM_D1_BUCKETS=$PWD/artifacts/agent-honest-19dim/buckets \
//!       target/debug/deps/combo_solve_gadget_smoke-<hash> \
//!         --ignored --nocapture
//!
//! IMPORTANT: `CHAM_SLOT_BUCKET` must be UNSET for agent-honest-19dim
//! (bundle was trained without it; setting it corrupts every key).

use cham_agent::search_bridge::{SearchBridgeCfg, try_solve_combo_gadget};
use cham_blueprint::policy::BlueprintPolicy;
use cham_core::card::{Card, Deck};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State, Street};
use cham_core::obs::{Observables, Player, is_legal};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};
use cham_engine::ladder::ActionLadder;
use cham_search::trigger::SolverChoice;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

fn river_state_and_seq(ladder: &ActionLadder) -> Option<(State, ActionSeq)> {
    let board = [Card(40), Card(41), Card(42), Card(43), Card(44)];
    let hero = [10u8, 11]; // BB (seat 1)
    let vill = [20u8, 21]; // SB (seat 0)
    let prefix = [
        Card(vill[0]),
        Card(hero[0]),
        Card(vill[1]),
        Card(hero[1]),
        board[0],
        board[1],
        board[2],
        board[3],
        board[4],
    ];
    let mut st = State::new(CFG, Deck::with_prefix(&prefix)).ok()?;
    let mut seq = ActionSeq::default();
    let mut guard = 0;
    while st.street() != Street::River && !st.is_terminal() && guard < 30 {
        guard += 1;
        let p = st.to_act();
        let obs = Observables::view(&st, Player::from_usize(p));
        let a = if is_legal(&obs, Action::Check) {
            Action::Check
        } else {
            Action::Call
        };
        cham_engine::ladder::record_action(ladder, &obs, Player::from_usize(p), a, &mut seq);
        st.apply(a).ok()?;
    }
    if st.street() == Street::River {
        Some((st, seq))
    } else {
        None
    }
}

#[test]
#[ignore = "integration smoke; needs shipped bundle"]
fn combo_gadget_returns_valid_outcome() {
    let bp_dir = std::env::var("CHAM_D1_BP")
        .unwrap_or_else(|_| "artifacts/agent-honest-19dim/robust".into());
    let policy =
        BlueprintPolicy::load(std::path::Path::new(&bp_dir), 0).expect("load BlueprintPolicy");

    let bundle = std::path::Path::new(&bp_dir).parent().expect("bundle dir");
    let bk = std::env::var("CHAM_D1_BUCKETS")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| bundle.join("buckets"));
    let cfg_path = bundle.join("abstraction.toml");
    let cfg = std::fs::read_to_string(&cfg_path)
        .ok()
        .and_then(|t| cham_engine::config::parse_config(&t).ok())
        .unwrap_or_else(AbstractionConfig::tiny);
    let encoder = Encoder::from_artifacts_dir(&bk, cfg.clone())
        .unwrap_or_else(|_| Encoder::cfg_only(cfg.clone()).expect("cfg_only"));

    let ladder = ActionLadder::new(&cfg);
    let (st, seq) = river_state_and_seq(&ladder).expect("river state");
    assert_eq!(st.to_act(), 1, "BB should act first postflop");
    let obs = Observables::view(&st, Player::from_usize(1));

    let bridge_cfg = SearchBridgeCfg {
        enabled: true,
        solver: SolverChoice::ReachGadget,
        iters: 400,
        min_pot_bb: 0.5,
        river_only: true,
    };
    // Symmetric 3-class fallback (same shape the tracker uses when it
    // has no history).
    let villain_classes: Vec<(f64, f64)> =
        vec![(1.0 / 3.0, 0.20), (1.0 / 3.0, 0.50), (1.0 / 3.0, 0.80)];

    let outcome = try_solve_combo_gadget(
        &bridge_cfg,
        &ladder,
        &obs,
        &seq,
        &st,
        &villain_classes,
        &policy,
        &encoder,
    );
    let Some(out) = outcome else {
        panic!("try_solve_combo_gadget returned None at a valid river root");
    };
    eprintln!(
        "gadget solve: action={:?} solver={} iters={}",
        out.action, out.solver, out.iters
    );
    eprintln!("distribution: {:?}", out.distribution);

    assert!(!out.distribution.is_empty(), "empty distribution");
    let total: f64 = out.distribution.iter().map(|(_, p)| p).sum();
    assert!(
        (total - 1.0).abs() < 1e-6,
        "distribution does not sum to 1: total={total}"
    );
    for (_, p) in &out.distribution {
        assert!((0.0..=1.0).contains(p), "prob out of range: {p}");
    }
    assert!(
        is_legal(&obs, out.action),
        "chosen action {:?} is not engine-legal",
        out.action
    );
    assert!(
        out.solver.contains("combo-cfr-gadget"),
        "solver tag should identify the gadget path: {}",
        out.solver
    );
}
