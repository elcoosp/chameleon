//! Epsilons and the invariant registry (SPECS/00 §5, SPECS/01 §7).
//!
//! `unreachable!("cham-xxx: invariant I<n>")` sites must reference these IDs.

/// Comparison epsilon for probabilities.
pub const EPS_PROB: f64 = 1e-9;
/// Comparison epsilon for EVs in bb.
pub const EPS_EV: f64 = 1e-9;
/// Comparison epsilon for equity ∈ [0,1].
pub const EPS_EQUITY: f64 = 1e-9;
/// Gap under which two bet sizes are considered equal (chips).
pub const EPS_CHIPS: i64 = 0; // chips are integers; no epsilon needed on money

/// Registry of cross-crate invariants. Docs only — enforced by tests.
pub mod invariants {
    /// I1 — every card byte is < 52 in any reachable state.
    pub const I1_CARD_DOMAIN: &str = "I1 card domain 0..=51";
    /// I2 — legal action list is non-empty until terminal.
    pub const I2_LEGAL_NONEMPTY: &str = "I2 legal actions non-empty until terminal";
    /// I3 — chip conservation: sum(stacks) + pot == 2 * start_stack.
    pub const I3_CHIP_CONSERVATION: &str = "I3 chip conservation";
    /// I4 — stacks never negative; bets bounded by stacks.
    pub const I4_STACKS_NONNEG: &str = "I4 stacks/bounds";
    /// I5 — payoffs are zero-sum at terminal.
    pub const I5_ZERO_SUM: &str = "I5 zero-sum payoffs";
    /// I6 — board cards unique; exactly 5 dealt by the river.
    pub const I6_BOARD_UNIQUE: &str = "I6 board uniqueness";
    /// I7 — min-raise progression: a raise-to ≥ previous to + last full raise size.
    pub const I7_MIN_RAISE: &str = "I7 min-raise progression";
    /// I8 — encoder key's legal mask popcount == row width == n_slots (cham-engine).
    pub const I8_MASK_WIDTH: &str = "I8 legal-mask/key-width agreement";
    /// I9 — PublicHistory contains no hidden card (folded holes stay hidden).
    pub const I9_NO_HIDDEN: &str = "I9 public-history secrecy";
}

pub use invariants::*;
