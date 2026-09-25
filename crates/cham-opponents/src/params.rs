//! Archetype parameters, jitter ranges and defaults (SPECS/03 §3).
//!
//! The decision procedure lives in `archetype.rs`; this file is the single source
//! of parameter truth. All thresholds are probabilities or equity multipliers.

use serde::{Deserialize, Serialize};

use crate::OpponentsError;

/// The four in-family archetypes (SPECS/10 §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArchetypeId {
    Nit,
    Tag,
    Lag,
    Station,
}

impl ArchetypeId {
    pub const ALL: [ArchetypeId; 4] = [
        ArchetypeId::Nit,
        ArchetypeId::Tag,
        ArchetypeId::Lag,
        ArchetypeId::Station,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            ArchetypeId::Nit => "nit",
            ArchetypeId::Tag => "tag",
            ArchetypeId::Lag => "lag",
            ArchetypeId::Station => "station",
        }
    }
    pub fn parse(s: &str) -> Option<ArchetypeId> {
        match s {
            "nit" => Some(ArchetypeId::Nit),
            "tag" => Some(ArchetypeId::Tag),
            "lag" => Some(ArchetypeId::Lag),
            "station" => Some(ArchetypeId::Station),
            _ => None,
        }
    }
}

/// One archetype's point parameters.
///
/// Threshold semantics: preflop gates are HAND FRACTIONS (cutoffs on the chart
/// percentile, 0 = strongest hand): `pct < open_raise` opens. Postflop gates are
/// EHS-proxy thresholds; mixing params are probabilities.
/// - `open_raise`: SB opens hands with pct below this cutoff (nit 0.12 = top 12%)
/// - `complete`: SB completes (limp-calls) down to this cutoff
/// - `call_open`: BB defends an open by calling below this cutoff
/// - `three_bet` / `call_3bet` / `four_bet`: cutoffs vs raises
/// - `iso_check`: BB iso-raises limps below this cutoff
/// - `cbet_flop` / `barrel_turn` / `barrel_river`: EHS thresholds to keep firing
/// - `donk` / `check_raise` / `bluff_river`: EHS gates for those lines
/// - `call_factor`: EHS must exceed pot_odds × call_factor to call bets
/// - `value_bet`: EHS above which we size value
/// - `size_idx`: 0 = 33% pot, 1 = 66%, 2 = 100%
/// - `trap`: probability of slow-playing very strong hands (checks instead of bets)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArchetypeParams {
    pub open_raise: f64,
    pub complete: f64,
    pub call_open: f64,
    pub three_bet: f64,
    pub call_3bet: f64,
    pub four_bet: f64,
    pub iso_check: f64,
    pub cbet_flop: f64,
    pub barrel_turn: f64,
    pub barrel_river: f64,
    pub donk: f64,
    pub check_raise: f64,
    pub bluff_river: f64,
    pub call_factor: f64,
    pub value_bet: f64,
    pub size_idx: u8,
    pub trap: f64,
}

impl ArchetypeParams {
    pub fn validate(&self) -> Result<(), OpponentsError> {
        let mut checks: Vec<(&str, f64)> = vec![
            ("open_raise", self.open_raise),
            ("complete", self.complete),
            ("call_open", self.call_open),
            ("three_bet", self.three_bet),
            ("call_3bet", self.call_3bet),
            ("four_bet", self.four_bet),
            ("iso_check", self.iso_check),
            ("cbet_flop", self.cbet_flop),
            ("barrel_turn", self.barrel_turn),
            ("barrel_river", self.barrel_river),
            ("donk", self.donk),
            ("check_raise", self.check_raise),
            ("bluff_river", self.bluff_river),
            ("value_bet", self.value_bet),
            ("trap", self.trap),
        ];
        checks.push(("call_factor", self.call_factor));
        for (k, v) in checks {
            if !v.is_finite() || v < 0.0 {
                return Err(OpponentsError::Params(format!("{k} must be ≥ 0")));
            }
        }
        if self.size_idx > 2 {
            return Err(OpponentsError::Params("size_idx ∈ {0,1,2}".into()));
        }
        Ok(())
    }

    /// Point defaults per archetype (SPECS/03 §3).
    pub fn point(arch: ArchetypeId) -> ArchetypeParams {
        match arch {
            ArchetypeId::Nit => ArchetypeParams {
                open_raise: 0.12,
                complete: 0.20,
                call_open: 0.45,
                three_bet: 0.045,
                call_3bet: 0.10,
                four_bet: 0.015,
                iso_check: 0.30,
                cbet_flop: 0.62,
                barrel_turn: 0.70,
                barrel_river: 0.78,
                donk: 0.60,
                check_raise: 0.80,
                bluff_river: 0.90,
                call_factor: 1.35,
                value_bet: 0.72,
                size_idx: 1,
                trap: 0.05,
            },
            ArchetypeId::Tag => ArchetypeParams {
                open_raise: 0.25,
                complete: 0.38,
                call_open: 0.58,
                three_bet: 0.09,
                call_3bet: 0.22,
                four_bet: 0.04,
                iso_check: 0.40,
                cbet_flop: 0.52,
                barrel_turn: 0.62,
                barrel_river: 0.72,
                donk: 0.50,
                check_raise: 0.74,
                bluff_river: 0.85,
                call_factor: 1.10,
                value_bet: 0.68,
                size_idx: 1,
                trap: 0.10,
            },
            ArchetypeId::Lag => ArchetypeParams {
                open_raise: 0.40,
                complete: 0.52,
                call_open: 0.70,
                three_bet: 0.14,
                call_3bet: 0.34,
                four_bet: 0.07,
                iso_check: 0.50,
                cbet_flop: 0.42,
                barrel_turn: 0.55,
                barrel_river: 0.66,
                donk: 0.42,
                check_raise: 0.68,
                bluff_river: 0.78,
                call_factor: 0.92,
                value_bet: 0.64,
                size_idx: 2,
                trap: 0.12,
            },
            ArchetypeId::Station => ArchetypeParams {
                open_raise: 0.35,
                complete: 0.60,
                call_open: 0.78,
                three_bet: 0.05,
                call_3bet: 0.45,
                four_bet: 0.03,
                iso_check: 0.45,
                cbet_flop: 0.70,
                barrel_turn: 0.80,
                barrel_river: 0.85,
                donk: 0.55,
                check_raise: 0.90,
                bluff_river: 0.95,
                call_factor: 0.62,
                value_bet: 0.70,
                size_idx: 0,
                trap: 0.30,
            },
        }
    }
}

/// Jitter ranges per field: uniform draw in `[v − j, v + j]` clamped to [0, 1]
/// (`call_factor` clamped to [0.3, 2.0]). ±0.05–0.10 as v1 (SPECS/03 §3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JitterSpec {
    pub open_raise: f64,
    pub complete: f64,
    pub call_open: f64,
    pub three_bet: f64,
    pub call_3bet: f64,
    pub four_bet: f64,
    pub iso_check: f64,
    pub cbet_flop: f64,
    pub barrel_turn: f64,
    pub barrel_river: f64,
    pub donk: f64,
    pub check_raise: f64,
    pub bluff_river: f64,
    pub call_factor: f64,
    pub value_bet: f64,
    pub trap: f64,
}

impl JitterSpec {
    /// Default ±0.05–0.10 jitter ranges.
    pub fn standard() -> JitterSpec {
        JitterSpec {
            open_raise: 0.06,
            complete: 0.06,
            call_open: 0.08,
            three_bet: 0.04,
            call_3bet: 0.08,
            four_bet: 0.03,
            iso_check: 0.08,
            cbet_flop: 0.10,
            barrel_turn: 0.10,
            barrel_river: 0.10,
            donk: 0.10,
            check_raise: 0.10,
            bluff_river: 0.08,
            call_factor: 0.10,
            value_bet: 0.06,
            trap: 0.05,
        }
    }

    /// Draw a jittered parameter set (deterministic under `rng`).
    pub fn apply(&self, base: &ArchetypeParams, rng: &mut cham_core::rng::Rng) -> ArchetypeParams {
        fn jitter(rng: &mut cham_core::rng::Rng, base: f64, j: f64) -> f64 {
            (base + (cham_core::rng::next_f64(rng) * 2.0 - 1.0) * j).clamp(0.0, 1.0)
        }
        macro_rules! jit {
            ($base:expr, $j:expr) => {
                jitter(rng, $base, $j)
            };
        }
        ArchetypeParams {
            open_raise: jit!(base.open_raise, self.open_raise),
            complete: jit!(base.complete, self.complete),
            call_open: jit!(base.call_open, self.call_open),
            three_bet: jit!(base.three_bet, self.three_bet),
            call_3bet: jit!(base.call_3bet, self.call_3bet),
            four_bet: jit!(base.four_bet, self.four_bet),
            iso_check: jit!(base.iso_check, self.iso_check),
            cbet_flop: jit!(base.cbet_flop, self.cbet_flop),
            barrel_turn: jit!(base.barrel_turn, self.barrel_turn),
            barrel_river: jit!(base.barrel_river, self.barrel_river),
            donk: jit!(base.donk, self.donk),
            check_raise: jit!(base.check_raise, self.check_raise),
            bluff_river: jit!(base.bluff_river, self.bluff_river),
            call_factor: (base.call_factor
                + (cham_core::rng::next_f64(rng) * 2.0 - 1.0) * self.call_factor)
                .clamp(0.3, 2.0),
            value_bet: jit!(base.value_bet, self.value_bet),
            size_idx: base.size_idx,
            trap: jit!(base.trap, self.trap),
        }
    }
}
