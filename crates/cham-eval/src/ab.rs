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
    /// Mean all-in-EV variance-reduction factor across both arms on this
    /// opponent (v3 §2.1 step 1: first-class VR telemetry — 1.0 means no
    /// all-in runout fired, >1.0 means runout luck was removed).
    #[serde(default)]
    pub vr_factor: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AbVerdict {
    pub delta_mb: f64,
    pub ci: (f64, f64),
    pub per_opp: Vec<PerOppDelta>,
    pub sprt: Option<SprtState>,
    pub promote: bool,
    pub rule: String,
    /// Mean `vr_factor` over opponents (v3 §2.1 step 1).
    #[serde(default)]
    pub vr_factor: f64,
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
        F: Fn() -> Box<dyn Agent> + Sync,
    {
        // Parallel per-opponent (V2 A/B speedup). Each thread computes one
        // arm pair independently: same seed (opponent stream identical
        // across arms), own RNG for the bootstrap CI. Order preserved by
        // collecting handles, then joining in pool order — deterministic
        // results regardless of completion order.
        let per_opp_results: Vec<Result<(PerOppDelta, Vec<f64>), EvalError>> =
            std::thread::scope(|s| {
                let handles: Vec<_> = pool
                    .iter()
                    .enumerate()
                    .map(|(i, opp)| {
                        s.spawn(move || -> Result<(PerOppDelta, Vec<f64>), EvalError> {
                            let mk = |arm_seed: u64| MatchSpec {
                                opponent: OpponentSpecDto(opp.id()),
                                deals: spec.deals_per_opp,
                                depth_bb,
                                base_seed: arm_seed ^ ((i as u64) << 32),
                                label: format!("ab:{}/{}/{}", spec.a, spec.b, opp.id()),
                            };
                            let ra = MatchRunner::run(&mk(spec.seeds[0]), hero_factory_a, None)?;
                            let rb = MatchRunner::run(&mk(spec.seeds[0]), hero_factory_b, None)?;
                            let da = ra.per_deal_profits.clone().unwrap_or_default();
                            let db = rb.per_deal_profits.clone().unwrap_or_default();
                            let diffs: Vec<f64> =
                                da.iter().zip(db.iter()).map(|(x, y)| x - y).collect();
                            let rng = &mut cham_core::rng::rng_from_seed(spec.seeds[0] ^ 0xAB);
                            let ci = paired_ci(&diffs, spec.conf, rng);
                            let delta = crate::stats::mean(&diffs);
                            let vr = (ra.vr_factor + rb.vr_factor) / 2.0;
                            Ok((
                                PerOppDelta {
                                    opponent: opp.id(),
                                    delta_mb: delta,
                                    ci,
                                    vr_factor: vr,
                                },
                                diffs,
                            ))
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|h| h.join().expect("ab thread"))
                    .collect()
            });

        // Reassemble in pool order.
        let mut per_opp = Vec::with_capacity(per_opp_results.len());
        let mut all_diffs: Vec<f64> = Vec::new();
        for r in per_opp_results {
            let (d, diffs) = r?;
            per_opp.push(d);
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
        let vr_factor = mean_vr(&per_opp);
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
                    "vr_factor": vr_factor,
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
            vr_factor,
        })
    }

    /// Parallel, per-opponent-isolated shared-hero A/B (v3 §1.1: replaces the
    /// old sequential `run_shared` body). Each opponent gets its OWN hero
    /// instances (session isolation, matching `ladder.rs::run_opponent`'s
    /// contract — tracker/router belief state never leaks across opponent
    /// identities) and its own thread; seeds derive exactly as in `ladder.rs`
    /// / the old `run` (factory) path, so results are deterministic
    /// regardless of thread scheduling. Chunked in 250-deal SPRT ranges via
    /// `MatchRunner::run_shared_range` (same early-stop shape as `ladder`).
    pub fn run_shared<FA, FB>(
        spec: &AbSpec,
        pool: &[cham_opponents::OpponentSpec],
        hero_factory_a: &FA,
        hero_factory_b: &FB,
        depth_bb: i64,
        rec: Option<&mut Recorder>,
    ) -> Result<AbVerdict, EvalError>
    where
        FA: Fn() -> Result<Box<dyn Agent>, String> + Sync,
        FB: Fn() -> Result<Box<dyn Agent>, String> + Sync,
    {
        /// Deals per shared-hero chunk (mirrors `ladder.rs::CHUNK_DEALS`).
        const CHUNK_DEALS: u64 = 250;
        let per_opp_results: Vec<Result<(PerOppDelta, Vec<f64>), EvalError>> =
            std::thread::scope(|s| {
                let handles: Vec<_> = pool
                    .iter()
                    .enumerate()
                    .map(|(i, opp)| {
                        s.spawn(move || -> Result<(PerOppDelta, Vec<f64>), EvalError> {
                            // Fresh, session-isolated heroes per opponent —
                            // same contract as ladder.rs::run_opponent.
                            let mut hero_a = hero_factory_a()
                                .map_err(|e| EvalError::Match(format!("arm a: {e}")))?;
                            let mut hero_b = hero_factory_b()
                                .map_err(|e| EvalError::Match(format!("arm b: {e}")))?;
                            let mk = |arm_seed: u64| MatchSpec {
                                opponent: OpponentSpecDto(opp.id()),
                                deals: spec.deals_per_opp,
                                depth_bb,
                                base_seed: arm_seed ^ ((i as u64) << 32),
                                label: format!("ab:{}/{}/{}", spec.a, spec.b, opp.id()),
                            };
                            // 250-deal chunked ranges: identical per-deal
                            // streams to the unchunked call (seeds derive
                            // from the global deal index), same as ladder.
                            let mut da_all: Vec<f64> = Vec::new();
                            let mut db_all: Vec<f64> = Vec::new();
                            let mut done = 0u64;
                            let mut vr_sum = 0.0;
                            let mut n_chunks = 0u64;
                            while done < spec.deals_per_opp {
                                let take = (spec.deals_per_opp - done).min(CHUNK_DEALS);
                                let range = done..done + take;
                                let ra = MatchRunner::run_shared_range(
                                    &mk(spec.seeds[0]),
                                    hero_a.as_mut(),
                                    None,
                                    range.clone(),
                                )?;
                                let rb = MatchRunner::run_shared_range(
                                    &mk(spec.seeds[0]),
                                    hero_b.as_mut(),
                                    None,
                                    range,
                                )?;
                                da_all.extend(ra.per_deal_profits.clone().unwrap_or_default());
                                db_all.extend(rb.per_deal_profits.clone().unwrap_or_default());
                                vr_sum += (ra.vr_factor + rb.vr_factor) / 2.0;
                                n_chunks += 1;
                                done += take;
                            }
                            // paired diffs per deal (identical opponent streams)
                            let diffs: Vec<f64> = da_all
                                .iter()
                                .zip(db_all.iter())
                                .map(|(x, y)| x - y)
                                .collect();
                            let rng = &mut cham_core::rng::rng_from_seed(spec.seeds[0] ^ 0xAB);
                            let ci = paired_ci(&diffs, spec.conf, rng);
                            let delta = crate::stats::mean(&diffs);
                            let vr = if n_chunks > 0 {
                                vr_sum / n_chunks as f64
                            } else {
                                1.0
                            };
                            Ok((
                                PerOppDelta {
                                    opponent: opp.id(),
                                    delta_mb: delta,
                                    ci,
                                    vr_factor: vr,
                                },
                                diffs,
                            ))
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|h| h.join().expect("ab thread"))
                    .collect()
            });

        // Reassemble in pool order.
        let mut per_opp = Vec::with_capacity(per_opp_results.len());
        let mut all_diffs: Vec<f64> = Vec::new();
        for r in per_opp_results {
            let (d, diffs) = r?;
            per_opp.push(d);
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
        let vr_factor = mean_vr(&per_opp);
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
                    "vr_factor": vr_factor,
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
            vr_factor,
        })
    }

    /// Holm correction over a pre-registered gate family (SPECS/10 §4).
    pub fn holm_family(pvals: &[f64], alpha: f64) -> Vec<bool> {
        holm(pvals, alpha)
    }
}

use cham_core::obs::Agent;

/// Mean variance-reduction factor over per-opponent deltas (v3 §2.1 step 1:
/// the "how much did VR already buy us" number on every gate printout).
fn mean_vr(per_opp: &[PerOppDelta]) -> f64 {
    if per_opp.is_empty() {
        return 1.0;
    }
    per_opp.iter().map(|d| d.vr_factor).sum::<f64>() / per_opp.len() as f64
}
