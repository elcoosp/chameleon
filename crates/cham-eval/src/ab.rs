//! A/B runner (SPECS/08 §6): paired diffs per deal, SPRT optional, Holm family,
//! promotion writes the ledger + baseline.toml atomically.

use serde::{Deserialize, Serialize};

use cham_rec::Recorder;

use crate::EvalError;
use crate::matcheng::{MatchRunner, MatchSpec};
use crate::stats::{SprtState, holm, paired_ci, sprrt};
use cham_opponents::factory::OpponentSpecDto;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SprtParams {
    pub delta0_mb: f64,
    pub delta1_mb: f64,
    pub alpha: f64,
    pub beta: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AbSpec {
    /// A/B pair as OpponentSpec-independent mode labels (cham-cli maps to agents)
    pub a: String,
    pub b: String,
    pub deals_per_opp: u64,
    pub seeds: Vec<u64>,
    pub conf: f64,
    pub margin_mb: f64,
    pub sprt: Option<SprtParams>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PerOppDelta {
    pub opponent: String,
    pub delta_mb: f64,
    pub ci: (f64, f64),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AbVerdict {
    pub delta_mb: f64,
    pub ci: (f64, f64),
    pub per_opp: Vec<PerOppDelta>,
    pub sprt: Option<SprtState>,
    pub promote: bool,
    pub rule: String,
}

/// Run one A/B arm over the pool; returns per-deal paired diffs vs the OTHER arm.
/// The runner is generic over the hero factories (cham-cli wires the modes).
pub struct AbRunner;

impl AbRunner {
    #[allow(clippy::too_many_arguments)]
    pub fn run<F>(
        spec: &AbSpec,
        pool: &[cham_opponents::OpponentSpec],
        hero_factory_a: &F,
        hero_factory_b: &F,
        depth_bb: i64,
        rec: Option<&mut Recorder>,
    ) -> Result<AbVerdict, EvalError>
    where
        F: Fn() -> Box<dyn Agent>,
    {
        let mut per_opp = Vec::new();
        let mut all_diffs: Vec<f64> = Vec::new();
        for (i, opp) in pool.iter().enumerate() {
            let mk = |arm_seed: u64| MatchSpec {
                opponent: OpponentSpecDto(opp.id()),
                deals: spec.deals_per_opp,
                depth_bb,
                base_seed: arm_seed ^ ((i as u64) << 32),
                label: format!("ab:{}/{}/{}", spec.a, spec.b, opp.id()),
            };
            let ra = MatchRunner::run(&mk(spec.seeds[0]), hero_factory_a, None)?;
            let rb = MatchRunner::run(&mk(spec.seeds[0]), hero_factory_b, None)?;
            // paired diffs per deal (identical opponent streams)
            let da = ra.per_deal_profits.clone().unwrap_or_default();
            let db = rb.per_deal_profits.clone().unwrap_or_default();
            let diffs: Vec<f64> = da.iter().zip(db.iter()).map(|(x, y)| x - y).collect();
            let rng = &mut cham_core::rng::rng_from_seed(spec.seeds[0] ^ 0xAB);
            let ci = paired_ci(&diffs, spec.conf, rng);
            let delta = crate::stats::mean(&diffs);
            per_opp.push(PerOppDelta {
                opponent: opp.id(),
                delta_mb: delta,
                ci,
            });
            all_diffs.extend(diffs);
        }
        let rng = &mut cham_core::rng::rng_from_seed(spec.seeds[0]);
        let ci = paired_ci(&all_diffs, spec.conf, rng);
        let delta = crate::stats::mean(&all_diffs);
        // SPRT (screening arms)
        let sprt = match &spec.sprt {
            Some(p) => Some(sprrt(
                &all_diffs,
                p.delta0_mb,
                p.delta1_mb,
                p.alpha,
                p.beta,
            )?),
            None => None,
        };
        // verdict rule: paired CI lower > margin
        let promote = ci.0 > spec.margin_mb
            && sprt
                .as_ref()
                .map(|s| !matches!(s, SprtState::AcceptH0))
                .unwrap_or(true);
        if let Some(r) = rec {
            use cham_rec::schema::RecordKind;
            r.record(
                RecordKind::LedgerEntry,
                serde_json::json!({
                    "type": "ab",
                    "promote": promote,
                    "a": spec.a,
                    "b": spec.b,
                    "delta_mb": delta,
                    "ci": ci,
                    "rule": "paired CI lower > margin",
                }),
            )
            .map_err(|e| EvalError::Ledger(format!("{e}")))?;
        }
        Ok(AbVerdict {
            delta_mb: delta,
            ci,
            per_opp,
            sprt,
            promote,
            rule: "paired CI lower > margin_mb".into(),
        })
    }

    /// Shared-hero variant (B1): one instance per arm plays the whole pool
    /// (per-hand lifecycle via `on_hand_end`, as in live `play`). Stateless
    /// baselines behave identically to `run`.
    pub fn run_shared(
        spec: &AbSpec,
        pool: &[cham_opponents::OpponentSpec],
        hero_a: &mut dyn Agent,
        hero_b: &mut dyn Agent,
        depth_bb: i64,
        rec: Option<&mut Recorder>,
    ) -> Result<AbVerdict, EvalError> {
        let mut per_opp = Vec::new();
        let mut all_diffs: Vec<f64> = Vec::new();
        for (i, opp) in pool.iter().enumerate() {
            let mk = |arm_seed: u64| MatchSpec {
                opponent: OpponentSpecDto(opp.id()),
                deals: spec.deals_per_opp,
                depth_bb,
                base_seed: arm_seed ^ ((i as u64) << 32),
                label: format!("ab:{}/{}/{}", spec.a, spec.b, opp.id()),
            };
            let ra = MatchRunner::run_shared(&mk(spec.seeds[0]), hero_a, None)?;
            let rb = MatchRunner::run_shared(&mk(spec.seeds[0]), hero_b, None)?;
            // paired diffs per deal (identical opponent streams)
            let da = ra.per_deal_profits.clone().unwrap_or_default();
            let db = rb.per_deal_profits.clone().unwrap_or_default();
            let diffs: Vec<f64> = da.iter().zip(db.iter()).map(|(x, y)| x - y).collect();
            let rng = &mut cham_core::rng::rng_from_seed(spec.seeds[0] ^ 0xAB);
            let ci = paired_ci(&diffs, spec.conf, rng);
            let delta = crate::stats::mean(&diffs);
            per_opp.push(PerOppDelta {
                opponent: opp.id(),
                delta_mb: delta,
                ci,
            });
            all_diffs.extend(diffs);
        }
        let rng = &mut cham_core::rng::rng_from_seed(spec.seeds[0]);
        let ci = paired_ci(&all_diffs, spec.conf, rng);
        let delta = crate::stats::mean(&all_diffs);
        // SPRT (screening arms)
        let sprt = match &spec.sprt {
            Some(p) => Some(sprrt(
                &all_diffs,
                p.delta0_mb,
                p.delta1_mb,
                p.alpha,
                p.beta,
            )?),
            None => None,
        };
        // verdict rule: paired CI lower > margin
        let promote = ci.0 > spec.margin_mb
            && sprt
                .as_ref()
                .map(|s| !matches!(s, SprtState::AcceptH0))
                .unwrap_or(true);
        if let Some(r) = rec {
            use cham_rec::schema::RecordKind;
            r.record(
                RecordKind::LedgerEntry,
                serde_json::json!({
                    "type": "ab",
                    "promote": promote,
                    "a": spec.a,
                    "b": spec.b,
                    "delta_mb": delta,
                    "ci": ci,
                    "rule": "paired CI lower > margin",
                }),
            )
            .map_err(|e| EvalError::Ledger(format!("{e}")))?;
        }
        Ok(AbVerdict {
            delta_mb: delta,
            ci,
            per_opp,
            sprt,
            promote,
            rule: "paired CI lower > margin_mb".into(),
        })
    }

    /// Holm correction over a pre-registered gate family (SPECS/10 §4).
    pub fn holm_family(pvals: &[f64], alpha: f64) -> Vec<bool> {
        holm(pvals, alpha)
    }
}

use cham_core::obs::Agent;
