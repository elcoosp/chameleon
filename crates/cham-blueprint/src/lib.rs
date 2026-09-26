//! cham-blueprint (SPECS/04): the trainer. Correctness rewrite of the v1 estimator:
//!
//! - ES-MCCFR: chance and opponent actions SAMPLED, hero actions ENUMERATED —
//!   no reach multipliers, no importance weights, no baselines (review A3)
//! - one regret row per infoset (the key carries the position bit)
//! - regret-based pruning (Pluribus trick), delayed linear averaging in both modes
//! - Deterministic (bit-identical) vs Hogwild (atomic CAS-adds) threading
//! - quantized, strategy-only inference artifacts with visit-based confidence

#![forbid(unsafe_code)]

pub mod lbr;
pub mod modes;
pub mod policy;
pub mod table;
pub mod train_cache;
pub mod trainer;
pub mod traversal;
pub mod warmstart;

pub use modes::{BeliefBins, TrainMode};
pub use policy::{BlueprintPolicy, ProvenanceRecord};
pub use table::RegretTable;
pub use table::{DeltaBuffer, ThreadMode};
pub use trainer::{
    RunProvenance, TrainerConfig, averaging_weight, default_threads, train, train_with_threads,
};

use thiserror::Error;

/// Errors surfaced by the trainer.
#[derive(Debug, Error)]
pub enum BlueprintError {
    #[error("table: {0}")]
    Table(String),
    #[error("artifact {path}: {reason}")]
    Artifact {
        path: std::path::PathBuf,
        reason: String,
    },
    #[error("provenance: {0}")]
    Provenance(String),
    #[error("abstraction hash mismatch: expected {expected:#x}, found {found:#x}")]
    HashMismatch { expected: u64, found: u64 },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("postcard: {0}")]
    Postcard(String),
    #[error("training: {0}")]
    Training(String),
}
