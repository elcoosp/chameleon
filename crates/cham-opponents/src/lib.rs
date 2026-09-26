//! cham-opponents (SPECS/03): all opponents implement `cham_core::Agent`.
//!
//! v2 core properties:
//! - Scripts are **probability oracles**: exact/equity-proxy thresholds with
//!   **fresh independent draws per decision** — `action_probs(obs)` is analytic
//!   and per-decision independent (required by one-sided CFR reach products).
//! - The out-of-family evaluation trio: PerturbedNash, FamilyB, Noisy.

#![forbid(unsafe_code)]

pub mod archetype;
pub mod baselines;
pub mod drift;
pub mod factory;
pub mod family_b;
pub mod frozen;
pub mod noisy;
pub mod params;
pub mod percentile;
pub mod perturbed;
pub mod session;

pub use archetype::ArchetypeAgent;
pub use factory::OpponentSpec;
pub use frozen::{FrozenAgent, FrozenRows};
pub use params::ArchetypeId;
pub use params::{ArchetypeParams, JitterSpec};
pub use percentile::PercentileChart;

use thiserror::Error;

/// Errors surfaced by the opponents crate.
#[derive(Debug, Error)]
pub enum OpponentsError {
    #[error("unknown opponent id: {0}")]
    UnknownId(String),
    #[error("factory: {0}")]
    Factory(String),
    #[error("params: {0}")]
    Params(String),
}
