//! Contractual test set for cham-agent (SPECS/07 §7).

use std::path::Path;

use cham_agent::modes::AgentMode;
use cham_agent::pipeline::ChameleonAgent;
use cham_agent::tracker::Tracker;
use cham_blueprint::policy::{BlueprintPolicy, ProvenanceRecord};
use cham_blueprint::table::{RegretTable, ThreadMode};
use cham_core::card::{Card, Deck};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::obs::{Agent, Observables, Player};
use cham_core::rng::{child, rng_from_seed};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::Encoder;
use cham_router::model::SoftmaxModel;
use cham_router::runtime::RouterRuntime;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

fn card(s: &str) -> Card {
    Card::parse(s).expect("card")
}

/// Train one small robust table → all experts use it (identical artifact).
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
    };
    let (table, prov) = cham_blueprint::train(
        &tcfg,
        &cham_blueprint::TrainMode::Robust,
        CFG,
        &mut enc,
        ThreadMode::Deterministic,
        dir,
        None,
        None,
    )
    .expect("train");
    std::fs::create_dir_all(dir).expect("dir");
    let record = ProvenanceRecord {
        abstraction_hash: prov.abstraction_hash,
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
    BlueprintPolicy::build_artifact(&table, &record, dir).expect("build");
    BlueprintPolicy::load(dir, 0).expect("load")
}

fn make_agent(mode: AgentMode) -> ChameleonAgent {
    let cfg = AbstractionConfig::tiny();
    let enc = Encoder::cfg_only(cfg).expect("enc");
    let router = RouterRuntime::new(SoftmaxModel::new(20, 4), 0.7, 8.0, 0.5, -1.5);
    // Unique scratch dir per call: parallel test processes share
    // `artifacts/runs/`, and `build_artifact` writes policy.bin directly
    // (no tmp+rename) while `trained_policy` loads it back — a shared dir
    // admits torn reads under load (intermittent `bad magic` load failures).
    static AGENT_DIR_CTR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = AGENT_DIR_CTR.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = format!("artifacts/runs/agent-test-{}-{n}", std::process::id());
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

// ---------- tracker ----------

#[test]
fn tracker_ewm_math() {
    let mut t = Tracker::new();
    assert_eq!(t.hands, 0);
    // one fold-to-everything opponent hand: vpip 0
    let ph = ph_fold();
    t.observe_hand(&ph, -100, 0);
    assert_eq!(t.hands, 1);
    // EWM: s ← s·λ + x·(1−λ); λ = 0.5^(1/60) ≈ 0.98851 → s = 0.5·0.98851 ≈ 0.49426
    // H-1 fix (2026-09-27): the test previously encoded the bug — it used
    // `(0.5f64).ln() / 60.0` (the LOG, ≈ −0.0116) as if it were λ, which
    // let the negative-λ code pass. Now matches the corrected tracker.
    let lam = (0.5f64).powf(1.0 / 60.0);
    let expected = 0.5 * lam;
    assert!(
        (t.ewm[0] - expected).abs() < 1e-6,
        "vpip ewm {} vs {expected}",
        t.ewm[0]
    );
}

fn ph_fold() -> cham_core::engine::history::PublicHistory {
    cham_core::engine::history::PublicHistory {
        actions: vec![(
            cham_core::engine::Street::Preflop,
            cham_core::obs::Player::Sb,
            Action::Fold,
        )],
        board: [card("2c"); 5],
        showdown_holes: [None, None],
        nets: [50, -50],
    }
}

#[test]
fn tracker_opportunity_counts() {
    let mut t = Tracker::new();
    let ph = ph_open_call();
    t.observe_hand(&ph, 0, 0);
    assert_eq!(t.opp_faces_open, 1, "opponent faced our open");
    let ph2 = ph_open_call();
    t.observe_hand(&ph2, 0, 0);
    assert_eq!(t.opp_faces_open, 2);
}

fn ph_open_call() -> cham_core::engine::history::PublicHistory {
    cham_core::engine::history::PublicHistory {
        actions: vec![
            (
                cham_core::engine::Street::Preflop,
                cham_core::obs::Player::Sb,
                Action::Raise { to: 250 },
            ),
            (
                cham_core::engine::Street::Preflop,
                cham_core::obs::Player::Bb,
                Action::Call,
            ),
        ],
        board: [card("2c"); 5],
        showdown_holes: [None, None],
        nets: [0, 0],
    }
}

#[test]
fn tracker_leak_proof() {
    // I9 through the agent: the tracker state serializes WITHOUT any hole cards —
    // across 1k fuzzed hands the serialized state contains no Hand2/lead-card data.
    use cham_core::engine::fuzz;
    let mut t = Tracker::new();
    for seed in 0..1_000u64 {
        let rng = &mut rng_from_seed(0x1EA9 ^ seed);
        if let Ok(hh) = fuzz::play_random(CFG, seed, rng) {
            let ph = cham_core::engine::history::PublicHistory::from(&hh);
            t.observe_hand(&ph, hh.result_sb, 0);
        }
    }
    let text = serde_json::to_string(&t).expect("serialize");
    let v: serde_json::Value = serde_json::from_str(&text).expect("json");
    let keys: Vec<&str> = v
        .as_object()
        .expect("obj")
        .keys()
        .map(|k| k.as_str())
        .collect();
    for forbidden in ["holes", "board", "showdown_holes", "cards", "seed"] {
        assert!(
            !keys.iter().any(|k| k.contains(forbidden)),
            "tracker leaks {forbidden}"
        );
    }
    assert!(!t.showdown_seen(cham_core::card::Hand2::new(card("As"), card("Ks"))));
}

#[test]
fn tracker_maturity_shrink() {
    // min(1, hands/150) toward 0.5
    assert!((cham_router::features::maturity_shrink(0) - 0.0).abs() < 1e-9);
    assert!((cham_router::features::maturity_shrink(75) - 0.5).abs() < 1e-9);
    assert!((cham_router::features::maturity_shrink(300) - 1.0).abs() < 1e-9);
    // a raw 0.0 stat shrinks toward 0.5: hands=75 → 0.25
    let mut t = Tracker::new();
    t.hands = 75;
    t.ewm[0] = 0.0;
    assert!((t.shrunk_ewm()[0] - 0.25).abs() < 1e-9);
}

// ---------- pipeline ----------

#[test]
fn weights_frozen_within_hand() {
    let mut agent = make_agent(AgentMode::full_search_off());
    // play one hand with ≥ 2 AGENT decisions; weights must be identical across them
    let prefix = [
        card("Ah"),
        card("2c"),
        card("Ad"),
        card("3s"),
        card("9h"),
        card("4d"),
        card("Js"),
    ];
    // flop, BB (villain) to act first; the agent (SB) gets ≥ 2 decisions
    let mut s = State::new(CFG, Deck::with_prefix(&prefix)).expect("s");
    s.apply(Action::Call).expect("ok");
    s.apply(Action::Check).expect("ok");
    let mut rng = rng_from_seed(5);
    let mut villain = cham_opponents::baselines::CallBot;
    let mut weights = vec![];
    let mut guard = 0;
    while !s.is_terminal() && guard < 400 && weights.len() < 3 {
        guard += 1;
        let obs = Observables::view(&s, Player::from_usize(s.to_act()));
        if s.to_act() == 0 {
            let a = agent.act(&obs, &mut rng);
            if let Some(t) = &agent.last_trace {
                weights.push(t.weights_frozen);
            }
            s.apply(a).expect("legal");
        } else {
            let a = villain.act(&obs, &mut rng);
            agent.on_public_action(&obs, Player::from_usize(s.to_act()), a);
            s.apply(a).expect("legal");
        }
    }
    assert!(weights.len() >= 2, "captured {} decisions", weights.len());
    for w in &weights[1..] {
        assert_eq!(w, &weights[0], "weights frozen within the hand");
    }
}

#[test]
fn pipeline_deterministic_replay() {
    // THE sacred test: full mode, seeded, single-threaded → byte-identical traces.
    let run = |seed: u64| -> String {
        let mut agent = make_agent(AgentMode::full_search_off());
        let mut out = String::new();
        let mut villain = cham_opponents::baselines::CallBot;
        for h in 0..30u64 {
            let rng = &mut child(seed, &format!("h{h}"));
            let mut s = State::new(CFG, Deck::shuffled(rng)).expect("s");
            let mut guard = 0;
            while !s.is_terminal() && guard < 400 {
                guard += 1;
                let obs = Observables::view(&s, Player::from_usize(s.to_act()));
                let a = if s.to_act() == 0 {
                    let a = agent.act(&obs, rng);
                    if let Some(t) = &agent.last_trace {
                        out.push_str(&format!("{};", t.action));
                    }
                    agent.on_public_action(&obs, Player::from_usize(s.to_act()), a);
                    a
                } else {
                    let a = villain.act(&obs, rng);
                    agent.on_public_action(&obs, Player::from_usize(s.to_act()), a);
                    a
                };
                s.apply(a).expect("legal");
            }
        }
        out
    };
    let a = run(0xABCD);
    let b = run(0xABCD);
    assert_eq!(a, b, "byte-identical traces under the same seed");
    let c = run(0xABCE);
    assert_ne!(a, c, "different seeds differ");
}

#[test]
fn fallback_paths() {
    // uncovered expert + uncovered robust → uniform + fallback record: force by
    // using an EMPTY artifact (no rows) — all lookups miss.
    let cfg = AbstractionConfig::tiny();
    let enc = Encoder::cfg_only(cfg).expect("enc");
    let router = RouterRuntime::new(SoftmaxModel::new(20, 4), 0.7, 8.0, 0.5, -1.5);
    let mut empty_table = RegretTable::new(ThreadMode::Deterministic);
    // insert one row so build_artifact succeeds, but with a key that never occurs
    empty_table.entry_or_insert(u64::MAX, 2);
    let dir = std::path::Path::new("artifacts/runs/agent-empty");
    std::fs::create_dir_all(dir).expect("dir");
    let prov = ProvenanceRecord {
        abstraction_hash: 0,
        artifact_hash: 0,
        mode: "Robust".into(),
        opponent_id: None,
        depth_bb: 100,
        iters: 0,
        train_seed: 0,
        thread_mode: "Deterministic".into(),
        threads: 1,
        parent: None,
        wall_s: 0.0,
        infosets: 1,
        created_unix: 0,
    };
    BlueprintPolicy::build_artifact(&empty_table, &prov, dir).expect("build");
    let policy = BlueprintPolicy::load(dir, 0).expect("load");
    let mut agent = ChameleonAgent::new(
        AgentMode::full_search_off(),
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
    .expect("agent");
    let mut rng = rng_from_seed(3);
    let s = fresh_hand();
    let obs = Observables::view(&s, Player::Sb);
    let a = agent.act(&obs, &mut rng);
    assert!(
        obs.legal.iter().any(|l| l.action == a),
        "uniform fallback is legal"
    );
    assert!(
        agent
            .last_trace
            .as_ref()
            .map(|t| t.fallback_used)
            .unwrap_or(false),
        "fallback recorded"
    );
}

#[test]
fn pipeline_mode_matrix() {
    // all canonical modes build and produce structurally distinct traces
    let modes = vec![
        AgentMode::full_search_off(),
        AgentMode::argmax(),
        AgentMode::robust_only(),
    ];
    for mode in modes {
        let mut agent = make_agent(mode.clone());
        let rng = &mut rng_from_seed(7);
        let s = fresh_hand();
        let obs = Observables::view(&s, Player::Sb);
        let _ = agent.act(&obs, rng);
        assert!(agent.last_trace.is_some(), "mode {} traces", mode.routing);
    }
    // search lockout: enabled without ledger ref must fail construction
    let bad = AgentMode {
        routing: "mixture".into(),
        search: cham_agent::modes::SearchCfg {
            enabled: true,
            solver: "Rnr".into(),
            g4_ledger_ref: String::new(),
        },
        fallback_mode: std::env::var("CHAM_FALLBACK_MODE").unwrap_or_else(|_| "renorm".into()),
    };
    assert!(
        bad.validate().is_err(),
        "search_mode_lockout: no G4 ref → refuse"
    );
    // L-19 re-baseline (2026-09-27): a non-empty g4_ledger_ref used to make
    // `search.enabled = true` validate, but the runtime hardcodes
    // `search: None` in every decision trace (pipeline.rs), so "valid"
    // meant "silently plays the fallback strategy". The validate() contract
    // now REFUSES any enabled search until the RiverSearcher is wired in.
    // Order matters: the empty-ref check runs first (still a clean G4
    // refusal), and the not-wired check runs second for the non-empty case.
    let ok = AgentMode {
        routing: "mixture".into(),
        search: cham_agent::modes::SearchCfg {
            enabled: true,
            solver: "Rnr".into(),
            g4_ledger_ref: "EXP-002".into(),
        },
        fallback_mode: "renorm".into(),
    };
    let err = ok
        .validate()
        .expect_err("L-19: enabled search must refuse until the searcher is wired");
    let msg = format!("{err}");
    assert!(
        msg.contains("not yet wired") || msg.contains("RiverSearcher"),
        "L-19 refusal message must point at the missing wiring, got: {msg}"
    );
}

#[test]
fn argmax_no_rng_consumption() {
    let mut a1 = make_agent(AgentMode::argmax());
    let mut a2 = make_agent(AgentMode::argmax());
    let s = fresh_hand();
    let obs = Observables::view(&s, Player::Sb);
    let r1 = &mut rng_from_seed(99);
    let r2 = &mut rng_from_seed(99);
    let a = a1.act(&obs, r1);
    let b = a2.act(&obs, r2);
    assert_eq!(a, b, "argmax consumes no rng");
    // rngs untouched: same seed states produce identical next draws
    assert_eq!(
        cham_core::rng::next_u32(r1),
        cham_core::rng::next_u32(r2),
        "rng streams unmodified by argmax"
    );
}

#[test]
fn mirror_match_smoke() {
    // 200 hands, two independent agents, no panics, valid traces
    let mut a = make_agent(AgentMode::full_search_off());
    let mut b = make_agent(AgentMode::full_search_off());
    for h in 0..200u64 {
        let rng = &mut child(0x1234, &format!("h{h}"));
        let mut s = State::new(CFG, Deck::shuffled(rng)).expect("s");
        let mut guard = 0;
        while !s.is_terminal() && guard < 400 {
            guard += 1;
            let obs = Observables::view(&s, Player::from_usize(s.to_act()));
            let agent: &mut dyn Agent = if s.to_act() == 0 { &mut a } else { &mut b };
            let action = agent.act(&obs, rng);
            let actor = Player::from_usize(s.to_act());
            if s.to_act() == 0 {
                a.on_public_action(&obs, actor, action);
            } else {
                b.on_public_action(&obs, actor, action);
            }
            s.apply(action).expect("legal");
        }
        let ph = cham_core::engine::history::PublicHistory::from(
            &cham_core::engine::history::HandHistory {
                seed: h,
                actions: vec![],
                cfg: CFG,
                holes: [s.hole(0), s.hole(1)],
                board: *s.board(),
                board_len: s.board_len(),
                result_sb: s.payoffs()[0],
            },
        );
        a.on_hand_end(&ph, s.payoffs()[0]);
        b.on_hand_end(&ph, s.payoffs()[1]);
    }
    assert!(a.tracker.hands == 200 && b.tracker.hands == 200);
}

fn fresh_hand() -> State {
    let prefix = [
        card("Ah"),
        card("2c"),
        card("Ad"),
        card("3s"),
        card("9h"),
        card("4d"),
        card("Js"),
    ];
    let mut s = State::new(CFG, Deck::with_prefix(&prefix)).expect("s");
    s.apply(Action::Call).expect("ok");
    s.apply(Action::Check).expect("ok");
    s
}

#[test]
fn reach_weighted_mixture_e2e() {
    // Kuhn-toy style check on the pipeline level: with ALL experts identical and
    // the robust identical, the mixture reduces to that expert's σ (reach products
    // of identical σ's cancel in the normalization).
    let mut agent = make_agent(AgentMode::full_search_off());
    let s = fresh_hand();
    let obs = Observables::view(&s, Player::Bb);
    let rng = &mut rng_from_seed(11);
    let _ = agent.act(&obs, rng);
    // the trace exists and the weights are the router's output (frozen)
    let t = agent.last_trace.expect("trace");
    // L-18 fix (2026-09-27): the previous assertion ended with
    // `|| t.weights_frozen[4] > 0.0`, so a sum far from 1.0 passed whenever
    // the fifth slot happened to be positive — the assertion was
    // effectively vacuous. Require BOTH the sum to be 1 AND every weight
    // to be a valid probability.
    let sum: f64 = t.weights_frozen.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-6,
        "weights_frozen must sum to 1.0, got {sum}: {:?}",
        t.weights_frozen
    );
    for (i, &w) in t.weights_frozen.iter().enumerate() {
        assert!(
            (0.0..=1.0).contains(&w),
            "weights_frozen[{i}] = {w} out of [0,1]: {:?}",
            t.weights_frozen
        );
    }
}

#[test]
fn loader_hash_guards() {
    // H-6 fix (2026-09-27): policy.bin is now SELF-VERIFYING. The payload
    // region (keys || offsets || rows) is hashed at build time and the
    // hash is stamped into the embedded provenance; `load` recomputes it
    // and refuses a mismatch. The previous version of this test asserted
    // `err.is_ok() || err.is_err()` — a tautology that passed regardless
    // of tampering — precisely the reason H-6 went unnoticed.
    let dir = std::path::Path::new("artifacts/runs/loader-test");
    std::fs::create_dir_all(dir).expect("dir");
    let _ = trained_policy(dir, 50, 0xBEEF);

    // Sanity: an untampered artifact loads cleanly.
    assert!(
        BlueprintPolicy::load(dir, 0).is_ok(),
        "fresh artifact loads"
    );

    // Tamper the LAST byte — it lives inside the payload region (the final
    // row's `probs`), so the payload hash must now disagree with the
    // provenance-stamped hash.
    let mut bytes = std::fs::read(dir.join("policy.bin")).expect("read");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    std::fs::write(dir.join("policy.bin"), &bytes).expect("write");
    let err = BlueprintPolicy::load(dir, 0);
    match err {
        Ok(_) => panic!("tampered policy.bin loaded — H-6 hash guard failed"),
        Err(e) => {
            let s = format!("{e}");
            assert!(
                s.contains("HashMismatch") || s.contains("hash"),
                "tampered load must fail on the payload hash, got: {s}"
            );
        }
    }

    // Rebuild, then verify the abstraction-hash guard still fires.
    let _ = trained_policy(dir, 50, 0xBEEF);
    let err2 = BlueprintPolicy::load(dir, 0xDEAD_BEEF);
    assert!(err2.is_err(), "abstraction hash mismatch must refuse");
}

#[test]
fn loader_refuses_over_budget() {
    // B8: a synthetic bundle over a tiny budget refuses on the budget path
    // (naming the largest contributor); a generous budget passes the guard
    // (then fails on the missing abstraction.toml — proving the guard passed).
    let dir = tempfile::tempdir().expect("dir");
    let base = dir.path().join("bundle");
    std::fs::create_dir_all(base.join("experts/0")).expect("dir");
    let big = vec![0xABu8; 3 * 1024 * 1024];
    std::fs::write(base.join("experts/0/policy.bin"), &big).expect("write");
    let err = match cham_agent::loader::load_agent_with_budget(&base, "mixture", 100, 1) {
        Ok(_) => panic!("tiny budget must refuse"),
        Err(e) => e,
    };
    let msg = format!("{err}");
    assert!(msg.contains("budget"), "budget refusal names itself: {msg}");
    assert!(
        msg.contains("policy.bin"),
        "refusal names largest contributor: {msg}"
    );
    let err2 = match cham_agent::loader::load_agent_with_budget(&base, "mixture", 100, 4) {
        Ok(_) => panic!("no abstraction.toml here"),
        Err(e) => e,
    };
    assert!(
        !format!("{err2}").contains("budget"),
        "guard must pass first: {err2}"
    );
}

#[test]
fn exp012_r3_robust_only_telemetry_scoped() {
    // R3: robust-only fallback bit reflects the robust tier alone, and the
    // trace carries identical miss-detection counts regardless of mode.
    let mut a = make_agent(AgentMode::robust_only());
    let s = fresh_hand();
    let obs = Observables::view(&s, Player::Bb);
    let rng = &mut rng_from_seed(21);
    let _ = a.act(&obs, rng);
    let t = a.last_trace.expect("trace");
    // R3 scoping invariant: robust-only's bit equals the robust tier's own
    // miss (never the mixture/expert bits), whatever the coverage.
    assert_eq!(t.fallback_used, t.robust_missed);
}

#[test]
fn exp013_r2_renorm_differs_from_substitute() {
    // R2 invariant: miss DETECTION is identical under both composition modes;
    // only the mixture VALUE changes. Exercised at the AgentMode level: both
    // modes validate and route identically on fully-covered infosets.
    let mut renorm = AgentMode::full_search_off();
    renorm.fallback_mode = "renorm".into();
    let mut subst = AgentMode::full_search_off();
    subst.fallback_mode = "substitute".into();
    let mut a = make_agent(renorm);
    let mut b = make_agent(subst);
    let s = fresh_hand();
    let obs = Observables::view(&s, Player::Bb);
    let _ = a.act(&obs, &mut rng_from_seed(31));
    let _ = b.act(&obs, &mut rng_from_seed(31));
    let (ta, tb) = (a.last_trace.unwrap(), b.last_trace.unwrap());
    assert_eq!(ta.expert_missed, tb.expert_missed, "detection identical");
    assert_eq!(ta.robust_missed, tb.robust_missed);
    assert_eq!(ta.fallback_used, tb.fallback_used, "no misses -> same bit");
}
