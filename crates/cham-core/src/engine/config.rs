//! Engine config (SPECS/01 §5): validated eagerly, no `#[serde(default)]`.

use serde::{Deserialize, Serialize};

use crate::CoreError;

/// `EngineConfig { start_stack, sb, bb }` — 1 bb = 100 chips, SB = 50.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineConfig {
    /// Starting stack in chips (10_000 = 100 bb at bb = 100).
    pub start_stack: i64,
    /// Small blind in chips.
    pub sb: i64,
    /// Big blind in chips.
    pub bb: i64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig::depth(100)
    }
}

impl EngineConfig {
    /// Standard development depth: 100 bb (SPECS/00 §4).
    pub fn depth(depth_bb: i64) -> EngineConfig {
        EngineConfig {
            start_stack: depth_bb * 100,
            sb: 50,
            bb: 100,
        }
    }
    pub fn depth_bb(&self) -> i64 {
        self.start_stack / self.bb
    }
    /// Eager validation (SPECS/00 §5): `sb*2 == bb`, `20bb ≤ start_stack ≤ 1000bb`.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.sb <= 0 || self.bb <= 0 || self.start_stack <= 0 {
            return Err(CoreError::InvalidConfig(
                "blinds and stack must be positive".into(),
            ));
        }
        if self.sb * 2 != self.bb {
            return Err(CoreError::InvalidConfig(format!(
                "sb*2 must equal bb (got sb={}, bb={})",
                self.sb, self.bb
            )));
        }
        if self.start_stack < 20 * self.bb || self.start_stack > 1000 * self.bb {
            return Err(CoreError::InvalidConfig(format!(
                "start_stack {} out of [20bb, 1000bb] at bb={}",
                self.start_stack, self.bb
            )));
        }
        Ok(())
    }
}
