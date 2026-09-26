//! B10 measurement: hero decision latency + artifact load.
//!
//! Self-contained: builds a tiny synthetic hero once (same training pattern as
//! the agent contract tests), so these benches run with or without a trained
//! `artifacts/agent` bundle:
//! - `decision_latency`: end-to-end hero decision (hand lifecycle → tracker
//!   update → router → policy → mixture), search DISABLED;
//! - `decision_latency_search`: same, with the river search unit (cached
//!   subgame + 400-iter RNR solve) forced ON — the cost the decision path
//!   would pay when the G4 trigger fires;
//! - `artifact_load`: `BlueprintPolicy::load` on the tiny synthetic bundle
//!   (the artifact unit; a trained bundle only changes coverage, not format).

use cham_agent::modes::AgentMode;
use cham_agent::pipeline::ChameleonAgent;
use cham_blueprint::policy::{BlueprintPolicy, ProvenanceRecord};
use cham_blueprint::table::ThreadMode;
use cham_core::card::Deck;
use cham_core::engine::config::EngineConfig;
use cham_core::engine::history::{HandHistory, PublicHistory};
use cham_core::engine::{Action, State};
use cham_core::obs::{Agent as _, Observables, Player};
use cham_core::rng::{child, rng_from_seed};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};
use cham_router::model::SoftmaxModel;
use cham_router::runtime::RouterRuntime;
use criterion::{Criterion, black_box, criterion_group, criterion_main};

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

struct BenchWorld {
    hero: ChameleonAgent,
    policy_dir: std::path::PathBuf,
    _tmp: tempfile::TempDir,
    river_obs_state: State,
    ph: PublicHistory,
    hero_net: i64,
}

fn build_world() -> BenchWorld {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = tmp.path().join("tiny-bundle");
    std::fs::create_dir_all(&dir).expect("dir");
    // tiny trained policy (300 iters, same as the agent contract tests)
    let cfg = AbstractionConfig::tiny();
    let mut enc = Encoder::cfg_only(cfg).expect("enc");
    let tcfg = cham_blueprint::TrainerConfig {
        depth_bb: 100,
        iters: 300,
        train_seed: 0xA6E,
        snapshot_every: 300,
        bayes_session_block: 100,
        regret_discount: 1.0,
    };
    let (table, prov) = cham_blueprint::train(
        &tcfg,
        &cham_blueprint::TrainMode::Robust,
        CFG,
        &mut enc,
        ThreadMode::Deterministic,
        &dir,
        None,
        None,
    )
    .expect("train");
    let record = ProvenanceRecord {
        abstraction_hash: prov.abstraction_hash,
        artifact_hash: 0,
        mode: "Robust".into(),
        opponent_id: None,
        depth_bb: 100,
        iters: 300,
        train_seed: 0xA6E,
        thread_mode: "Deterministic".into(),
        threads: 1,
        parent: None,
        wall_s: 0.0,
        infosets: table.len(),
        created_unix: 0,
    };
    BlueprintPolicy::build_artifact(&table, &record, &dir).expect("build");
    let policy = BlueprintPolicy::load(&dir, 0).expect("load");
    let hero = ChameleonAgent::new(
        AgentMode::full_search_off(),
        Encoder::cfg_only(AbstractionConfig::tiny()).expect("enc"),
        RouterRuntime::new(SoftmaxModel::new(20, 4), 0.7, 0.3, 0.5, -1.5),
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
    .expect("agent");
    // drive one hand to the river for a realistic mid-hand observation
    let rng = &mut child(0xDEC, "river");
    let mut s = State::new(CFG, Deck::shuffled(rng)).expect("s");
    let mut seq = ActionSeq::default();
    let mut guard = 0;
    let mut log: Vec<(cham_core::engine::Street, Player, Action)> = Vec::new();
    {
        let enc2 = Encoder::cfg_only(AbstractionConfig::tiny()).expect("enc");
        while s.street() != cham_core::engine::Street::River && guard < 400 {
            guard += 1;
            if s.is_terminal() {
                break;
            }
            let seat = s.to_act();
            let obs = Observables::view(&s, Player::from_usize(seat));
            let legals: Vec<Action> = obs.legal.iter().map(|l| l.action).collect();
            if legals.is_empty() {
                break;
            }
            let a = legals[0];
            log.push((s.street(), Player::from_usize(seat), a));
            enc2.record(&obs, Player::from_usize(seat), a, &mut seq);
            if s.apply(a).is_err() {
                break;
            }
        }
    }
    if s.is_terminal() {
        // degenerate shuffle folded preflop — replay until a river is live
        let rng = &mut rng_from_seed(0xDEC0DE);
        s = State::new(CFG, Deck::shuffled(rng)).expect("s");
        s.apply(Action::Call).expect("call");
        s.apply(Action::Check).expect("check");
        s.apply(Action::Check).expect("check");
        s.apply(Action::Check).expect("check");
        s.apply(Action::Check).expect("check");
        s.apply(Action::Check).expect("check");
    }
    let n = s.board_len() as usize;
    let mut board = [cham_core::card::Card(0); 5];
    board[..n].copy_from_slice(&s.board()[..n]);
    let hh = HandHistory {
        seed: 0xDEC,
        actions: log,
        cfg: s.cfg(),
        holes: [s.hole(0), s.hole(1)],
        board,
        board_len: s.board_len(),
        result_sb: 0,
    };
    let ph = PublicHistory::from(&hh);
    BenchWorld {
        hero,
        policy_dir: dir,
        _tmp: tmp,
        river_obs_state: s,
        ph,
        hero_net: 0,
    }
}

fn bench_decision(c: &mut Criterion) {
    let mut w = build_world();
    let mut rng = rng_from_seed(0x111);
    c.bench_function("decision_latency", |b| {
        b.iter(|| {
            // full per-decision path: hand lifecycle → tracker → router →
            // policy → mixture (search disabled)
            w.hero.on_hand_end(black_box(&w.ph), w.hero_net);
            let obs = Observables::view(
                black_box(&w.river_obs_state),
                Player::from_usize(w.river_obs_state.to_act()),
            );
            w.hero.act(&obs, &mut rng).to_str()
        })
    });
}

fn bench_decision_search(c: &mut Criterion) {
    let mut w = build_world();
    let mut rng = rng_from_seed(0x222);
    // forced-ON search unit: cached subgame build + 400-iter RNR solve
    let hero = cham_search::prior::collapse_to_classes(
        (0..9).map(|i| (1.0 / 9.0, i as f64 / 8.0)).collect(),
        3,
    );
    let villain = cham_search::prior::collapse_to_classes(
        (0..9).map(|i| (1.0 / 9.0, i as f64 / 8.0)).collect(),
        3,
    );
    let mut prior = cham_search::prior::PriorStrats::empty();
    prior.set("check", vec![1.0]);
    prior.set("fold", vec![1.0, 0.0, 0.0]);
    prior.set("call", vec![0.0, 1.0, 0.0]);
    c.bench_function("decision_latency_search", |b| {
        b.iter(|| {
            w.hero.on_hand_end(black_box(&w.ph), w.hero_net);
            let obs = Observables::view(
                black_box(&w.river_obs_state),
                Player::from_usize(w.river_obs_state.to_act()),
            );
            let a = w.hero.act(&obs, &mut rng);
            // river search forced ON: content-keyed build (L1 hit) + solve
            let sg = cham_search::cache::cached_build(
                hero.clone(),
                villain.clone(),
                12.0,
                92.0,
                &[0.5, 1.25],
                0xBE4C,
            )
            .expect("sg");
            let r = cham_search::solve::solve(
                &sg,
                &prior,
                &cham_search::trigger::SolverChoice::Rnr { p: 0.9 },
                400,
            )
            .expect("solve");
            (a.to_str(), r.our_strategy.len())
        })
    });
}

fn bench_artifact_load(c: &mut Criterion) {
    let w = build_world();
    c.bench_function("artifact_load", |b| {
        b.iter(|| {
            let p = BlueprintPolicy::load(black_box(&w.policy_dir), 0).expect("load");
            p.len()
        })
    });
}

criterion_group!(
    benches,
    bench_decision,
    bench_decision_search,
    bench_artifact_load
);
criterion_main!(benches);
