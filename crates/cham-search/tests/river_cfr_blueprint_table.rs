//! Test the blueprint strategy table machinery. Synthetic uniform
//! policy — no bundle, no encoder, no env vars.

use cham_core::card::{Card, Deck};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State, Street};
use cham_core::obs::{Observables, Player, is_legal};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::ActionSeq;
use cham_engine::ladder::ActionLadder;
use cham_search::pubtree::PublicTree;
use cham_search::river_cfr::build_blueprint_strategy_table;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

fn river_state_and_seq(ladder: &ActionLadder) -> (State, ActionSeq) {
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
        st.apply(a).expect("apply");
    }
    assert_eq!(st.street(), Street::River, "did not reach river");
    (st, seq)
}

#[test]
fn table_shape_and_distributions_valid() {
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);
    let (st, seq) = river_state_and_seq(&ladder);
    let tree = PublicTree::build_from_state(st, seq, &ladder, 100_000);

    let hero: Vec<[u8; 2]> = vec![[10, 11], [12, 13]];
    let villain: Vec<[u8; 2]> = vec![[14, 15], [16, 17]];

    let policy = |obs: &Observables<'_>, seq: &ActionSeq| -> Option<Vec<f64>> {
        let slots = ladder.slots(obs, seq);
        let k = slots.len();
        if k == 0 {
            None
        } else {
            Some(vec![1.0 / k as f64; k])
        }
    };

    let (hs, vs) =
        build_blueprint_strategy_table(&tree, st, seq, &ladder, &hero, &villain, 1, policy);

    let mut checked = 0usize;
    for (node_idx, nd) in tree.nodes.iter().enumerate() {
        if nd.terminal {
            assert!(hs[node_idx].is_empty(), "terminal hero row at {node_idx}");
            assert!(
                vs[node_idx].is_empty(),
                "terminal villain row at {node_idx}"
            );
            continue;
        }
        let is_hero = (nd.player as usize) == 1;
        let (tab, ncombos) = if is_hero {
            (&hs[node_idx], hero.len())
        } else {
            (&vs[node_idx], villain.len())
        };
        assert_eq!(
            tab.len(),
            ncombos,
            "node {node_idx}: combos {} vs {ncombos}",
            tab.len()
        );
        for (i, row) in tab.iter().enumerate() {
            assert_eq!(
                row.len(),
                nd.actions.len(),
                "node {node_idx} combo {i}: width"
            );
            let s: f64 = row.iter().sum();
            assert!((s - 1.0).abs() < 1e-9, "node {node_idx} combo {i}: sum {s}");
            for &p in row {
                assert!((0.0..=1.0).contains(&p), "prob out of range: {p}");
            }
        }
        checked += 1;
    }
    assert!(checked > 0, "no non-terminal nodes checked");
    eprintln!("table shape valid for {checked} non-terminal nodes");
}

#[test]
fn policy_none_falls_back_to_uniform() {
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);
    let (st, seq) = river_state_and_seq(&ladder);
    let tree = PublicTree::build_from_state(st, seq, &ladder, 100_000);

    let hero: Vec<[u8; 2]> = vec![[10, 11]];
    let villain: Vec<[u8; 2]> = vec![[14, 15]];

    let policy = |_: &Observables<'_>, _: &ActionSeq| -> Option<Vec<f64>> { None };
    let (hs, vs) =
        build_blueprint_strategy_table(&tree, st, seq, &ladder, &hero, &villain, 1, policy);

    for (node_idx, nd) in tree.nodes.iter().enumerate() {
        if nd.terminal {
            continue;
        }
        let na = nd.actions.len();
        let expected = 1.0 / na as f64;
        let is_hero = (nd.player as usize) == 1;
        let tab = if is_hero {
            &hs[node_idx]
        } else {
            &vs[node_idx]
        };
        for row in tab {
            for &p in row {
                assert!(
                    (p - expected).abs() < 1e-12,
                    "node {node_idx}: want {expected}, got {p}"
                );
            }
        }
    }
}
