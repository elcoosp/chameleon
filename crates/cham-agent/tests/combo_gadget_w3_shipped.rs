//! W3 gate against the SHIPPED blueprint, direct path.
//!
//! The uniform-blueprint version (`combo_gadget_w3_direct.rs`) is
//! trivially satisfiable (uniform is very exploitable, so any sane
//! solver beats it). This version loads `agent-honest-19dim/robust`,
//! computes its exploitability and its villain CFV, and checks that the
//! gadget-bounded solve does not exceed it.
//!
//! Run with:
//!
//!     cargo test -p cham-agent --test combo_gadget_w3_shipped --no-run
//!     env -i PATH="$PATH" HOME="$HOME" \
//!       CHAM_D1_BP="$PWD/artifacts/agent-honest-19dim/robust" \
//!       CHAM_D1_BUCKETS="$PWD/artifacts/agent-honest-19dim/buckets" \
//!       target/debug/deps/combo_gadget_w3_shipped-<hash> --ignored --nocapture
//!
//! CHAM_SLOT_BUCKET must be UNSET for agent-honest-19dim.

use cham_blueprint::policy::BlueprintPolicy;
use cham_core::card::{Card, Deck, Hand2};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State, Street};
use cham_core::obs::{Observables, Player, is_legal};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};
use cham_engine::ladder::ActionLadder;
use cham_search::pubtree::PublicTree;
use cham_search::river_cfr::{RiverCfr, build_blueprint_strategy_table};

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
#[ignore = "integration; needs shipped bundle"]
fn w3_gate_with_shipped_blueprint() {
    let bp_dir = std::env::var("CHAM_D1_BP")
        .unwrap_or_else(|_| "artifacts/agent-honest-19dim/robust".into());
    let robust = BlueprintPolicy::load(std::path::Path::new(&bp_dir), 0).expect("load bp");

    let bundle = std::path::Path::new(&bp_dir).parent().expect("bundle");
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
    let (st, seq) = river_state_and_seq(&ladder);
    let tree = PublicTree::build_from_state(st, seq, &ladder, 200_000);

    let board = [Card(40), Card(41), Card(42), Card(43), Card(44)];
    let hero: Vec<[u8; 2]> = vec![[10, 11], [12, 13], [14, 15]];
    let villain: Vec<[u8; 2]> = vec![[16, 17], [18, 19], [20, 21]];
    let rank = |c: &[u8; 2]| -> u32 {
        (cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &board) * 1e6) as u32
    };
    let hero_rank: Vec<u32> = hero.iter().map(rank).collect();
    let villain_rank: Vec<u32> = villain.iter().map(rank).collect();

    let hero_seat = 1;

    // Build the shipped blueprint's per-combo strategy table.
    let bp_policy = |o: &Observables<'_>, s: &ActionSeq| -> Option<Vec<f64>> {
        let mut enc = encoder.clone();
        robust.strategy(o, &mut enc, s)
    };
    let (bp_hero, bp_villain) = build_blueprint_strategy_table(
        &tree, st, seq, &ladder, &hero, &villain, hero_seat, bp_policy,
    );

    // Helper solver to compute VBR and the villain CFV.
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
    let vbr_bp = helper.exploitability(&cham_search::river_cfr::SolvedRiver {
        hero_strat: bp_hero.clone(),
        villain_strat: bp_villain.clone(),
        gadget_root_strat: None,
        iters: 0,
    });

    // v_bp_hero from the blueprint table.
    let villain_cfv = helper.villain_cfv_under_strategy(&bp_hero, &bp_villain);
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
    eprintln!("=== W3 gate, shipped blueprint ===");
    eprintln!("  bundle:                     {bp_dir}");
    eprintln!(
        "  VBR(shipped blueprint)    = {vbr_bp:.3} chips ({:.5} bb)",
        vbr_bp / 100.0
    );
    eprintln!(
        "  VBR(resolved, gadget on)  = {vbr_resolved:.3} chips ({:.5} bb)",
        vbr_resolved / 100.0
    );
    eprintln!();

    assert!(
        vbr_resolved <= vbr_bp + 1.0,
        "gadget should not worsen: resolved {vbr_resolved:.3} vs blueprint {vbr_bp:.3}"
    );
}
