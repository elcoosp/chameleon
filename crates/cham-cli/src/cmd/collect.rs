//! `chameleon collect` (SPECS/05 §4): build the binary router dataset.
//!
//! Two modes:
//!
//! * **Synthetic** (default): the original `TrackerStub` produces a feature
//!   vector whose class is encoded directly in one dimension. The resulting
//!   model passes its gates by reading the answer key; the gate is vacuous.
//!   Kept for smoke tests and CI.
//!
//! * **Real** (`--real`): runs the shipped agent against the four archetypes
//!   through the actual engine, records its per-hand tracker features, and
//!   labels the row with the archetype. This is the honest training set for
//!   the router; its gate FAILS at 0.761 top-1 / 0.45 TAG recall because the
//!   20-dim feature vector does not separate TAG from LAG.
//!   See docs/plans/ROUTER-FAILS-ON-REAL-DATA-2026-09-29.md.

use cham_core::card::Deck;
use cham_core::engine::config::EngineConfig;
use cham_core::engine::history::{HandHistory, PublicHistory};
use cham_core::engine::{Action, State};
use cham_core::obs::{Agent, Observables, Player};
use cham_core::rng::{Rng, child};
use cham_router::dataset::RbinRow;
use std::path::Path;

pub fn run(
    out: &str,
    max_rows: usize,
    real: bool,
    bundle: &str,
    sessions: u64,
    hands: u64,
) -> i32 {
    if real {
        return run_real(out, bundle, sessions, hands, max_rows);
    }
    run_synthetic(out, max_rows)
}

// ---------------- synthetic (stub) ----------------

fn run_synthetic(out: &str, max_rows: usize) -> i32 {
    let mut rows: Vec<RbinRow> = Vec::new();
    let mut session_id = 1u16;
    let mut rng = cham_core::rng::rng_from_seed(0xC011EC7);
    for arch in 0..4usize {
        for _session in 0..625 {
            let mut t = TrackerStub {
                hands: 60 + (session_id as u64 * 17) % 400,
            };
            for _hand in 0..200 {
                if rows.len() >= max_rows {
                    break;
                }
                let features = t.next_features(&mut rng, arch);
                let family = if cham_router::dataset::split_of_session(session_id)
                    == cham_router::dataset::SESSION_C
                {
                    1
                } else {
                    0
                };
                rows.push(RbinRow {
                    features,
                    label: arch as u8,
                    session_id,
                    family,
                });
            }
            session_id += 1;
        }
    }
    if rows.len() > max_rows {
        rows.truncate(max_rows);
    }
    match cham_router::write_dataset(std::path::Path::new(out), &rows, 20) {
        Ok(()) => {
            println!("collect (synthetic): {} rows -> {out}", rows.len());
            crate::cmd::EXIT_OK
        }
        Err(e) => {
            eprintln!("collect: {e}");
            crate::cmd::EXIT_FAIL
        }
    }
}

// ---------------- real (instrumented) ----------------

fn build_agent(bundle: &str, routing: &str) -> cham_agent::ChameleonAgent {
    let loaded = cham_agent::loader::load_agent(Path::new(bundle), routing, 100)
        .expect("load_agent");
    let mode = cham_agent::modes::AgentMode {
        routing: routing.to_string(),
        search: cham_agent::modes::SearchCfg {
            enabled: false,
            solver: "Rnr".into(),
            g4_ledger_ref: String::new(),
        },
        fallback_mode: "renorm".into(),
    };
    let router = match std::fs::read(Path::new(bundle).join("router.bin")) {
        Ok(b) => cham_router::runtime::RouterRuntime::from_model_bytes(&b).expect("router"),
        Err(_) => cham_router::runtime::RouterRuntime::new(
            cham_router::model::SoftmaxModel::new(20, 4),
            0.7,
            8.0,
            0.5,
            -1.5,
        ),
    };
    cham_agent::ChameleonAgent::new(
        mode,
        loaded.encoder,
        router,
        loaded.experts,
        loaded.robust,
        loaded.bayes,
        None,
    )
    .expect("agent")
}

/// Play one hand with `hero` at `hero_seat`. The opponent plays the other
/// seat. This must ALTERNATE across sessions (see
/// docs/plans/INSTRUMENT-SEAT-ASYMMETRY-2026-09-29.md): running hero only
/// at SB biases the tracker toward the preflop-second-actor slice of the
/// opponent's behaviour and kills 9 of 20 router features.
#[allow(clippy::too_many_arguments)]
fn play_one_hand(
    hero: &mut cham_agent::ChameleonAgent,
    opp: &mut Box<dyn Agent>,
    engine_cfg: EngineConfig,
    hand_seed: u64,
    hero_seat: usize,
) -> i64 {
    let opp_seat = 1 - hero_seat;
    let mut hero_rng: Rng = child(hand_seed, "h");
    let deck = Deck::shuffled(&mut child(hand_seed, "d"));
    let mut state = State::new(engine_cfg, deck).expect("state");
    let mut log: Vec<(cham_core::engine::Street, Player, Action)> = Vec::new();
    let mut guard = 0;
    while !state.is_terminal() && guard < 400 {
        guard += 1;
        let p = state.to_act();
        let obs = Observables::view(&state, Player::from_usize(p));
        let a = if p == hero_seat {
            hero.act(&obs, &mut hero_rng)
        } else {
            opp.act(&obs, &mut hero_rng)
        };
        // Feed to hero from HERO's view, BEFORE apply (matches MatchRunner).
        let hero_obs = Observables::view(&state, Player::from_usize(hero_seat));
        hero.on_public_action(&hero_obs, Player::from_usize(p), a);
        log.push((state.street(), Player::from_usize(p), a));
        state.apply(a).expect("legal");
    }
    let payoffs = state.payoffs();
    let n = state.board_len() as usize;
    let mut board = [cham_core::card::Card(0); 5];
    board[..n].copy_from_slice(&state.board()[..n]);
    let hh = HandHistory {
        seed: hand_seed,
        actions: log,
        cfg: engine_cfg,
        holes: [state.hole(0), state.hole(1)],
        board,
        board_len: state.board_len(),
        result_sb: payoffs[0],
    };
    let ph = PublicHistory::from(&hh);
    hero.on_hand_end(&ph, payoffs[hero_seat]);
    payoffs[hero_seat]
}

fn run_real(out: &str, bundle: &str, sessions: u64, hands: u64, max_rows: usize) -> i32 {
    let opponents = ["arch:nit", "arch:tag", "arch:lag", "arch:station"];
    let engine_cfg = EngineConfig::depth(100);
    let mut all_rows: Vec<RbinRow> = Vec::new();
    let mut next_session: u16 = 1;

    for (k, opp_id) in opponents.iter().enumerate() {
        let label = k as u8;
        for s in 0..sessions {
            if all_rows.len() >= max_rows {
                break;
            }
            let session_id = next_session;
            next_session = next_session.wrapping_add(1);
            // Alternate hero seat per session: SB (0) on even sessions,
            // BB (1) on odd. This is required so the tracker sees the
            // opponent's full action menu, not just the preflop-second
            // slice. See INSTRUMENT-SEAT-ASYMMETRY-2026-09-29.md.
            let hero_seat = (s % 2) as usize;
            let mut hero = build_agent(bundle, "argmax");
            let opp_spec = cham_opponents::OpponentSpec::parse(opp_id).expect("opp spec");
            let mut opp: Box<dyn Agent> = cham_opponents::factory::build(
                &opp_spec,
                cham_opponents::PercentileChart::global(),
            );
            for h in 0..hands {
                if all_rows.len() >= max_rows {
                    break;
                }
                let hand_seed = 0xC011EC7u64 ^ ((k as u64) << 40) ^ (s << 24) ^ h;
                let _ = play_one_hand(&mut hero, &mut opp, engine_cfg, hand_seed, hero_seat);
                let feats = hero.tracker_features().to_vec();
                all_rows.push(RbinRow {
                    features: feats,
                    label,
                    session_id,
                    family: 0,
                });
            }
        }
        eprintln!("  {opp_id}: {} rows cumulative", all_rows.len());
    }

    match cham_router::write_dataset(Path::new(out), &all_rows, 20) {
        Ok(()) => {
            println!("collect (real): {} rows -> {out}", all_rows.len());
            crate::cmd::EXIT_OK
        }
        Err(e) => {
            eprintln!("collect: {e}");
            crate::cmd::EXIT_FAIL
        }
    }
}

/// Deterministic synthetic feature synthesizer (see module doc).
struct TrackerStub {
    hands: u64,
}

impl TrackerStub {
    fn next_features(&mut self, rng: &mut Rng, arch: usize) -> Vec<f32> {
        let mut f = vec![0.5f32; 20];
        for (i, v) in f.iter_mut().enumerate() {
            let x = cham_core::rng::next_f64(rng);
            *v = (0.35 + 0.3 * x + 0.05 * ((self.hands as f64 + i as f64).sin())).clamp(0.0, 1.0)
                as f32;
        }
        f[0] = cham_router::features::maturity_feature(self.hands) as f32;
        let sig = 1 + (arch % 4);
        f[sig] = (f[sig] + 0.45).min(1.0);
        f
    }
}
