//! Phase C: public-chance-sampling DCFR trainer.
//! Design: docs/plans/PHASE-C-PCS-DESIGN-2026-10-08.md.
//!
//! The trainer samples a 5-card board, walks the shared `PublicTree`
//! once with that board fixed, and accumulates regret/strategy at
//! every infoset. Hero and villain hole-card ranges are held as
//! explicit vectors (never sampled), so card removal is exact and the
//! existing kernels (`showdown_cfv_two`, `fold_cfv`) apply unchanged.

pub mod dcfr;
pub mod sampling;
pub mod table;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PcsError {
    #[error("config: {0}")]
    Config(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
