//! cham-core (SPECS/01): cards, fast hand evaluation, the HUNL rules engine,
//! RNG discipline, observables and the `Agent` trait with leak-proof public
//! histories. Zero policy logic; zero dependency on `cham-rec`.
//!
//! Invariants live in [`consts`]; hot paths are allocation-free (`State` is `Copy`).

#![forbid(unsafe_code)]

pub mod card;
pub mod consts;
pub mod engine;
pub mod eval;
pub mod obs;
pub mod rng;

pub use card::{Card, Deck, Hand2};
pub use engine::config::EngineConfig;
pub use engine::history::{HandHistory, PublicHistory};
pub use engine::{Action, ApplyOutcome, State, Street};
pub use obs::{Agent, AgentError, LegalAction, Observables, Player};
pub use rng::{Rng, child, rng_from_seed};

use thiserror::Error;

/// Errors surfaced by the foundation crate.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid card spec: {0}")]
    InvalidCard(String),
    #[error("invalid engine config: {0}")]
    InvalidConfig(String),
    #[error("illegal action {action:?} in state: {reason}")]
    IllegalAction { action: Action, reason: String },
    #[error("engine invariant violated: {0}")]
    Invariant(&'static str),
    #[error("deck exhausted (dealt {pos} of 52)")]
    DeckExhausted { pos: u8 },
    #[error("replay failed: {0}")]
    Replay(String),
}
