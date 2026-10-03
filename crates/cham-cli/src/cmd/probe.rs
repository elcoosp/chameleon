//! `chameleon probe` (SPECS/09 §2): Tier 1 — LBR proxy, coverage, router metrics.
//! Owned by cham-eval; computed via cham-blueprint::lbr + router metrics.
//!
//! ## P1 diagnostic mode (`--diag-fallback`)
//!
//! The 2026-09-26 full-abstraction ladder fell back on 26.7% of decisions
//! (2010/7540), while the tiny abstraction achieved 0%. This mode loads a
//! bundle from `--bundle` (default `artifacts/agent`), drives a small match
//! against each opponent in `config/pool.toml`, and prints per-expert
//! strategy-miss counts so we can see WHICH expert misses and WHERE.

use std::path::Path;

use cham_agent::modes::{AgentMode, SearchCfg};
use cham_agent::pipeline::ChameleonAgent;
use cham_core::card::{Card, Deck};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::history::{HandHistory, PublicHistory};
use cham_core::engine::{Action, State, Street};
use cham_core::obs::{Agent as _, Observables, Player};
use cham_core::rng::child;
use cham_router::model::SoftmaxModel;
use cham_router::runtime::RouterRuntime;

pub fn run(agent: &str, diag_fallback: bool, bundle: Option<&str>, search: bool) -> i32 {
    // 2026-10-01: refuse unknown agent names (see STALE-BINARY-GOTCHA-2026-10-01.md).
    if !crate::cmd::guard::is_known_agent(agent) {
        eprintln!(
            "probe: agent '{agent}' is not a recognized agent name in this binary.\n\
             probe: known trained: {:?}\n\
             probe: known baselines: {:?}",
            crate::cmd::guard::trained_agents(),
            crate::cmd::guard::BASELINE_AGENTS,
        );
        return crate::cmd::EXIT_FAIL;
    }
    let resolved = crate::cmd::guard::resolve_agent_bundle();
    let bundle = bundle.unwrap_or_else(|| resolved.to_str().unwrap_or("artifacts/agent"));
    if diag_fallback {
        return run_diag(agent, bundle, search);
    }
    // PERF-PLAN T7 guardrail: probing a trained agent without its bundle
    // yields silent-fallback numbers that look like bot bugs.
    if let Err(missing) = crate::cmd::guard::require_agent_artifacts(agent) {
        eprintln!("probe: agent '{agent}' needs trained artifacts, missing:");
        for m in &missing {
            eprintln!("probe:   {m}");
        }
        eprintln!("probe: train them with train-buckets + train-bp (robust + 4 experts) first");
        return crate::cmd::EXIT_BUDGET;
    }
    // coverage + lbr on the tiny abstraction (calibrated artifacts when present)
    let cfg = cham_engine::config::AbstractionConfig::tiny();
    let mut enc = match cham_engine::Encoder::from_artifacts_dir(
        std::path::Path::new("artifacts/buckets-tiny"),
        cfg.clone(),
    ) {
        Ok(e) => e,
        Err(_) => {
            eprintln!("probe: artifacts/buckets-tiny missing — run train-buckets first");
            return crate::cmd::EXIT_BUDGET;
        }
    };
    let engine = cham_core::engine::config::EngineConfig::depth(100);
    let mut uniform = |obs: &cham_core::obs::Observables<'_>,
                       _seq: &cham_engine::encoder::ActionSeq|
     -> Vec<(cham_core::engine::Action, f64)> {
        let n = obs.legal.len().max(1) as f64;
        obs.legal.iter().map(|la| (la.action, 1.0 / n)).collect()
    };
    let report = match cham_blueprint::lbr::lbr_vs(&mut uniform, 1, engine, &mut enc, 100, 0x90BE) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("probe lbr: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    let lbr_mb = report.lbr_mb_per_hand;
    // L-10 fix (2026-09-27): the previous verdict used hardcoded constants
    // (`coverage = 0.91`, `acc_b_dev = 0.84`) so the coverage half of the
    // gate was always true, and the printed numbers were pure decoration.
    // The `probe` subcommand has no router-dataset access to measure those
    // values, so we (a) drop the tautological coverage condition and (b)
    // print the coverage/acc as "n/a" rather than the fake constants. If a
    // later change adds a real router metrics endpoint, wire it here.
    let lbr_gate_mb = 60_000.0f64;
    let verdict = if lbr_mb.abs() < lbr_gate_mb {
        "PASS"
    } else {
        "FAIL"
    };
    println!(
        "probe: {verdict} (lbr {lbr_mb:.0} mb/hand, gate |lbr| < {lbr_gate_mb:.0}; \
         cov/acc_b_dev are NOT measured by this subcommand — see router's own \
         `train-router` gate) [{agent}]"
    );
    if verdict == "PASS" {
        crate::cmd::EXIT_OK
    } else {
        crate::cmd::EXIT_FAIL
    }
}

// ---------------- P1 diagnostic ----------------

#[derive(Default, Debug)]
struct DiagStats {
    decisions: u64,
    fallback_used: u64,
    expert_miss: [u64; 4],
    robust_miss: u64,
    reach_mass_zero: u64,
    mix_zero: u64,
    /// Diagnostic (2026-09-30): argmax chose expert k, k missed, robust
    /// covered. The action came from robust but `fallback_used` was false.
    /// See `ARGMAX-FALLBACK-REALLY-MATTERS-2026-09-30.md`.
    expert_missed_robust_covered: u64,
}

fn pool_ids(pool_path: &str) -> Vec<String> {
    let txt = match std::fs::read_to_string(pool_path) {
        Ok(t) => t,
        Err(_) => return vec!["callbot".into()],
    };
    let val: toml::Value = match toml::from_str(&txt) {
        Ok(v) => v,
        Err(_) => return vec!["callbot".into()],
    };
    let ids: Vec<String> = val
        .get("opponents")
        .and_then(|o| o.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|o| o.get("id").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        vec!["callbot".into()]
    } else {
        ids
    }
}

fn run_diag(agent: &str, bundle: &str, search: bool) -> i32 {
    let routing = crate::cmd::hero::routing_for(agent);
    let loaded = match cham_agent::loader::load_agent(Path::new(bundle), routing, 100) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("probe --diag-fallback: bundle '{bundle}' not loadable: {e}");
            return crate::cmd::EXIT_BUDGET;
        }
    };
    // 2026-10-01 (F1): `--search` opts in to live river solving. Uses
    // `EXP-SEARCH` as the auditable G4 ledger token (SPECS/06 §7).
    let mode = AgentMode {
        routing: routing.to_string(),
        search: SearchCfg {
            enabled: search,
            solver: "Rnr".into(),
            g4_ledger_ref: if search {
                "EXP-SEARCH".into()
            } else {
                String::new()
            },
        },
        fallback_mode: std::env::var("CHAM_FALLBACK_MODE").unwrap_or_else(|_| "renorm".into()),
    };
    let router = match std::fs::read(Path::new(bundle).join("router.bin")) {
        Ok(bytes) => match RouterRuntime::from_model_bytes(&bytes) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("probe --diag-fallback: router: {e}");
                return crate::cmd::EXIT_FAIL;
            }
        },
        Err(_) => RouterRuntime::new(SoftmaxModel::new(20, 4), 0.7, 8.0, 0.5, -1.5),
    };
    let mut bot = match ChameleonAgent::new(
        mode,
        loaded.encoder,
        router,
        loaded.experts,
        loaded.robust,
        loaded.bayes,
        None,
    ) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("probe --diag-fallback: agent: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };

    let deals: u64 = std::env::var("DIAG_DEALS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);
    let pool_path = std::env::var("DIAG_POOL").unwrap_or_else(|_| "config/pool.toml".into());
    let opps = pool_ids(&pool_path);

    println!(
        "probe --diag-fallback: bundle={bundle} agent={agent} deals/opponent={deals} pool={pool_path}"
    );
    println!(
        "  {:<26} {:>6} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "opponent",
        "decis",
        "fb_used",
        "rc_used",
        "e0_miss",
        "e1_miss",
        "e2_miss",
        "e3_miss",
        "r_miss",
        "reach0"
    );
    let mut total = DiagStats::default();
    for (i, opp_id) in opps.iter().enumerate() {
        let stats = run_diag_inner(&mut bot, opp_id, deals, 0x01AD ^ ((i as u64) << 32));
        total.decisions += stats.decisions;
        total.fallback_used += stats.fallback_used;
        for k in 0..4 {
            total.expert_miss[k] += stats.expert_miss[k];
        }
        total.robust_miss += stats.robust_miss;
        total.reach_mass_zero += stats.reach_mass_zero;
        total.mix_zero += stats.mix_zero;
        total.expert_missed_robust_covered += stats.expert_missed_robust_covered;
        println!(
            "  {:<26} {:>6} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9}",
            opp_id,
            stats.decisions,
            stats.fallback_used,
            stats.expert_missed_robust_covered,
            stats.expert_miss[0],
            stats.expert_miss[1],
            stats.expert_miss[2],
            stats.expert_miss[3],
            stats.robust_miss,
            stats.reach_mass_zero,
        );
    }
    println!(
        "  {:<26} {:>6} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "TOTAL",
        total.decisions,
        total.fallback_used,
        total.expert_missed_robust_covered,
        total.expert_miss[0],
        total.expert_miss[1],
        total.expert_miss[2],
        total.expert_miss[3],
        total.robust_miss,
        total.reach_mass_zero,
    );
    if total.decisions > 0 {
        let pct = 100.0 * total.fallback_used as f64 / total.decisions as f64;
        println!(
            "  fallback rate: {:.1}%  mix_zero: {}  reach_mass_zero: {}",
            pct, total.mix_zero, total.reach_mass_zero
        );
    }
    crate::cmd::EXIT_OK
}

fn run_diag_inner(bot: &mut ChameleonAgent, opp_id: &str, deals: u64, base_seed: u64) -> DiagStats {
    let mut stats = DiagStats::default();
    let cfg = EngineConfig::depth(100);
    // NOTE (2026-09-26, v3 repair): `factory::build` takes `&OpponentSpec`;
    // parse the id first (was passing the DTO — pre-existing breakage in the
    // in-progress diag work, fixed minimally to restore compilation).
    let opp_spec = match cham_opponents::OpponentSpec::from_dto(
        &cham_opponents::factory::OpponentSpecDto(opp_id.to_string()),
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("probe diag: bad opponent id '{opp_id}': {e}");
            return DiagStats::default();
        }
    };
    let mut opp =
        cham_opponents::factory::build(&opp_spec, cham_opponents::PercentileChart::global());
    for d in 0..deals {
        let mut deal_rng = child(base_seed, &format!("deal{d}"));
        let deck = Deck::shuffled(&mut deal_rng);
        let mut state = match State::new(cfg, deck) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut log: Vec<(Street, Player, Action)> = Vec::new();
        let mut guard = 0;
        while !state.is_terminal() && guard < 400 {
            guard += 1;
            let seat = state.to_act();
            let player = Player::from_usize(seat);
            let obs = Observables::view(&state, player);
            let a = if seat == 0 {
                let action = bot.act(&obs, &mut deal_rng);
                if let Some(t) = bot.last_trace.as_ref() {
                    stats.decisions += 1;
                    for k in 0..4 {
                        if t.expert_missed[k] {
                            stats.expert_miss[k] += 1;
                        }
                    }
                    if t.robust_missed {
                        stats.robust_miss += 1;
                    }
                    if t.reach_mass_zero {
                        stats.reach_mass_zero += 1;
                    }
                    if t.mix_zero {
                        stats.mix_zero += 1;
                    }
                    if t.fallback_used {
                        stats.fallback_used += 1;
                    }
                    if t.expert_missed_robust_covered {
                        stats.expert_missed_robust_covered += 1;
                    }
                }
                action
            } else {
                opp.act(&obs, &mut deal_rng)
            };
            {
                let hero_obs = Observables::view(&state, Player::from_usize(0));
                bot.on_public_action(&hero_obs, player, a);
            }
            {
                let opp_obs = Observables::view(&state, Player::from_usize(1));
                opp.on_public_action(&opp_obs, player, a);
            }
            log.push((state.street(), player, a));
            if state.apply(a).is_err() {
                break;
            }
        }
        let n = state.board_len() as usize;
        let mut board = [Card(0); 5];
        board[..n].copy_from_slice(&state.board()[..n]);
        let hh = HandHistory {
            seed: base_seed ^ d,
            actions: log,
            cfg: state.cfg(),
            holes: [state.hole(0), state.hole(1)],
            board,
            board_len: state.board_len(),
            result_sb: state.payoffs()[0],
        };
        let ph = PublicHistory::from(&hh);
        bot.on_hand_end(&ph, state.payoffs()[0]);
        opp.on_hand_end(&ph, state.payoffs()[1]);
    }
    stats
}
