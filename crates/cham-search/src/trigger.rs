//! When to search (SPECS/06 §2).

use cham_core::engine::Street;
use cham_core::obs::Observables;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SolverChoice {
    Fmbr,
    Rnr { p: f64 },
    ReachGadget,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchConfig {
    pub enabled: bool,
    pub solver: SolverChoice,
    pub budget: crate::budget::SearchBudget,
    /// minimum pot (bb) for the trigger to fire
    pub min_pot_bb: f64,
    /// river-only in v1 (turn search is the v2 stretch)
    pub river_only: bool,
}

impl Default for SearchConfig {
    fn default() -> Self {
        SearchConfig {
            enabled: false, // search stays OFF until G4 (SPECS/06 §7 lockout)
            solver: SolverChoice::Rnr { p: 0.9 },
            budget: crate::budget::SearchBudget::Iterations { iters: 400 },
            min_pot_bb: 8.0,
            river_only: true,
        }
    }
}

/// Trigger: enabled ∧ river-only satisfied ∧ pot above the floor.
pub fn should_search(obs: &Observables<'_>, cfg: &SearchConfig) -> bool {
    if !cfg.enabled {
        return false;
    }
    if cfg.river_only && obs.street != Street::River {
        return false;
    }
    obs.pot_bb() >= cfg.min_pot_bb
}
