//! The 20-dim feature contract (SPECS/05 §2): hand-frozen tracker statistics.
//!
//! Computed ONCE PER HAND at hand start from the tracker state (which updates only
//! at hand ends — no within-hand leakage). One row per hand; splits and CIs are
//! session-clustered. NO blueprint/policy inputs exist anywhere on this path
//! (DAG + anti-circularity — structural test `features_no_blueprint_inputs`).

use serde::{Deserialize, Serialize};

use cham_engine::features::RouterFeatures;

use crate::RouterError;

/// The ordered semantic contract over the 20 dims (SPECS/05 §2 table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureContract;

impl FeatureContract {
    pub const DIMS: usize = 20;
    pub const MATURITY: usize = 0;
    pub const EWM_STATS: std::ops::Range<usize> = 1..14; // 13 dims
    pub const OPPORTUNITY: std::ops::Range<usize> = 14..18; // 4 dims
    pub const TREND_Z: usize = 18;
    pub const HANDS_SINCE_SHOWDOWN: usize = 19;
}

/// The tracker-derived inputs (produced by cham-agent's Tracker; this crate only
/// owns the shrink/normalize math).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct FeatureInputs {
    pub hands_seen: u64,
    /// 13 EWM stats (half-life 60), already shrunk toward 0.5 by maturity
    pub ewm: [f64; 13],
    /// 4 log-scaled opportunity counts in [0,1]
    pub opportunity: [f64; 4],
    /// session EV trend z (clamped ±3, then /3)
    pub trend_z: f64,
    /// hands since last showdown, log-scaled /log(50)
    pub hands_since_showdown: f64,
}

/// EWM maturity shrink factor: min(1, hands/150) (SPECS/07 §2).
pub fn maturity_shrink(hands_seen: u64) -> f64 {
    (hands_seen as f64 / 150.0).min(1.0)
}

/// Shrink a raw stat toward 0.5 by the maturity factor.
pub fn shrink(raw: f64, hands_seen: u64) -> f64 {
    let m = maturity_shrink(hands_seen);
    0.5 * (1.0 - m) + raw * m
}

/// Log-scaled maturity feature: log10(n+1)/3.5 ∈ [0,1].
pub fn maturity_feature(hands_seen: u64) -> f64 {
    ((hands_seen as f64 + 1.0).log10() / 3.5).min(1.0)
}

/// Build the 20-dim feature vector from tracker inputs (pure function).
pub fn from_inputs(inputs: &FeatureInputs) -> Result<RouterFeatures, RouterError> {
    let mut f = [0f32; FeatureContract::DIMS];
    f[FeatureContract::MATURITY] = maturity_feature(inputs.hands_seen) as f32;
    for (i, v) in inputs.ewm.iter().enumerate() {
        if !v.is_finite() {
            return Err(RouterError::Features(format!("ewm[{i}] not finite")));
        }
        f[FeatureContract::EWM_STATS.start + i] = v.clamp(0.0, 1.0) as f32;
    }
    for (i, v) in inputs.opportunity.iter().enumerate() {
        if !v.is_finite() {
            return Err(RouterError::Features(format!(
                "opportunity[{i}] not finite"
            )));
        }
        f[FeatureContract::OPPORTUNITY.start + i] = v.clamp(0.0, 1.0) as f32;
    }
    f[FeatureContract::TREND_Z] = inputs.trend_z.clamp(-1.0, 1.0) as f32;
    f[FeatureContract::HANDS_SINCE_SHOWDOWN] = inputs.hands_since_showdown.clamp(0.0, 1.0) as f32;
    let rf = RouterFeatures(f);
    rf.validate()
        .map_err(|e| RouterError::Features(format!("{e}")))?;
    Ok(rf)
}
