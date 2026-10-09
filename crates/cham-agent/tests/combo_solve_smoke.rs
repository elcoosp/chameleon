//! Smoke test for `try_solve_combo` — the combo-level river wiring.

use cham_agent::search_bridge::{SearchBridgeCfg, try_solve_combo};
use cham_core::card::{Card, Deck};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State, Street};
use cham_core::obs::{Observables, Player, is_legal};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::ActionSeq;
use cham_engine::ladder::ActionLadder;
use cham_search::trigger::SolverChoice;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

fn build_river() -> Option<(State, ActionSeq)> {
    // Board and hole cards fixed so the test is deterministic.
    let board = [Card(40), Card(41), Card(42), Card(43), Card(44)];
    let hero = [10u8, 11]; // BB (seat 1)
    let vill = [20u8, 21]; // SB (seat 0)

    // Deck order for `with_prefix`: [s0a, s1a, s0b, s1b, board...]
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
    let seq = ActionSeq::default();
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
        if st.apply(a).is_err() {
            return None;
        }
    }
    if st.street() == Street::River {
        Some((st, seq))
    } else {
        None
    }
}

#[test]
fn combo_solve_returns_valid_outcome() {
    let (st, seq) = build_river().expect("river state");
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);

    // Hero (BB, seat 1) acts first postflop. That should be the root.
    assert_eq!(st.to_act(), 1, "test setup: BB should act first at river");

    let obs = Observables::view(&st, Player::from_usize(1));

    let bridge_cfg = SearchBridgeCfg {
        enabled: true,
        solver: SolverChoice::ReachGadget,
        iters: 200,
        min_pot_bb: 0.5,
        river_only: true,
        impl_kind: cham_agent::search_bridge::SolverImpl::default(),
    };

    // Symmetric 3-class villain range (the same fallback the tracker uses
    // when it has no history).
    let villain_classes: Vec<(f64, f64)> =
        vec![(1.0 / 3.0, 0.20), (1.0 / 3.0, 0.50), (1.0 / 3.0, 0.80)];

    let outcome = try_solve_combo(&bridge_cfg, &ladder, &obs, &seq, &st, &villain_classes);
    let Some(out) = outcome else {
        panic!("try_solve_combo returned None at a valid river root");
    };

    eprintln!(
        "combo solve: action={:?} iters={} solver={}",
        out.action, out.iters, out.solver
    );
    eprintln!("distribution: {:?}", out.distribution);

    // Distribution must be a valid probability vector.
    assert!(!out.distribution.is_empty(), "empty distribution");
    let total: f64 = out.distribution.iter().map(|(_, p)| p).sum();
    assert!(
        (total - 1.0).abs() < 1e-6,
        "distribution does not sum to 1: total={total}"
    );
    for (_, p) in &out.distribution {
        assert!((0.0..=1.0).contains(p), "prob out of range: {p}");
    }

    // The chosen action must be engine-legal.
    assert!(
        is_legal(&obs, out.action),
        "chosen action {:?} is not engine-legal",
        out.action
    );
}

#[test]
fn combo_solve_refuses_when_disabled() {
    let (st, seq) = build_river().expect("river state");
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);
    let obs = Observables::view(&st, Player::from_usize(1));

    let bridge_cfg = SearchBridgeCfg {
        enabled: false,
        solver: SolverChoice::ReachGadget,
        iters: 200,
        min_pot_bb: 0.5,
        river_only: true,
        impl_kind: cham_agent::search_bridge::SolverImpl::default(),
    };
    let villain_classes: Vec<(f64, f64)> = vec![(1.0, 0.5)];
    let out = try_solve_combo(&bridge_cfg, &ladder, &obs, &seq, &st, &villain_classes);
    assert!(out.is_none(), "should refuse when disabled");
}

#[test]
fn combo_solve_refuses_preflop() {
    let prefix = [
        Card(2),
        Card(3),
        Card(4),
        Card(5),
        Card(40),
        Card(41),
        Card(42),
        Card(43),
        Card(44),
    ];
    let st = State::new(CFG, Deck::with_prefix(&prefix)).expect("state");
    let seq = ActionSeq::default();
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);
    let obs = Observables::view(&st, Player::from_usize(st.to_act()));

    let bridge_cfg = SearchBridgeCfg {
        enabled: true,
        solver: SolverChoice::ReachGadget,
        iters: 200,
        min_pot_bb: 0.5,
        river_only: true,
        impl_kind: cham_agent::search_bridge::SolverImpl::default(),
    };
    let villain_classes: Vec<(f64, f64)> = vec![(1.0, 0.5)];
    let out = try_solve_combo(&bridge_cfg, &ladder, &obs, &seq, &st, &villain_classes);
    assert!(out.is_none(), "should refuse preflop");
}
