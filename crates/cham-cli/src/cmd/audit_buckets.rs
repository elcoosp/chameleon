//! EXP-017 bucket-quality audit CLI.
//!
//! Two modes:
//! - `--input <file>` (default): read `{"hands":[{"bucket":u32,"ev":f64}]}`
//!   and run `cham_engine::audit::audit_bucket_quality`.
//! - `--generate`: drive a short match against `--pool`, record
//!   `(hero-bucket, terminal hero net)` at each flop/turn hero decision,
//!   write the JSON, and audit it in place. Closes the EXP-017 gap: nothing
//!   in the tree produced the audit file before.

use std::path::Path;

use serde_json::json;

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

const HERO_SEAT: usize = 0;
const DEPTH_BB: i64 = 100;

pub fn run(input: &str) -> i32 {
    let hands = match read_hands(input) {
        Ok(h) => h,
        Err(code) => return code,
    };
    report(&hands)
}

pub fn run_generate(bundle: &str, pool: &str, deals: u64, out: &str) -> i32 {
    let mut bot = match build_bot(bundle, "full", DEPTH_BB) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("audit-buckets --generate: bundle '{bundle}': {e}");
            return crate::cmd::EXIT_BUDGET;
        }
    };

    let opp_ids = read_pool_ids(pool);
    if opp_ids.is_empty() {
        eprintln!("audit-buckets --generate: pool '{pool}' has no [[opponents]] with an id");
        return crate::cmd::EXIT_FAIL;
    }

    let cfg = EngineConfig::depth(DEPTH_BB);
    let mut pairs: Vec<(u32, f64)> = Vec::new();
    let mut flop_n: usize = 0;
    let mut turn_n: usize = 0;

    for (i, opp_id) in opp_ids.iter().enumerate() {
        let opp_spec = match cham_opponents::OpponentSpec::parse(opp_id) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("audit-buckets --generate: opponent '{opp_id}': {e}");
                continue;
            }
        };
        let mut opp =
            cham_opponents::factory::build(&opp_spec, cham_opponents::PercentileChart::global());
        let base_seed: u64 = 0x0A_017 ^ ((i as u64) << 32);
        for d in 0..deals {
            let mut deal_rng = child(base_seed, &format!("deal{d}"));
            let deck = Deck::shuffled(&mut deal_rng);
            let mut state = match State::new(cfg, deck) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut hand_buckets: Vec<(u32, Street)> = Vec::new();
            let mut log: Vec<(Street, Player, Action)> = Vec::new();
            let mut guard = 0;
            while !state.is_terminal() && guard < 400 {
                guard += 1;
                let seat = state.to_act();
                let player = Player::from_usize(seat);
                let obs = Observables::view(&state, player);

                if seat == HERO_SEAT && matches!(obs.street, Street::Flop | Street::Turn) {
                    let b = bot.encoder.bucket(&obs) as u32;
                    hand_buckets.push((b, obs.street));
                }

                let a = if seat == HERO_SEAT {
                    bot.act(&obs, &mut deal_rng)
                } else {
                    opp.act(&obs, &mut deal_rng)
                };

                {
                    let hero_obs = Observables::view(&state, Player::from_usize(HERO_SEAT));
                    bot.on_public_action(&hero_obs, player, a);
                }
                {
                    let opp_obs = Observables::view(&state, Player::from_usize(1 - HERO_SEAT));
                    opp.on_public_action(&opp_obs, player, a);
                }

                log.push((state.street(), player, a));
                if state.apply(a).is_err() {
                    break;
                }
            }

            if !state.is_terminal() {
                continue;
            }

            let ev = state.payoffs()[HERO_SEAT] as f64;
            for (b, st) in hand_buckets {
                pairs.push((b, ev));
                match st {
                    Street::Flop => flop_n += 1,
                    Street::Turn => turn_n += 1,
                    _ => {}
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
            bot.on_hand_end(&ph, state.payoffs()[HERO_SEAT]);
            opp.on_hand_end(&ph, state.payoffs()[1 - HERO_SEAT]);
        }
    }

    if pairs.is_empty() {
        eprintln!("audit-buckets --generate: no flop/turn hero decisions recorded");
        return crate::cmd::EXIT_FAIL;
    }

    let body = json!({
        "hands": pairs
            .iter()
            .map(|(b, ev)| json!({ "bucket": b, "ev": ev }))
            .collect::<Vec<_>>(),
    });
    let pretty = match serde_json::to_string_pretty(&body) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("audit-buckets --generate: json: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    if let Err(e) = std::fs::write(out, pretty) {
        eprintln!("audit-buckets --generate: write '{out}': {e}");
        return crate::cmd::EXIT_FAIL;
    }
    println!(
        "audit-buckets --generate: wrote {} pairs (flop={} turn={}) -> {out}",
        pairs.len(),
        flop_n,
        turn_n,
    );
    report(&pairs)
}

fn build_bot(bundle: &str, agent: &str, depth_bb: i64) -> Result<ChameleonAgent, String> {
    let routing = crate::cmd::hero::routing_for(agent);
    let loaded = cham_agent::loader::load_agent(Path::new(bundle), routing, depth_bb)
        .map_err(|e| format!("load: {e}"))?;
    let mode = AgentMode {
        routing: routing.to_string(),
        search: SearchCfg {
            enabled: false,
            solver: "Rnr".into(),
            g4_ledger_ref: String::new(),
        },
        fallback_mode: std::env::var("CHAM_FALLBACK_MODE").unwrap_or_else(|_| "renorm".into()),
    };
    let router = match std::fs::read(Path::new(bundle).join("router.bin")) {
        Ok(bytes) => cham_router::runtime::RouterRuntime::from_model_bytes(&bytes)
            .map_err(|e| format!("router: {e}"))?,
        Err(_) => RouterRuntime::new(SoftmaxModel::new(20, 4), 0.7, 8.0, 0.5, -1.5),
    };
    ChameleonAgent::new(
        mode,
        loaded.encoder,
        router,
        loaded.experts,
        loaded.robust,
        loaded.bayes,
        None,
    )
    .map_err(|e| format!("agent: {e}"))
}

fn read_pool_ids(pool_path: &str) -> Vec<String> {
    let txt = match std::fs::read_to_string(pool_path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let val: toml::Value = match toml::from_str(&txt) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    val.get("opponents")
        .and_then(|o| o.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|o| o.get("id").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn read_hands(input: &str) -> Result<Vec<(u32, f64)>, i32> {
    let txt = match std::fs::read_to_string(input) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("audit-buckets: cannot read '{input}': {e}");
            return Err(crate::cmd::EXIT_BUDGET);
        }
    };
    let v: serde_json::Value = match serde_json::from_str(&txt) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("audit-buckets: bad JSON: {e}");
            return Err(crate::cmd::EXIT_FAIL);
        }
    };
    Ok(v.get("hands")
        .and_then(|h| h.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|h| Some((h.get("bucket")?.as_u64()? as u32, h.get("ev")?.as_f64()?)))
                .collect()
        })
        .unwrap_or_default())
}

fn report(hands: &[(u32, f64)]) -> i32 {
    if hands.len() < 2 {
        eprintln!("audit-buckets: need >=2 hands in {{\"hands\":[{{\"bucket\":..,\"ev\":..}}]}}");
        return crate::cmd::EXIT_FAIL;
    }
    let r = cham_engine::audit::audit_bucket_quality(hands);
    println!(
        "audit-buckets: within={:.4} between={:.4} ratio={:.4} (n={})",
        r.within_bucket_var,
        r.between_bucket_var,
        r.ratio,
        hands.len()
    );
    crate::cmd::EXIT_OK
}
