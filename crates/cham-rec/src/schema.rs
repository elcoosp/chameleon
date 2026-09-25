//! The record-kind registry — the SINGLE source of truth (SPECS/12 §3).
//! Unknown kinds are a validator error; all producers must extend this table in the
//! same commit that adds a kind. No other file may list record kinds.

use serde::{Deserialize, Serialize};

use crate::RecError;

/// Every record kind the workspace may emit, with its required payload fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    /// cham-opponents via eval — spec_id, family ("A"|"B"|"PN"|"noise"), arch, seed, params
    OppSession,
    /// cham-eval — label, spec_ids, seeds, deals, seatings, mb_per_seating, se_mb, vr_factor, wall_s
    Match,
    /// cham-agent decision trace (SPECS/07 §5)
    Decision,
    /// cham-blueprint — iters, infosets, bytes, wall_s, thread_mode, threads
    BpSnapshot,
    /// cham-blueprint — lbr_mb, coverage, iters
    BpProbe,
    /// cham-blueprint — src_artifact, depth_bb, keys_transferred
    WarmstartStep,
    /// cham-router — rows, top1_b_dev, top1_b_test, ece_b_test, ece_family_c, per_class_recall, gates_passed
    RouterTrain,
    /// cham-search — triggered, solver, iters, truncated, lbr_gap (ours, theirs)
    SearchDecision,
    /// cham-agent loader — mode, artifact hashes (blake3 each), depth_bb, experts[5] provenance
    AgentLoad,
    /// cham-eval — rows, sessions, family_counts, abstraction_hash
    CollectRowset,
    /// cham-eval ledger — type (ab|ladder|slumbot|probe), a{}, b?, delta_mb?, ci?, sprt?, promote, ...
    LedgerEntry,
    /// cham-eval — lbr_mb, coverage, router acc/ece, verdict PASS/FAIL
    ProbeSummary,
    /// cham-agent — hand_idx, street, reason (uniform fallback on total coverage loss)
    FallbackUniform,
}

impl RecordKind {
    /// Parse from the registry string.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<RecordKind> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
    }

    /// The registry string.
    pub fn as_str(self) -> &'static str {
        match self {
            RecordKind::OppSession => "opp_session",
            RecordKind::Match => "match",
            RecordKind::Decision => "decision",
            RecordKind::BpSnapshot => "bp_snapshot",
            RecordKind::BpProbe => "bp_probe",
            RecordKind::WarmstartStep => "warmstart_step",
            RecordKind::RouterTrain => "router_train",
            RecordKind::SearchDecision => "search_decision",
            RecordKind::AgentLoad => "agent_load",
            RecordKind::CollectRowset => "collect_rowset",
            RecordKind::LedgerEntry => "ledger_entry",
            RecordKind::ProbeSummary => "probe_summary",
            RecordKind::FallbackUniform => "fallback_uniform",
        }
    }

    /// Required payload fields (SPECS/12 §3). `?`-marked optionals in the spec are
    /// absent here — only hard requirements are enforced.
    pub fn required_fields(self) -> &'static [&'static str] {
        match self {
            RecordKind::OppSession => &["spec_id", "family", "arch", "seed", "params"],
            RecordKind::Match => &[
                "label", "spec_ids", "seeds", "deals", "seatings", "mb_per_seating", "se_mb",
                "vr_factor", "wall_s",
            ],
            RecordKind::Decision => &[
                "hand_idx", "street", "slot", "action", "weights_frozen", "expert_visits",
                "fallback_used", "abstraction_hash",
            ],
            RecordKind::BpSnapshot => &["iters", "infosets", "bytes", "wall_s", "thread_mode", "threads"],
            RecordKind::BpProbe => &["lbr_mb", "coverage", "iters"],
            RecordKind::WarmstartStep => &["src_artifact", "depth_bb", "keys_transferred"],
            RecordKind::RouterTrain => &[
                "rows", "top1_b_dev", "top1_b_test", "ece_b_test", "ece_family_c",
                "per_class_recall", "gates_passed",
            ],
            RecordKind::SearchDecision => &["triggered", "solver", "iters", "truncated", "lbr_gap"],
            RecordKind::AgentLoad => &["mode", "artifact_hashes", "depth_bb", "experts"],
            RecordKind::CollectRowset => &["rows", "sessions", "family_counts", "abstraction_hash"],
            RecordKind::LedgerEntry => &["type", "promote"],
            RecordKind::ProbeSummary => &["lbr_mb", "coverage", "verdict"],
            RecordKind::FallbackUniform => &["hand_idx", "street", "reason"],
        }
    }
}

/// Reject non-finite payloads. serde_json silently maps non-finite floats to `null`,
/// so the validator additionally rejects the sentinel *strings* the workspace is
/// forbidden from using in place of them.
fn reject_non_finite(v: &serde_json::Value) -> Result<(), RecError> {
    match v {
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if !f.is_finite() {
                    return Err(RecError::NonFinite);
                }
            }
            Ok(())
        }
        serde_json::Value::String(s) => match s.as_str() {
            "NaN" | "inf" | "-inf" | "Infinity" | "-Infinity" => Err(RecError::NonFinite),
            _ => Ok(()),
        },
        serde_json::Value::Array(a) => a.iter().try_for_each(reject_non_finite),
        serde_json::Value::Object(o) => o.values().try_for_each(reject_non_finite),
        _ => Ok(()),
    }
}

/// Validate a payload against the registry: object shape, required fields, finiteness.
pub fn check_payload(kind: RecordKind, data: &serde_json::Value) -> Result<(), RecError> {
    let obj = data.as_object().ok_or_else(|| {
        RecError::Invalid(format!("payload for `{}` must be a JSON object", kind.as_str()))
    })?;
    for field in kind.required_fields() {
        if !obj.contains_key(*field) {
            return Err(RecError::MissingField {
                kind: kind.as_str().to_string(),
                field: (*field).to_string(),
            });
        }
    }
    reject_non_finite(data)
}
