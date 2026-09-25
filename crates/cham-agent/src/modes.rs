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
}

impl AgentMode {
    pub fn validate(&self) -> Result<(), crate::AgentError> {
        match self.routing.as_str() {
            "mixture" | "argmax" | "robust-only" | "bayes" => {}
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
        }
    }
}
