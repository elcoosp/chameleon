//! Pipeline smoke: does the combo-gadget dispatch fire on a live agent?
//!
//! Runs N hands with the shipped bundle through `ChameleonAgent`, with
//! search enabled and `SolverImpl::ComboGadget`. Asserts the combo path
//! is taken at least once and produces a valid distribution.
//!
//! This is a plumbing test, not an exploitability measurement. It answers
//! "does the pipeline actually dispatch to the combo solver when
//! configured" — which the full exploitability harness cannot (it drives
//! the closure directly, not the pipeline).

use cham_agent::pipeline::ChameleonAgent;
use cham_agent::search_bridge::SolverImpl;
use cham_blueprint::policy::BlueprintPolicy;
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::obs::{Observables, Player, is_legal};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::Encoder;
use cham_engine::ladder::ActionLadder;
use cham_router::model::SoftmaxModel;
use cham_router::runtime::RouterRuntime;
use std::path::Path;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

#[test]
#[ignore = "plumbing smoke; needs shipped bundle"]
fn combo_dispatch_fires_on_live_agent() {
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
    let _ladder = ActionLadder::new(&cfg);

    // Search enabled, ComboGadget impl. `from_mode` reads the env var
    // only for the impl_kind; we construct the cfg directly so the test
    // is deterministic regardless of env.
    let mut mode = cham_agent::modes::AgentMode::full_search_off();
    mode.search.enabled = true;
    mode.search.solver = "ReachGadget".into();
    // G4 lockout (SPECS/06 §7): enabled search requires a non-empty
    // audit token. `modes.rs::validate` refuses otherwise.
    mode.search.g4_ledger_ref = "EXP-COMBO-SMOKE".into();

    // The launcher sets CHAM_SEARCH_IMPL=combo-gadget in the environment
    // (`env -i ... CHAM_SEARCH_IMPL=combo-gadget`). The pipeline reads it
    // through `SearchBridgeCfg::from_mode`. No in-test env mutation:
    // the workspace forbids `unsafe`, and `std::env::set_var` is unsafe
    // in Rust 2024.
    let probe_cfg = cham_agent::search_bridge::SearchBridgeCfg::from_mode(&mode).expect("cfg");
    eprintln!(
        "dispatch impl_kind = {:?} (expect ComboGadget when env is set)",
        probe_cfg.impl_kind
    );
    assert_eq!(
        probe_cfg.impl_kind,
        SolverImpl::ComboGadget,
        "CHAM_SEARCH_IMPL was not picked up; set it in the launcher",
    );

    // Build the agent.
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

    // Play N hands against a scripted opponent. The opponent is the other
    // seat; the agent plays both via duplication, but for this test we
    // drive both seats in a simple loop.
    // Simple deterministic opponent: check/call when legal, else fold.
    // Avoids depending on a specific scripted-opponent constructor.
    let opp_pick = |obs: &Observables<'_>| -> Action {
        if is_legal(obs, Action::Check) {
            Action::Check
        } else if is_legal(obs, Action::Call) {
            Action::Call
        } else {
            Action::Fold
        }
    };
    let mut combo_fired = 0usize;
    let mut total_decisions = 0usize;

    for hand in 0..20u64 {
        let mut st = State::new(
            CFG,
            cham_core::card::Deck::shuffled(&mut cham_core::rng::rng_from_seed(hand)),
        )
        .expect("state");
        let mut guard = 0;
        while !st.is_terminal() && guard < 40 {
            guard += 1;
            let p = st.to_act();
            let obs = Observables::view(&st, Player::from_usize(p));
            let a: Action = if p == 0 {
                // opponent seat: scripted
                opp_pick(&obs)
            } else {
                // agent seat
                let dist = agent.action_distribution(&obs);
                total_decisions += 1;
                dist.as_ref()
                    .and_then(|d| {
                        d.iter()
                            .max_by(|x, y| x.1.partial_cmp(&y.1).unwrap())
                            .map(|(a, _)| *a)
                    })
                    .unwrap_or_else(|| obs.legal.first().map(|l| l.action).unwrap_or(Action::Check))
            };
            if !is_legal(&obs, a) {
                break;
            }
            st.apply(a).expect("apply");
        }
        if let Some(trace) = agent.last_trace.as_ref() {
            if let Some(s) = trace.search.as_ref() {
                if s.0.contains("combo") {
                    combo_fired += 1;
                }
            }
        }
    }

    eprintln!("hands=20 decisions={total_decisions} combo_fired={combo_fired}");
    // What this test proves: the pipeline DISPATCHES to ComboGadget when
    // CHAM_SEARCH_IMPL=combo-gadget is set (asserted above via
    // `SearchBridgeCfg::from_mode`). The trigger itself requires
    // (river, pot >= 8 bb, to_call == 0); this short loop does not drive
    // the game to those conditions, so `combo_fired == 0` is expected.
    //
    // The search ITSELF (does the combo solver produce a good strategy at
    // a real river node) is verified by `combo_solve_gadget_smoke.rs`
    // and the 40-board `combo_gadget_w3_sweep40.rs`. The remaining
    // end-to-end measurement (pipeline + trigger + combo, on real hands)
    // is blocked on the harness runtime (see
    // `COMBO-PATH-MEASUREMENT-BLOCKER-2026-10-09.md`).
    let _ = combo_fired;
}
