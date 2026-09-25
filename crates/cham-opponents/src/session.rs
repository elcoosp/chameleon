//! Session parameters flight record (SPECS/03 §6): family + params draws, with
//! out-of-family sessions labeled `family: "B"|"PN"|"noise"` so eval/protocol can
//! never accidentally count them as in-family.

use serde::{Deserialize, Serialize};

use crate::factory::OpponentSpec;

/// One opponent session's parameters (recorded under kind `opp_session`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionParams {
    pub spec_id: String,
    pub family: String,
    pub arch: String,
    pub seed: u64,
    pub params: serde_json::Value,
}

impl SessionParams {
    /// Build from a spec + the (jittered) params actually drawn.
    pub fn from_spec(
        spec: &OpponentSpec,
        seed: u64,
        drawn_params: serde_json::Value,
    ) -> SessionParams {
        let arch = match spec {
            OpponentSpec::Arch(a) | OpponentSpec::Jitter(a, _) | OpponentSpec::FamilyB(a) => {
                a.as_str().to_string()
            }
            OpponentSpec::Perturbed { tilt, .. } => tilt.as_str().to_string(),
            OpponentSpec::Noisy { inner, .. } => inner.id(),
            OpponentSpec::Switcher { a, b, .. } => format!("{}->{}", a.id(), b.id()),
            _ => spec.id(),
        };
        SessionParams {
            spec_id: spec.id(),
            family: spec.family().to_string(),
            arch,
            seed,
            params: drawn_params,
        }
    }

    /// JSON object for the `opp_session` record payload (all required fields set).
    pub fn to_record(&self) -> serde_json::Value {
        serde_json::json!({
            "spec_id": self.spec_id,
            "family": self.family,
            "arch": self.arch,
            "seed": self.seed,
            "params": self.params,
        })
    }
}
