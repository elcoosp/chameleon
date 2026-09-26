//! cham-engine (SPECS/02): the abstraction layer. **Infoset keys are pure functions
//! of (hole, board, geometry)** — zero Monte Carlo at encode time, zero in-key noise.
//!
//! - flop/turn buckets: Waugh-style suit-isomorphic (hand, board) orbit tables
//!   (offline-built, mmap'd, binary-search lookup)
//! - river buckets: exact equity quantile bins × board texture (no table)
//! - SPR bands on every street; legal mask in the key (invariant I8)
//! - the action ladder with pseudo-harmonic off-tree weights

#![forbid(unsafe_code)]

pub mod audit;
pub mod build;
pub mod canon;
pub mod config;
pub mod encoder;
pub mod features;
pub mod ladder;
pub mod tables;

pub use config::{AbstractionConfig, SprBandError};
pub use encoder::{ActionClass, ActionSeq, Encoder, InfoSetKey, SeqEntry};
pub use features::RouterFeatures;
pub use ladder::{AbstractAction, ActionLadder};

use thiserror::Error;

/// Errors surfaced by the abstraction layer.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("config: {0}")]
    Config(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("bucket artifact {path}: {reason}")]
    Artifact {
        path: std::path::PathBuf,
        reason: String,
    },
    #[error("meta.json: {0}")]
    Meta(String),
    #[error("abstraction hash mismatch: expected {expected:#x}, found {found:#x}")]
    HashMismatch { expected: u64, found: u64 },
    #[error("invariant violated: {0}")]
    Invariant(&'static str),
    #[error("mmap: {0}")]
    Mmap(String),
}
