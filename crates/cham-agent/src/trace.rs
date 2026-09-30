//! Decision traces (SPECS/07 §5): the only persistence of weights — no side
//! channels. Emitted through cham-rec (kind `decision`).

use cham_rec::Recorder;
use cham_rec::schema::RecordKind;

use crate::AgentError;

#[derive(Clone, Debug)]
pub struct DecisionTrace {
    pub hand_idx: u64,
    pub street: u8,
    pub slot: usize,
    pub action: String,
    pub weights_frozen: [f64; 5],
    pub argmax_k: Option<usize>,
    pub search: Option<(String, bool, String, u32, bool, f64)>, // solver, triggered, source, iters, truncated, lbr
    pub expert_visits: [u32; 4],
    pub fallback_used: bool,
    pub abstraction_hash: u64,
    /// Diagnostic (P1): expert k's `strategy()` returned `None` at this
    /// decision. Distinct from `fallback_used`, which also fires on robust
    /// miss, reach-mass zero, and mix-zero paths.
    pub expert_missed: [bool; 4],
    /// Diagnostic (P1): robust's `strategy()` returned `None`.
    pub robust_missed: bool,
    /// Diagnostic (P1): Σ w_k·π_k hit zero → plain weighted-average fallback.
    pub reach_mass_zero: bool,
    /// Diagnostic (P1): mix total hit zero → uniform fallback.
    pub mix_zero: bool,
    /// Diagnostic (2026-09-30): the argmax path picked expert k, k's
    /// strategy missed, and robust_sigma covered the decision. The
    /// ACTION comes from robust, but `fallback_used` is reset to false
    /// on this path (see `pipeline.rs::act_impl` R3 policy). See
    /// `ARGMAX-FALLBACK-REALLY-MATTERS-2026-09-30.md`.
    pub expert_missed_robust_covered: bool,
}

pub fn record(rec: Option<&mut Recorder>, run: &str, t: &DecisionTrace) -> Result<(), AgentError> {
    if let Some(rec) = rec {
        let mut data = serde_json::json!({
            "hand_idx": t.hand_idx,
            "street": t.street,
            "slot": t.slot,
            "action": t.action,
            "weights_frozen": t.weights_frozen,
            "expert_visits": t.expert_visits,
            "fallback_used": t.fallback_used,
            "abstraction_hash": format!("{:x}", t.abstraction_hash),
            "expert_missed": t.expert_missed,
            "robust_missed": t.robust_missed,
            "reach_mass_zero": t.reach_mass_zero,
            "mix_zero": t.mix_zero,
        });
        if let Some(k) = t.argmax_k {
            data["argmax_k"] = serde_json::json!(k);
        }
        if let Some((solver, triggered, source, iters, truncated, lbr)) = &t.search {
            data["search"] = serde_json::json!({
                "solver": solver, "triggered": triggered, "source": source,
                "iters": iters, "truncated": truncated, "lbr_gap_ours": lbr,
            });
        }
        let _ = run;
        rec.record(RecordKind::Decision, data)
            .map_err(|e| AgentError::Pipeline(format!("{e}")))?;
    }
    Ok(())
}
