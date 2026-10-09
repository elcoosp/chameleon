//! Pipeline smoke: does `act_with_state` (the state-aware path) dispatch
//! to the combo-gadget solver when `CHAM_SEARCH_IMPL=combo-gadget`?
//!
//! The earlier version of this test called `action_distribution` — which
//! is the state-LESS path (`try_solve` only, no combo dispatch). The
//! pipeline dispatch lives in `act_impl`, which `act_with_state` reaches.
//!
//! This test builds a river state directly (so the trigger conditions
//! `river && pot >= min_pot_bb && to_call == 0` are met), calls
//! `act_with_state`, and asserts the trace's solver string is
//! `combo-cfr-gadget`.

use cham_agent::pipeline::ChameleonAgent;
use cham_agent::search_bridge::SolverImpl;
use cham_blueprint::policy::BlueprintPolicy;
use cham_core::card::{Card, Deck};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State, Street};
use cham_core::obs::{Agent as _, Observables, Player, is_legal};
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};
use cham_engine::ladder::ActionLadder;
use cham_router::model::SoftmaxModel;
use cham_router::runtime::RouterRuntime;
use std::path::Path;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

fn river_state(ladder: &ActionLadder) -> (State, ActionSeq) {
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
#[ignore = "plumbing smoke; needs shipped bundle"]
fn combo_dispatch_fires_through_act_with_state() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bundle = std::env::var("CHAM_SEARCH_BUNDLE").unwrap_or_else(|_| {
        root.join("artifacts/agent-honest-19dim")
            .to_string_lossy()
            .into_owned()
    });
    let b = Path::new(&bundle);

    let cfg = std::fs::read_to_string(b.join("abstraction.toml"))
        .ok()
        .and_then(|t| cham_engine::config::parse_config(&t).ok())
        .unwrap_or_else(AbstractionConfig::tiny);
    let enc = Encoder::from_artifacts_dir(&b.join("buckets"), cfg.clone())
        .unwrap_or_else(|_| Encoder::cfg_only(cfg.clone()).expect("enc"));
    let robust = BlueprintPolicy::load(&b.join("robust"), 0).expect("load robust");
    let ladder = ActionLadder::new(&cfg);

    let mut mode = cham_agent::modes::AgentMode::full_search_off();
    mode.search.enabled = true;
    mode.search.solver = "ReachGadget".into();
    mode.search.g4_ledger_ref = "EXP-COMBO-SMOKE".into();

    // Assert the env var reaches `from_mode`.
    let probe = cham_agent::search_bridge::SearchBridgeCfg::from_mode(&mode).expect("cfg");
    eprintln!("dispatch impl_kind = {:?}", probe.impl_kind);
    assert_eq!(
        probe.impl_kind,
        SolverImpl::ComboGadget,
        "CHAM_SEARCH_IMPL was not picked up; the launcher must set it",
    );

    let router = RouterRuntime::new(SoftmaxModel::new(20, 4), 0.7, 8.0, 0.5, -1.5);
    let mut agent = ChameleonAgent::new(
        mode,
        enc,
        router,
        vec![
            robust.clone(),
            robust.clone(),
            robust.clone(),
            robust.clone(),
        ],
        robust.clone(),
        None,
        None,
    )
    .expect("agent");

    let (st, _seq) = river_state(&ladder);
    // Sanity: trigger conditions — river, to_call == 0.
    assert_eq!(st.street(), Street::River);
    assert_eq!(st.to_act(), 1, "hero (BB) to act at river root");

    let obs = Observables::view(&st, Player::from_usize(1));
    eprintln!("pot_bb = {:.3}", obs.pot_bb());

    let mut rng = rng_from_seed(0xC0FFEE);
    let action = agent.act_with_state(&obs, &mut rng, Some(&st));
    eprintln!("action chosen = {:?}", action);

    // Read the trace.
    let trace = agent.last_trace.as_ref().expect("trace populated");
    if let Some(s) = trace.search.as_ref() {
        eprintln!("search trace = {:?}", s);
        assert!(
            s.0.contains("combo"),
            "expected the combo-gadget dispatch, got {:?}",
            s.0,
        );
    } else {
        // No search fired at all: either trigger guard (pot below
        // min_pot_bb) or something else refused. That's a legit outcome
        // to report.
        eprintln!(
            "no search trace at all — trigger did not fire (pot_bb={:.3}, min={})",
            obs.pot_bb(),
            probe.min_pot_bb,
        );
        panic!("search did not fire even though river + to_call==0");
    }
}
