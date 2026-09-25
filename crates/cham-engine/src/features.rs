//! RouterFeatures — the 20-dim, hand-frozen router feature CONTRACT
//! (SPECS/02 §5 + SPECS/05 §2). cham-engine defines the type and the
//! serialization/validity contract only; cham-router owns the ordered semantic
//! list; cham-agent builds values from its tracker. No blueprint/policy inputs
//! exist on this path (DAG + anti-circularity, review A6).
//!
//! Ordered dims (see SPECS/05 §2 for ranges):
//! 0 maturity | 1..=13 EWM opponent stats | 14..=17 opportunity counts |
//! 18 session EV trend z | 19 hands-since-showdown.

use serde::{Deserialize, Serialize};

use crate::EngineError;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouterFeatures(pub [f32; 20]);

pub const N_FEATURES: usize = 20;

impl Default for RouterFeatures {
    fn default() -> Self {
        RouterFeatures([0.0; N_FEATURES])
    }
}

impl RouterFeatures {
    /// Contract check: all dims finite and within the documented [-1.2, 1.2] envelope
    /// (documented per-dim ranges live in SPECS/05 §2; all fall inside this bound).
    pub fn validate(&self) -> Result<(), EngineError> {
        for (i, v) in self.0.iter().enumerate() {
            if !v.is_finite() {
                return Err(EngineError::Config(format!("router feature {i} not finite")));
            }
            if !(-1.2..=1.2).contains(v) {
                return Err(EngineError::Config(format!("router feature {i} out of range: {v}")));
            }
        }
        Ok(())
    }
}
