//! Agent modes (SPECS/07 §3): config-driven — two modes differ only in knobs,
//! never code paths. `soft-buckets-on` is CUT (feature removed).

use serde::{Deserialize, Serialize};

/// Search sub-configuration (ledger-locked: `full` with search on requires a
/// non-empty `g4_ledger_ref` — SPECS/06 §7 lockout).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchCfg {
    pub enabled: bool,
    pub solver: String, // "Fmbr" | "Rnr" | "ReachGadget"
    pub g4_ledger_ref: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentMode {
    pub routing: String, // "mixture" | "argmax" | "robust-only" | "bayes"
    pub search: SearchCfg,
    /// EXP-013: mixture composition on strategy miss.
    /// "renorm" (default, R2: drop missed tier + renormalize, fallback only
    /// on genuinely-empty mixture) vs "substitute" (legacy: missed expert →
    /// robust σ, missed robust → uniform). Deserialization default keeps
    /// existing configs/TOMLs working.
    #[serde(default = "default_fallback_mode")]
    pub fallback_mode: String,
}

fn default_fallback_mode() -> String {
    "renorm".into()
}

impl AgentMode {
    pub fn validate(&self) -> Result<(), crate::AgentError> {
        match self.routing.as_str() {
            "mixture" | "argmax" | "hedged" | "robust-only" | "bayes" => {}
            other => {
                return Err(crate::AgentError::Loader(format!(
                    "unknown routing: {other}"
                )));
            }
        }
        if self.search.enabled && self.search.g4_ledger_ref.is_empty() {
            return Err(crate::AgentError::Loader(
                "search_enabled = true requires a non-empty g4_ledger_ref (G4 lockout, SPECS/06 §7)"
                    .into(),
            ));
        }
        // L-19 fix (2026-09-27): `pipeline.rs` has no `RiverSearcher` field;
        // the only writer of the decision-trace `search` slot is the hardcoded
        // `search: None`. So `search_enabled = true` in a config loads,
        // validates, and then yields NO search at every decision — the
        // "silent fallback" symptom, on the very feature that is supposed to
        // be opt-in. Until the searcher is wired into the pipeline, refuse
        // loudly at load time instead of accepting-and-ignoring.
        if self.search.enabled {
            return Err(crate::AgentError::Loader(
                "search_enabled = true is not yet wired into the runtime \
                 (pipeline hardcodes `search: None`); refusing rather than \
                 silently playing the fallback strategy. Set \
                 search_enabled = false, or complete the RiverSearcher wiring \
                 (cham-agent/src/pipeline.rs)."
                    .into(),
            ));
        }
        Ok(())
    }

    pub fn full_search_off() -> AgentMode {
        AgentMode {
            routing: "mixture".into(),
            search: SearchCfg {
                enabled: false,
                solver: "Rnr".into(),
                g4_ledger_ref: String::new(),
            },
            fallback_mode: "renorm".into(),
        }
    }
    pub fn argmax() -> AgentMode {
        AgentMode {
            routing: "argmax".into(),
            search: SearchCfg {
                enabled: false,
                solver: "Rnr".into(),
                g4_ledger_ref: String::new(),
            },
            fallback_mode: "renorm".into(),
        }
    }
    /// Hedged routing (2026-09-29): argmax when the top weight exceeds
    /// `CHAM_HEDGE_THRESHOLD`, mixture otherwise. See `pipeline.rs`
    /// act_impl's `"hedged"` branch.
    pub fn hedged() -> AgentMode {
        AgentMode {
            routing: "hedged".into(),
            search: SearchCfg {
                enabled: false,
                solver: "Rnr".into(),
                g4_ledger_ref: String::new(),
            },
            fallback_mode: "renorm".into(),
        }
    }

    pub fn robust_only() -> AgentMode {
        AgentMode {
            routing: "robust-only".into(),
            search: SearchCfg {
                enabled: false,
                solver: "Rnr".into(),
                g4_ledger_ref: String::new(),
            },
            fallback_mode: "renorm".into(),
        }
    }
}
