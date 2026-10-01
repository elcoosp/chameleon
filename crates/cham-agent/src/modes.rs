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
            "mixture" | "argmax" | "sample-expert" | "hedged" | "robust-only" | "bayes" => {}
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
        // L-19 relaxation (2026-10-01): the pipeline now actually calls
        // `cham_search::solve` when `search.enabled` is true (F1 fix,
        // `crates/cham-agent/src/search_bridge.rs`). The pre-F1 refusal
        // ("not yet wired, refusing rather than silently ignoring") is no
        // longer accurate; the remaining G4 lockout above still requires a
        // non-empty `g4_ledger_ref` as the auditable opt-in token
        // (SPECS/06 §7).
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every AgentMode constructor must produce a mode whose `validate()`
    /// returns Ok. This is the regression test for the 2026-09-30 hedged
    /// bug: `AgentMode::hedged()` existed in `pipeline.rs` as a routing
    /// string, but `validate()` didn't recognize it — the probe failed
    /// with "unknown routing: hedged" while the ladder (which doesn't
    /// call validate()) silently worked.
    #[test]
    fn all_constructors_validate() {
        AgentMode::full_search_off()
            .validate()
            .expect("full_search_off");
        AgentMode::argmax().validate().expect("argmax");
        AgentMode::hedged().validate().expect("hedged");
        AgentMode::robust_only().validate().expect("robust_only");
    }

    /// Every routing string that `hero.rs::routing_for` can return must
    /// round-trip through validate(). The list here mirrors the CLI-visible
    /// set in crates/cham-cli/src/cmd/hero.rs.
    ///
    /// If a new routing mode is added to hero.rs without being added here,
    /// this test will not catch it (different crate), but if it's added
    /// here without being added to validate(), the test WILL catch it.
    #[test]
    fn every_routing_string_validates() {
        for s in ["mixture", "argmax", "hedged", "robust-only", "bayes"] {
            let mode = AgentMode {
                routing: s.into(),
                search: SearchCfg {
                    enabled: false,
                    solver: "Rnr".into(),
                    g4_ledger_ref: String::new(),
                },
                fallback_mode: "renorm".into(),
            };
            mode.validate().unwrap_or_else(|e| {
                panic!("routing {s:?} failed to validate: {e}");
            });
        }
    }

    /// Unknown routing strings must fail validation, not be accepted silently.
    #[test]
    fn unknown_routing_rejected() {
        let mode = AgentMode {
            routing: "definitely-not-a-mode".into(),
            search: SearchCfg {
                enabled: false,
                solver: "Rnr".into(),
                g4_ledger_ref: String::new(),
            },
            fallback_mode: "renorm".into(),
        };
        assert!(mode.validate().is_err(), "unknown routing must be rejected");
    }

    /// F1 (2026-10-01): the L-19 lockout now only refuses an *empty*
    /// `g4_ledger_ref`. The former "not yet wired" refusal is gone because
    /// the pipeline DOES call `cham_search::solve` (see
    /// `crates/cham-agent/src/search_bridge.rs`). The G4 contract
    /// (SPECS/06 §7) remains: an enabled search must carry an auditable
    /// ledger token.
    #[test]
    fn search_enabled_without_g4_ref_rejected() {
        let mode = AgentMode {
            routing: "mixture".into(),
            search: SearchCfg {
                enabled: true,
                solver: "Rnr".into(),
                g4_ledger_ref: String::new(),
            },
            fallback_mode: "renorm".into(),
        };
        assert!(
            mode.validate().is_err(),
            "search_enabled with no g4_ledger_ref must be rejected (G4 lockout)"
        );
    }

    /// F1 (2026-10-01): with a non-empty `g4_ledger_ref`, enabled search
    /// is now accepted — the pipeline is wired.
    #[test]
    fn search_enabled_with_g4_ref_accepted() {
        let mode = AgentMode {
            routing: "mixture".into(),
            search: SearchCfg {
                enabled: true,
                solver: "Rnr".into(),
                g4_ledger_ref: "EXP-SEARCH".into(),
            },
            fallback_mode: "renorm".into(),
        };
        mode.validate()
            .expect("F1: enabled search + g4_ledger_ref must validate");
    }
}
