//! cham-search (SPECS/06): inference-time river solving.
//!
//! v2 solver family (replaces v1's unsound anchored CFR+):
//! - **FMBR** — fixed-model best response: maximum exploitation vs the prior
//! - **RNR(p)** — restricted Nash response: principled interpolation with a
//!   safety knob (villain plays the prior with prob p, freely with 1−p)
//! - **ReachGadget** — the conservative arm (villain clamped toward robust play)
//!
//! Scope decision D-012: the subgame collapses each player's river range into
//! weighted STRENGTH CLASSES (deterministic ordering decides showdowns), which
//! makes every solver exactly verifiable against the enumerative-LP oracle.
//! The combo-level expansion (≤ 64×128 leaf paths) is the M4 stretch.

#![forbid(unsafe_code)]
#![warn(clippy::significant_drop_in_scrutinee)]

pub mod budget;
pub mod cache;
pub mod cache_persist;
pub mod fullgame;
pub mod kernel;
pub mod oracle;
pub mod prior;
pub mod pubtree;
pub mod river_cfr;
pub mod solve;
pub mod subgame;
pub mod trigger;
pub mod vbr;

pub use budget::{SearchBudget, WallClockGuard};
pub use prior::PriorStrats;
pub use solve::SolveResult;
pub use subgame::Subgame;
pub use trigger::SolverChoice;
pub use trigger::{SearchConfig, should_search};

use thiserror::Error;

/// Errors surfaced by the search crate.
#[derive(Debug, Error)]
pub enum SearchError {
    #[error("subgame: {0}")]
    Subgame(String),
    #[error("solver: {0}")]
    Solver(String),
    #[error("budget exhausted")]
    BudgetExhausted,
}
