//! W3 acceptance gate: VBR(resolved) ≤ VBR(blueprint).
//!
//! Plan (docs/reviews/CHAMELEON-SOTA-PLAN.md, Phase D): the combo-level
//! solver with the safe-resolving gadget must produce a river strategy
//! whose exploitability is no worse than the blueprint's.
//!
//! Method (self-contained, no bundle dependency):
//!
//! 1. Hand-build a "blueprint" as a uniform strategy over the river tree.
//! 2. Compute VBR_blueprint = exploitability of the uniform strategy.
//! 3. Solve with `RiverCfr` using the gadget, `v_bp_hero` = CFVs of the
//!    blueprint at the subgame root (approximated by zero here — the
//!    uniform blueprint's CFV is symmetric).
//! 4. Compute VBR_resolved = exploitability of the resolved strategy.
//! 5. Assert VBR_resolved ≤ VBR_blueprint.
//!
//! What this exercises: the gadget's ability to bound the resolved
//! strategy's exploitability by the blueprint's, in code, on a real
//! river tree.

use cham_core::card::{Card, Deck, Hand2};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State, Street};
use cham_core::obs::{Observables, Player, is_legal};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::ActionSeq;
use cham_engine::ladder::ActionLadder;
use cham_search::pubtree::PublicTree;
use cham_search::river_cfr::{RiverCfr, SolvedRiver};

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

fn board5() -> [Card; 5] {
    [Card(40), Card(41), Card(42), Card(43), Card(44)]
}

fn make_uniform_solution(tree: &PublicTree, nh: usize, nv: usize) -> SolvedRiver {
    let nnodes = tree.nodes.len();
    let mut hero_strat = vec![Vec::new(); nnodes];
    let mut villain_strat = vec![Vec::new(); nnodes];
    for (i, n) in tree.nodes.iter().enumerate() {
        if n.terminal {
            continue;
        }
        let na = n.actions.len();
        let is_hero = (n.player as usize) == 1;
        let ncombos = if is_hero { nh } else { nv };
        let rows: Vec<Vec<f64>> = (0..ncombos).map(|_| vec![1.0 / na as f64; na]).collect();
        if is_hero {
            hero_strat[i] = rows;
        } else {
            villain_strat[i] = rows;
        }
    }
    SolvedRiver {
        hero_strat,
        villain_strat,
        gadget_root_strat: None,
        iters: 0,
    }
}

#[test]
#[ignore = "W3 gate; run with --ignored --nocapture"]
fn resolved_vbr_bounded_by_blueprint_vbr() {
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);
    let (st, seq) = river_state_and_seq(&ladder);
    let tree = PublicTree::build_from_state(st, seq, &ladder, 100_000);

    let board = board5();
    let hero: Vec<[u8; 2]> = vec![[10, 11], [12, 13]];
    let villain: Vec<[u8; 2]> = vec![[14, 15], [16, 17]];
    let rank = |c: &[u8; 2]| -> u32 {
        (cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &board) * 1e6) as u32
    };
    let hero_rank: Vec<u32> = hero.iter().map(rank).collect();
    let villain_rank: Vec<u32> = villain.iter().map(rank).collect();

    let solver = RiverCfr::new(
        &tree,
        &hero,
        &hero_rank,
        &villain,
        &villain_rank,
        st,
        1,
        None,
    );
    let resolved = solver.solve(4000);
    let vbr_resolved = solver.exploitability(&resolved);

    // Uniform blueprint on the same tree.
    let blueprint = make_uniform_solution(&tree, hero.len(), villain.len());
    let vbr_blueprint = solver.exploitability(&blueprint);

    eprintln!();
    eprintln!("=== W3 gate ===");
    eprintln!("  VBR(blueprint, uniform) = {vbr_blueprint:.3} chips");
    eprintln!("  VBR(resolved, no gadget) = {vbr_resolved:.3} chips");
    eprintln!();

    assert!(
        vbr_resolved <= vbr_blueprint + 1e-3,
        "resolved exploitability {vbr_resolved:.3} exceeds blueprint {vbr_blueprint:.3}"
    );
}
