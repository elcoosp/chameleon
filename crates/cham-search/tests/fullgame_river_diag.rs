//! Diagnostic: inspect the river-only PublicTree action slots so the
//! river-reduction test (design doc §Testing) can map them onto
//! `RiverVbr`'s fixed [check, bet-s, bet-b, jam] / [fold, call, raise]
//! conventions.
//!
//! Motivation. The design doc's river-reduction test compares
//! `FullGameVbr` on a river-only tree against `RiverVbr`. That
//! comparison requires the two walkers to agree on the action set at
//! every node. `RiverVbr` uses its own hard-coded fracs; the ladder
//! used by `PublicTree::build` uses the abstraction's slots. Before
//! writing the reduction test we need to see, concretely, what actions
//! the ladder returns at a river root and at a river facing-a-bet node.

use cham_core::card::{Card, Deck};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State, Street};
use cham_core::obs::{Observables, Player, is_legal};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::ActionSeq;
use cham_engine::ladder::ActionLadder;
use cham_search::pubtree::PublicTree;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

/// Advance a fresh state to the river via a fixed check/call line.
fn river_state(hero: [u8; 2], vill: [u8; 2], board: &[Card; 5]) -> Option<(State, ActionSeq)> {
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
    let ladder = ActionLadder::new(&AbstractionConfig::tiny());
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
        cham_engine::ladder::record_action(&ladder, &obs, Player::from_usize(p), a, &mut seq);
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
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn river_node_actions() {
    let board = [Card(2), Card(7), Card(11), Card(19), Card(23)];
    let hero = [0u8, 1];
    let vill = [3u8, 5];

    let (st, seq) = river_state(hero, vill, &board).expect("river state");
    eprintln!();
    eprintln!("=== river-only PublicTree action dump ===");
    eprintln!("st.street() = {:?}", st.street());
    eprintln!("st.pot() = {} chips (bb = {})", st.pot(), CFG.bb);
    eprintln!("st.stacks() = {:?}", st.stacks());
    eprintln!("st.current_bet() = {}", st.current_bet());
    eprintln!("st.to_act() = {}", st.to_act());

    let ladder = ActionLadder::new(&AbstractionConfig::tiny());
    let tree = PublicTree::build_from_state(st, seq, &ladder, 100_000);
    eprintln!("river tree nodes: {}", tree.len());

    let root = &tree.nodes[tree.root as usize];
    eprintln!(
        "root: player={} terminal={} n_actions={}",
        root.player,
        root.terminal,
        root.actions.len()
    );

    let p = root.player as usize;
    let obs = Observables::view(&st, Player::from_usize(p));
    let slots = ladder.slots(&obs, &seq);
    eprintln!("ladder slots at root ({}):", slots.len());
    for (i, s) in slots.iter().enumerate() {
        let real = ladder.to_real(&obs, &seq, i);
        eprintln!("  slot[{}]: {:?}  -> to_real = {:?}", i, s, real);
    }
    eprintln!("root actions in tree:");
    for (i, a) in root.actions.iter().enumerate() {
        eprintln!("  action[{}]: {:?}", i, a);
    }

    // For each root action, apply it and inspect the child.
    for (i, a) in root.actions.iter().enumerate() {
        let mut st2 = st;
        if st2.apply(*a).is_err() {
            eprintln!("child[{}] {:?} -> apply error", i, a);
            continue;
        }
        let child_id = root.children[i];
        let child = &tree.nodes[child_id as usize];
        if child.terminal {
            eprintln!(
                "child[{}] {:?} -> TERM (pot={} stacks={:?})",
                i,
                a,
                st2.pot(),
                st2.stacks()
            );
            continue;
        }
        let p2 = child.player as usize;
        let obs2 = Observables::view(&st2, Player::from_usize(p2));
        let slots2 = ladder.slots(&obs2, &seq);
        eprintln!(
            "child[{}] {:?} -> player={} st={:?} n_actions={} slots:",
            i,
            a,
            child.player,
            st2.street(),
            child.actions.len()
        );
        for (j, s) in slots2.iter().enumerate() {
            let real = ladder.to_real(&obs2, &seq, j);
            eprintln!("      slot[{}]: {:?} -> to_real = {:?}", j, s, real);
        }
        eprintln!("      tree actions:");
        for (j, a2) in child.actions.iter().enumerate() {
            eprintln!("        action[{}]: {:?}", j, a2);
        }
    }
}
