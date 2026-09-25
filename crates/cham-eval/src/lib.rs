//! cham-eval (SPECS/08): numbers with defensible intervals.
//!
//! - duplicate-deck matching: profit(d) = (netA + netB)/2 — the SUM cancels seat
//!   advantage (v1's "(net1 − net2)/2" was garbled)
//! - session-clustered CIs, SPRT early stopping, Holm correction
//! - all-in-EV variance reduction + AIVAT-style known-opponent baseline
//! - the real Slumbot dialect with a verify-first gate; Glicko ELO is CUT

#![forbid(unsafe_code)]

pub mod ab;
pub mod dashboard;
pub mod ingest;
pub mod ledger;
pub mod matcheng;
pub mod slumbot;
pub mod stats;
pub mod vr;

pub use ab::{AbRunner, AbSpec, AbVerdict};
pub use ledger::Ledger;
pub use matcheng::{MatchRunner, MatchSpec, MatchResult, PoolResult};
pub use stats::{bootstrap_ci, holm, mean, paired_ci, required_seatings, session_cluster_ci, se, sprrt, welch_t, SprtState};
pub use vr::{apply_allin_ev, variance_factor};

use thiserror::Error;

/// Errors surfaced by the eval crate.
#[derive(Debug, Error)]
pub enum EvalError {
    #[error("match: {0}")]
    Match(String),
    #[error("stats: {0}")]
    Stats(String),
    #[error("ledger: {0}")]
    Ledger(String),
    #[error("slumbot: {0}")]
    Slumbot(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}
