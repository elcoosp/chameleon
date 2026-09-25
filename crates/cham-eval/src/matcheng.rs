//! Duplicate-deck match engine (SPECS/08 §2).
//!
//! ```text
//! profit(d) = (net_hero_seatA(d) + net_hero_seatB(d)) / 2
//! ```
//! — the SUM of the two seatings' nets (seat advantages CANCEL by adding; v1's
//! "(net1 − net2)/2" was garbled). σ over per-deal profits, clusterable by session.
//! Opponent stream per deal: `child(base_seed, "oppA{d}")` — deterministic given
//! the deal index, identical across hero configs, which is what makes A/B paired.

use serde::{Deserialize, Serialize};

use cham_core::card::Deck;
use cham_core::engine::config::EngineConfig;
use cham_core::engine::State;
use cham_core::obs::{Agent, Observables, Player};
use cham_core::rng::{child, Rng};
use cham_opponents::factory::build;
use cham_opponents::factory::OpponentSpecDto;
use cham_opponents::OpponentSpec;
use cham_rec::Recorder;

use crate::EvalError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatchSpec {
    pub opponent: OpponentSpecDto,
    pub deals: u64,
    pub depth_bb: i64,
    pub base_seed: u64,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatchResult {
    pub mb_per_seating: f64,
    pub se_mb: f64,
    pub seatings: u64,
    pub vr_factor: f64,
    pub per_deal_profits: Option<Vec<f64>>,
    pub wall_s: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PoolResult {
    pub per_opponent: Vec<(String, f64, f64)>, // id, mb, se
    pub total_seatings: u64,
}

/// Play ONE seating from `state`; `hero_seat`'s actions come from `hero`, the
/// other seat's from `opp`. Returns hero net (chips).
fn play_seating(
    state: &mut State,
    hero_seat: usize,
    hero: &mut dyn Agent,
    opp: &mut dyn Agent,
    rng: &mut Rng,
) -> Result<i64, EvalError> {
    let mut guard = 0;
    while !state.is_terminal() {
        guard += 1;
        if guard > 400 {
            return Err(EvalError::Match("hand did not terminate".into()));
        }
        let seat = state.to_act();
        let obs = Observables::view(state, Player::from_usize(seat));
        let a = if seat == hero_seat {
            hero.act(&obs, rng)
        } else {
            opp.act(&obs, rng)
        };
        state.apply(a).map_err(|e| EvalError::Match(format!("illegal action: {e}")))?;
    }
    Ok(state.payoffs()[hero_seat])
}

/// MatchRunner: duplicate deals + flight records.
pub struct MatchRunner;

impl MatchRunner {
    /// Run a duplicate match: deal d is played twice with seats swapped.
    #[allow(clippy::too_many_arguments)]
    pub fn run<F>(
        spec: &MatchSpec,
        hero_factory: &F,
        rec: Option<&mut Recorder>,
    ) -> Result<MatchResult, EvalError>
    where
        F: Fn() -> Box<dyn Agent>,
    {
        let t0 = std::time::Instant::now();
        let engine_cfg = EngineConfig::depth(spec.depth_bb);
        let opp_spec = OpponentSpec::parse(&spec.opponent.0)
            .map_err(|e| EvalError::Match(format!("spec: {e}")))?;
        let mut opp = build(&opp_spec, cham_opponents::PercentileChart::global());
        let mut profits: Vec<f64> = Vec::with_capacity(spec.deals as usize);
        for d in 0..spec.deals {
            // ONE shuffle per deal — both seatings see the identical deck
            let deal_rng = &mut child(spec.base_seed, &format!("d{d}"));
            let deck = Deck::shuffled(deal_rng);
            // identical opponent streams per (deal, seating) across hero configs
            let opp_rng_a = &mut child(spec.base_seed, &format!("oppA{d}"));
            let opp_rng_b = &mut child(spec.base_seed, &format!("oppB{d}"));
            let mut hero_a = hero_factory();
            let mut hero_b = hero_factory();
            // seating A: hero at seat 0 (SB)
            let mut state_a = State::new(engine_cfg, deck).map_err(|e| EvalError::Match(format!("{e}")))?;
            let net_a = play_seating(&mut state_a, 0, hero_a.as_mut(), opp.as_mut(), opp_rng_a)?;
            // seating B: SAME deck, hero at seat 1 (BB) — seats swapped
            let mut state_b = State::new(engine_cfg, deck).map_err(|e| EvalError::Match(format!("{e}")))?;
            let net_b = play_seating(&mut state_b, 1, hero_b.as_mut(), opp.as_mut(), opp_rng_b)?;
            // THE formula (v2 fix): sum cancels seat advantage
            profits.push((net_a + net_b) as f64 / 2.0 / 100.0); // bb
        }
        let mb: Vec<f64> = profits.iter().map(|p| p * 1000.0).collect();
        let m = crate::stats::mean(&mb);
        let se = crate::stats::se(&mb);
        if let Some(r) = rec {
            use cham_rec::schema::RecordKind;
            r.record(
                RecordKind::Match,
                serde_json::json!({
                    "label": spec.label,
                    "spec_ids": [spec.opponent.0.clone()],
                    "seeds": [spec.base_seed],
                    "deals": spec.deals,
                    "seatings": spec.deals * 2,
                    "mb_per_seating": m,
                    "se_mb": se,
                    "vr_factor": 1.0,
                    "wall_s": t0.elapsed().as_secs_f64(),
                }),
            )
            .map_err(|e| EvalError::Match(format!("record: {e}")))?;
        }
        Ok(MatchResult {
            mb_per_seating: m,
            se_mb: se,
            seatings: spec.deals * 2,
            vr_factor: 1.0,
            per_deal_profits: Some(mb),
            wall_s: t0.elapsed().as_secs_f64(),
        })
    }

    /// Run the hero against a pool of opponents.
    pub fn run_pool<F>(
        hero_factory: &F,
        pool: &[OpponentSpec],
        deals_per_opp: u64,
        base_seed: u64,
        depth_bb: i64,
        mut rec: Option<&mut Recorder>,
    ) -> Result<PoolResult, EvalError>
    where
        F: Fn() -> Box<dyn Agent>,
    {
        let mut out = PoolResult::default();
        for (i, opp) in pool.iter().enumerate() {
            let spec = MatchSpec {
                opponent: OpponentSpecDto(opp.id()),
                deals: deals_per_opp,
                depth_bb,
                base_seed: base_seed ^ ((i as u64) << 32),
                label: format!("pool:{}/{}", opp.id(), i),
            };
            let r = Self::run(&spec, hero_factory, rec.as_deref_mut())?;
            out.per_opponent.push((opp.id(), r.mb_per_seating, r.se_mb));
            out.total_seatings += r.seatings;
        }
        Ok(out)
    }
}
