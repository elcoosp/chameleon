//! cham-agent (SPECS/07): composition — (encoder + router + experts + searcher +
//! tracker) → one `Agent`, in config-driven modes.
//!
//! v2 core properties:
//! - the tracker consumes `&PublicHistory` ONLY (leak-proof, invariant I9)
//! - router weights are FROZEN for the whole hand (per-hand field, not per decision)
//! - the behavioral mixture is reach-weighted: `σ_mix(a|i) ∝ Σ_k w_k·π_k(i)·σ_k(a|i)`
//! - confidence-gated fallback (visits) with weights untouched; total coverage loss
//!   → uniform + `fallback_uniform` record

#![forbid(unsafe_code)]

pub mod loader;
pub mod modes;
pub mod pipeline;
pub mod trace;
pub mod tracker;

pub use modes::AgentMode;
pub use pipeline::ChameleonAgent;
pub use tracker::Tracker;

use thiserror::Error;

/// Errors surfaced by the agent.
#[derive(Debug, Error)]
pub enum AgentError {
    #[error("loader: {0}")]
    Loader(String),
    #[error("pipeline: {0}")]
    Pipeline(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}
