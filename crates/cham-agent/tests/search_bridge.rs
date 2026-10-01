//! F1 (2026-10-01): the river search bridge fires from the live pipeline
//! when `AgentMode.search.enabled` is true.
//!
//! This is a WIRING test — it does not judge the solver's output quality
//! (the solver has its own suite, including the one-card-poker acceptance
//! test). It asserts:
//!   1. With search OFF (the shipped default), no decision records a
//!      search trace.
//!   2. With search ON and a river-street observation meeting the trigger
//!      preconditions, `DecisionTrace.search` becomes `Some(...)`.

use std::path::Path;

use cham_agent::modes::{AgentMode, SearchCfg};
use cham_agent::pipeline::ChameleonAgent;
use cham_blueprint::policy::{BlueprintPolicy, ProvenanceRecord};
use cham_blueprint::table::ThreadMode;
use cham_core::card::Deck;
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::obs::{Agent, Observables, Player};
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::Encoder;
use cham_router::model::SoftmaxModel;
use cham_router::runtime::RouterRuntime;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

fn trained_policy(dir: &Path, iters: u64, seed: u64) -> BlueprintPolicy {
    let cfg = AbstractionConfig::tiny();
    let mut enc = Encoder::cfg_only(cfg).expect("enc");
    let tcfg = cham_blueprint::TrainerConfig {
        depth_bb: 100,
        iters,
        train_seed: seed,
        snapshot_every: iters,
        bayes_session_block: 100,
        regret_discount: 1.0,
        avg_gamma: 0.9,
        checkpoint_every: 0,
        checkpoint_dir: None,
        explore_eps: 0.0,
    };
    let engine_cfg = EngineConfig::depth(100);
    let (table, _prov) = cham_blueprint::train(
        &tcfg,
        &cham_blueprint::TrainMode::Robust,
        engine_cfg,
        &mut enc,
        ThreadMode::Deterministic,
        dir,
        None,
        None,
    )
    .expect("train");
    let record = ProvenanceRecord {
        abstraction_hash: enc.abstraction_hash(),
        artifact_hash: 0,
        mode: "Robust".into(),
        opponent_id: None,
        depth_bb: 100,
        iters,
        train_seed: seed,
        thread_mode: "Deterministic".into(),
        threads: 1,
        parent: None,
        wall_s: 0.0,
        infosets: table.len(),
        created_unix: 0,
    };
    let policy_dir = dir.join("policy");
    BlueprintPolicy::build_artifact(&table, &record, &policy_dir).expect("build");
    BlueprintPolicy::load(&policy_dir, enc.abstraction_hash()).expect("load")
}

fn make_agent(mode: AgentMode) -> ChameleonAgent {
    let enc = Encoder::cfg_only(AbstractionConfig::tiny()).expect("enc");
    let router = RouterRuntime::new(SoftmaxModel::new(20, 4), 0.7, 8.0, 0.5, -1.5);
    static AGENT_DIR_CTR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = AGENT_DIR_CTR.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = format!(
        "artifacts/runs/search-bridge-test-{}-{n}",
        std::process::id()
    );
    let dir = std::path::Path::new(&dir);
    std::fs::create_dir_all(dir).expect("dir");
    let policy = trained_policy(dir, 300, 0xA6E);
    ChameleonAgent::new(
        mode,
        enc,
        router,
        vec![
            policy.clone(),
            policy.clone(),
            policy.clone(),
            policy.clone(),
        ],
        policy,
        None,
        None,
    )
    .expect("agent")
}

/// Drive a state to the river with all-check sequences. Panics if the
/// hand terminates before the river (it should not with checks/calls).
fn drive_to_river() -> State {
    let rng = &mut rng_from_seed(0xDEADBEEF);
    let mut s = State::new(CFG, Deck::shuffled(rng)).expect("state");
    // Preflop: SB call, BB check.
    s.apply(Action::Call).expect("preflop call");
    s.apply(Action::Check).expect("preflop check");
    while s.street() == cham_core::engine::Street::Flop {
        s.apply(Action::Check).expect("flop check");
    }
    while s.street() == cham_core::engine::Street::Turn {
        s.apply(Action::Check).expect("turn check");
    }
    assert_eq!(
        s.street(),
        cham_core::engine::Street::River,
        "check-check should reach river"
    );
    s
}

/// Search OFF (the shipped default): no decision records a search trace.
#[test]
fn no_search_when_flag_off() {
    let mut agent = make_agent(AgentMode::argmax());
    let s = drive_to_river();
    let rng = &mut rng_from_seed(0xF00D);
    // Ensure the agent acts at the river.
    let seat = s.to_act();
    if seat == 0 {
        let obs = Observables::view(&s, Player::from_usize(0));
        let _ = agent.act(&obs, rng);
        let t = agent.last_trace.as_ref().expect("trace");
        assert!(
            t.search.is_none(),
            "search must be None when mode.search.enabled = false"
        );
    } else {
        // Hero is BB; feed villain a check, then hero acts.
        let mut s = s;
        s.apply(Action::Check).expect("villain check");
        let obs_hero = Observables::view(&s, Player::from_usize(0));
        let _ = agent.act(&obs_hero, rng);
        let t = agent.last_trace.as_ref().expect("trace");
        assert!(
            t.search.is_none(),
            "search must be None when mode.search.enabled = false"
        );
    }
}

/// Search ON + a river decision meeting the trigger preconditions:
/// `DecisionTrace.search` is Some.
#[test]
fn search_fires_when_flag_on() {
    let mut mode = AgentMode::argmax();
    mode.search = SearchCfg {
        enabled: true,
        solver: "Rnr".into(),
        g4_ledger_ref: "EXP-SEARCH".into(),
    };
    let mut agent = make_agent(mode);
    let s = drive_to_river();
    let rng = &mut rng_from_seed(0xBEEF);
    // Ensure the agent acts at the river.
    let seat = s.to_act();
    if seat == 0 {
        let obs = Observables::view(&s, Player::from_usize(0));
        let _ = agent.act(&obs, rng);
        let t = agent.last_trace.as_ref().expect("trace");
        assert!(
            t.search.is_some(),
            "search must fire on a river decision with pot meeting min_pot_bb"
        );
    } else {
        let mut s = s;
        s.apply(Action::Check).expect("villain check");
        let obs_hero = Observables::view(&s, Player::from_usize(0));
        let _ = agent.act(&obs_hero, rng);
        let t = agent.last_trace.as_ref().expect("trace");
        assert!(
            t.search.is_some(),
            "search must fire on a river decision with pot meeting min_pot_bb"
        );
    }
}

/// F1-A/B (2026-10-01): search must NOT fire when the hero faces a bet
/// on the river. The solver tree is rooted at hero-acts-first; there is
/// no honest mapping from its root distribution onto a {fold, call,
/// raise} legal set. The bridge refuses this state.
#[test]
fn search_does_not_fire_when_facing_bet() {
    let mut mode = AgentMode::argmax();
    mode.search = SearchCfg {
        enabled: true,
        solver: "Rnr".into(),
        g4_ledger_ref: "EXP-SEARCH".into(),
    };
    let mut agent = make_agent(mode);

    // Drive preflop+flop+turn all-check to the river.
    let rng = &mut rng_from_seed(0xCAFE);
    let mut s = State::new(CFG, Deck::shuffled(rng)).expect("state");
    s.apply(Action::Call).expect("preflop call");
    s.apply(Action::Check).expect("preflop check");
    while s.street() == cham_core::engine::Street::Flop {
        s.apply(Action::Check).expect("flop check");
    }
    while s.street() == cham_core::engine::Street::Turn {
        s.apply(Action::Check).expect("turn check");
    }
    // On the river, first to act checks, second bets 100, hero (SB) now
    // faces the bet.
    let first = s.to_act();
    s.apply(Action::Check).expect("river check");
    let second = s.to_act();
    assert_ne!(first, second, "two different actors on river");
    s.apply(Action::Bet { to: 200 }).expect("river bet");

    // Now hero is facing a bet. Act. The trace should NOT record a
    // search even though search is enabled — the trigger refuses.
    let hero_seat = s.to_act();
    let obs = Observables::view(&s, Player::from_usize(hero_seat));
    assert!(obs.to_call > 0, "we should be facing a bet");
    let _ = agent.act(&obs, rng);
    let t = agent.last_trace.as_ref().expect("trace");
    assert!(
        t.search.is_none(),
        "search must NOT fire when to_call > 0 (F1-A/B guard)"
    );
}
