//! Direct W3 gate test: the gadget bounds the resolved strategy's
//! exploitability by the blueprint's. This bypasses the pipeline (which
//! the harness in `search_exploitability.rs` cannot exercise) and drives
//! `RiverCfr` directly.
//!
//! Plan (docs/reviews/CHAMELEON-SOTA-PLAN.md, Phase D):
//!   VBR(blueprint + resolve) <= VBR(blueprint)
//!
//! Method:
//! 1. Build a river tree and a real hero/villain range from a state.
//! 2. Compute a synthetic "blueprint" strategy table (uniform here —
//!    real would come from `BlueprintPolicy`, but the property we're
//!    testing is structural: the gadget must not make things worse).
//! 3. VBR_bp = exploitability of that table.
//! 4. v_bp_hero = -villain_cfv_under_strategy(bp_table).
//! 5. Solve with the gadget on → VBR_resolved.
//! 6. Assert VBR_resolved <= VBR_bp + slack.
//!
//! If the gadget is correct, step 6 holds; if it is mis-sized or the
//! `v_bp` sign convention is wrong, VBR_resolved is worse.

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

/// Uniform strategy table over a tree. Stands in for a "blueprint".
fn uniform_table(tree: &PublicTree, hero_seat: usize, nh: usize, nv: usize) -> SolvedRiver {
    let nnodes = tree.nodes.len();
    let mut hero_strat = vec![Vec::new(); nnodes];
    let mut villain_strat = vec![Vec::new(); nnodes];
    for (i, n) in tree.nodes.iter().enumerate() {
        if n.terminal {
            continue;
        }
        let na = n.actions.len();
        let is_hero = (n.player as usize) == hero_seat;
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
fn gadget_bounds_exploitability_by_blueprint() {
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);
    let (st, seq) = river_state_and_seq(&ladder);
    let tree = PublicTree::build_from_state(st, seq, &ladder, 100_000);

    let board = [Card(40), Card(41), Card(42), Card(43), Card(44)];
    let hero: Vec<[u8; 2]> = vec![[10, 11], [12, 13], [14, 15]];
    let villain: Vec<[u8; 2]> = vec![[16, 17], [18, 19], [20, 21]];
    let rank = |c: &[u8; 2]| -> u32 {
        (cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &board) * 1e6) as u32
    };
    let hero_rank: Vec<u32> = hero.iter().map(rank).collect();
    let villain_rank: Vec<u32> = villain.iter().map(rank).collect();

    let hero_seat = 1;

    // Blueprint stand-in: uniform.
    let bp = uniform_table(&tree, hero_seat, hero.len(), villain.len());

    // Helper solver instance to compute exploitability and the villain CFV.
    let helper = RiverCfr::new(
        &tree,
        &hero,
        &hero_rank,
        &villain,
        &villain_rank,
        st,
        hero_seat,
        None,
    );
    let vbr_bp = helper.exploitability(&bp);

    // v_bp_hero = -villain_cfv_under_strategy(bp).
    let villain_cfv = helper.villain_cfv_under_strategy(&bp.hero_strat, &bp.villain_strat);
    let v_bp_hero: Vec<f64> = villain_cfv.iter().map(|v| -v).collect();

    // Solve with the gadget on.
    let gadget_solver = RiverCfr::new(
        &tree,
        &hero,
        &hero_rank,
        &villain,
        &villain_rank,
        st,
        hero_seat,
        Some(v_bp_hero),
    );
    let solved = gadget_solver.solve(4000);
    let vbr_resolved = gadget_solver.exploitability(&solved);

    eprintln!();
    eprintln!("=== W3 direct gate ===");
    eprintln!("  VBR(blueprint=uniform)   = {vbr_bp:.3} chips");
    eprintln!("  VBR(resolved, gadget on) = {vbr_resolved:.3} chips");
    eprintln!();

    // The gadget should not make the resolved strategy *more* exploitable
    // than the blueprint. A small positive slack covers numerical drift
    // and the finite-iters residual.
    assert!(
        vbr_resolved <= vbr_bp + 1.0,
        "gadget should bound exploitability: resolved {vbr_resolved:.3} > blueprint {vbr_bp:.3} + slack"
    );
}
