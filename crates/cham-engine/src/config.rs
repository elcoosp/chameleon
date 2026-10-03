//! `AbstractionConfig` (SPECS/02 §2) and the `abstraction_hash`
//! (= blake3 over TOML bytes ‖ bucket artifacts — retraining buckets invalidates
//! dependent blueprints loudly, not silently).

use serde::{Deserialize, Serialize};

use cham_core::consts::EPS_EV;

/// Bucketing parameters. The v1 knobs `soft_bucket`, `depth_bands` and
/// `equity_mc_rollouts` are REMOVED (review cuts; grep-verified absent).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BucketConfig {
    /// only "exact169" is supported in v1
    pub preflop: String,
    pub flop_k: u32,
    pub turn_k: u32,
    pub river_eq_bins: u32,
    pub river_texture_classes: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LadderConfig {
    /// Preflop open sizes in bb (SPECS/02 §4).
    ///
    /// **Deprecated / dead (F6b, 2026-10-01).** This field is validated
    /// by `AbstractionConfig::validate` and its TOML bytes are folded
    /// into `abstraction_hash`, so changing it invalidates every
    /// existing policy artifact — but the ladder never reads it.
    /// Preflop sizing actually comes from `raise_fracs`. The external
    /// competitiveness audit (§F6b) flagged the `[2.2, 3.0]`-style
    /// opens in `abstraction.toml` as fiction for this reason.
    ///
    /// Kept as-is for hash stability. A successor should either
    /// (a) wire it into the preflop ladder (behavior change, requires
    /// retrain), or (b) delete it in a versioned migration that bumps
    /// `version` and rebuilds all artifacts. See
    /// `docs/plans/F6C-SIZE-BUCKET-DESIGN-2026-10-01.md` §5.
    pub preflop_open_bb: Vec<f64>,
    /// `preflop_levels_bb[lvl]` = open sizes in bb when
    /// `lvl` Raise-class actions already happened preflop. Level 0 = open
    /// (≈2.5 bb), level 1 = 3-bet (≈8 bb), level 2 = 4-bet (≈20 bb); beyond
    /// the last level only jam remains. Empty = legacy `raise_fracs`
    /// behaviour. Keying change: retrain when enabling.
    ///
    /// Hash stability: skipped from the hashed serialization while empty,
    /// so legacy configs keep their `abstraction_hash`; setting levels
    /// changes the hash loudly (dependent blueprints invalidate).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preflop_levels_bb: Vec<Vec<f64>>,
    pub raise_fracs: Vec<f64>,
    pub flop_bet_fracs: Vec<f64>,
    pub turn_bet_fracs: Vec<f64>,
    pub river_bet_fracs: Vec<f64>,
    pub raises_per_street_cap: u32,
    pub all_in_always: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AbstractionConfig {
    pub version: u32,
    pub buckets: BucketConfig,
    pub ladder: LadderConfig,
    /// log-spaced band EDGES (len = bands+1), default 16 bands over [0.3, 40]
    pub spr_bands: Vec<f64>,
    pub seq_history_len: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum SprBandError {
    #[error("spr_bands must be ascending, finite and have ≥ 2 entries")]
    BadBands,
}

impl AbstractionConfig {
    /// The full v2 defaults (SPECS/02 §2).
    pub fn full() -> AbstractionConfig {
        AbstractionConfig {
            version: 2,
            buckets: BucketConfig {
                preflop: "exact169".into(),
                flop_k: 300,
                turn_k: 200,
                river_eq_bins: 64,
                river_texture_classes: 8,
            },
            ladder: LadderConfig {
                preflop_open_bb: vec![2.2, 3.0],
                preflop_levels_bb: vec![vec![2.5], vec![8.0], vec![20.0]],
                raise_fracs: vec![0.5, 1.0],
                flop_bet_fracs: vec![0.33, 0.75],
                turn_bet_fracs: vec![0.33, 0.75],
                river_bet_fracs: vec![0.33, 0.66, 1.25],
                raises_per_street_cap: 2,
                all_in_always: true,
            },
            spr_bands: log_bands(16, 0.3, 40.0),
            seq_history_len: 8,
        }
    }

    /// The M1 walking-skeleton profile (SPECS/11 M1).
    pub fn tiny() -> AbstractionConfig {
        AbstractionConfig {
            version: 2,
            buckets: BucketConfig {
                preflop: "exact169".into(),
                flop_k: 32,
                turn_k: 16,
                river_eq_bins: 16,
                river_texture_classes: 4,
            },
            ladder: LadderConfig {
                preflop_open_bb: vec![2.5],
                preflop_levels_bb: vec![],
                raise_fracs: vec![1.0],
                flop_bet_fracs: vec![0.5],
                turn_bet_fracs: vec![0.5],
                river_bet_fracs: vec![0.5, 1.25],
                raises_per_street_cap: 1,
                all_in_always: true,
            },
            spr_bands: log_bands(16, 0.3, 40.0),
            seq_history_len: 8,
        }
    }

    /// Eager validation (SPECS/00 §5): no defaults on gameplay-affecting fields.
    pub fn validate(&self) -> Result<(), crate::EngineError> {
        if self.buckets.preflop != "exact169" {
            return Err(crate::EngineError::Config(
                "buckets.preflop must be exact169".into(),
            ));
        }
        if self.buckets.flop_k == 0 || self.buckets.turn_k == 0 {
            return Err(crate::EngineError::Config(
                "flop_k/turn_k must be > 0".into(),
            ));
        }
        if self.buckets.river_eq_bins == 0 || !self.buckets.river_eq_bins.is_power_of_two() {
            return Err(crate::EngineError::Config(
                "river_eq_bins must be a power of two".into(),
            ));
        }
        if self.buckets.river_texture_classes == 0 || self.buckets.river_texture_classes > 8 {
            return Err(crate::EngineError::Config(
                "river_texture_classes ∈ [1, 8]".into(),
            ));
        }
        for f in [
            &self.ladder.preflop_open_bb,
            &self.ladder.raise_fracs,
            &self.ladder.flop_bet_fracs,
            &self.ladder.turn_bet_fracs,
            &self.ladder.river_bet_fracs,
        ] {
            if f.is_empty() || f.iter().any(|x| !x.is_finite() || *x <= 0.0) {
                return Err(crate::EngineError::Config(
                    "ladder fracs must be positive".into(),
                ));
            }
        }
        for lvl in &self.ladder.preflop_levels_bb {
            if lvl.is_empty() || lvl.iter().any(|x| !x.is_finite() || *x <= 0.0) {
                return Err(crate::EngineError::Config(
                    "ladder preflop_levels_bb levels must be non-empty and positive".into(),
                ));
            }
        }
        self.validate_bands()
            .map_err(|e| crate::EngineError::Config(format!("spr bands: {e}")))?;
        Ok(())
    }

    pub fn validate_bands(&self) -> Result<(), SprBandError> {
        if self.spr_bands.len() < 2 {
            return Err(SprBandError::BadBands);
        }
        for w in self.spr_bands.windows(2) {
            if w[0] >= w[1] || !w[0].is_finite() || !w[1].is_finite() {
                return Err(SprBandError::BadBands);
            }
        }
        Ok(())
    }

    pub fn n_texture_classes(&self) -> u32 {
        self.buckets.river_texture_classes
    }
}

/// `n` log-spaced edges over [`lo`, `hi`] → `n+1` band edges.
pub fn log_bands(n: u32, lo: f64, hi: f64) -> Vec<f64> {
    let n = n.max(1);
    let mut v = Vec::with_capacity(n as usize + 1);
    for i in 0..=n {
        let t = i as f64 / n as f64;
        v.push(lo * (hi / lo).powf(t));
    }
    v
}

/// SPR band index: bands are `[edges[i], edges[i+1])`, values below the first edge
/// clamp to 0, above the last edge clamp to the last band.
pub fn spr_band_index(edges: &[f64], spr: f64) -> u8 {
    // partition_point on ascending edges: first edge strictly greater than spr
    let idx = edges.partition_point(|&e| e <= spr);
    (idx.saturating_sub(1).min(edges.len() - 2)) as u8
}

/// FNV-1a over a byte stream — hot-path key mixing only (never tamper evidence).
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// `abstraction_hash` = blake3(TOML ‖ artifacts) folded to u64 (SPECS/00 §7).
pub fn abstraction_hash(toml_bytes: &[u8], artifact_bytes: &[&[u8]]) -> u64 {
    let mut h = blake3::Hasher::new();
    h.update(toml_bytes);
    for a in artifact_bytes {
        h.update(&(a.len() as u64).to_le_bytes());
        h.update(a);
    }
    let out = h.finalize();
    u64::from_le_bytes(out.as_bytes()[..8].try_into().expect("8 bytes"))
}

/// Load a config from TOML text and validate.
pub fn parse_config(toml_text: &str) -> Result<AbstractionConfig, crate::EngineError> {
    let cfg: AbstractionConfig =
        toml::from_str(toml_text).map_err(|e| crate::EngineError::Config(format!("toml: {e}")))?;
    cfg.validate()?;
    Ok(cfg)
}

/// Keep float noise out of band comparisons.
pub fn eps() -> f64 {
    EPS_EV
}
