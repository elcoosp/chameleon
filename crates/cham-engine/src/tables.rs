//! Runtime bucket lookup + mmap'd artifact loading (SPECS/02 §3, §5).

use std::path::Path;

use serde::{Deserialize, Serialize};

use cham_core::card::{Card, Hand2};

use crate::EngineError;
use crate::canon::{TableView, canonical_key};
use crate::config::AbstractionConfig;

/// meta.json — everything the runtime needs besides the TOML (SPECS/02 §3).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RiverMeta {
    /// global equity quantile edges (len = river_eq_bins + 1), committed offline
    pub river_eq_edges: Vec<f64>,
    /// k-means metadata
    pub kmeans: Option<KmeansMeta>,
    /// deterministic bucket for orbit keys missing from a table (M1-tiny mode;
    /// decision D-006 — full-coverage builds ship no misses)
    pub default_bucket: u16,
    pub blake3: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KmeansMeta {
    pub k: u32,
    pub feature: String, // "river_cdf16"
    pub feature_runs: u32,
    pub seeds: Vec<u64>,
    pub inertia: Vec<f64>,
}

/// Sentinel default_bucket meaning "compute the deterministic fallback at runtime"
/// (M1-tiny sampled tables; decision D-006).
pub const MISS_SENTINEL: u16 = 0xFFFF;

/// One bucket table loaded into owned bytes (flop or turn).
///
/// Decision D-008: the spec calls for mmap'd read-only tables, but `memmap2`'s
/// constructors are `unsafe` and the workspace is `#![forbid(unsafe_code)]`. The
/// M1-tiny artifacts are ≤ ~15 MB, so owned bytes are equivalent in practice; the
/// full 110 MB turn table (M2) can enable a dedicated `#[allow(unsafe_code)]`
/// mmap module behind a feature flag once a human signs off.
pub struct MmapTable {
    bytes: Vec<u8>,
    view_len: u64,
    default_bucket: u16,
}

impl MmapTable {
    pub fn open(path: &Path) -> Result<MmapTable, EngineError> {
        let bytes = std::fs::read(path).map_err(|e| EngineError::Artifact {
            path: path.to_path_buf(),
            reason: format!("read: {e}"),
        })?;
        let tv = TableView::parse(&bytes).map_err(|e| match e {
            EngineError::Artifact { reason, .. } => EngineError::Artifact {
                path: path.to_path_buf(),
                reason,
            },
            other => other,
        })?;
        Ok(MmapTable {
            view_len: tv.len(),
            default_bucket: tv.default_bucket,
            bytes,
        })
    }

    #[inline]
    pub fn lookup(&self, key: u64) -> u16 {
        let bytes: &[u8] = &self.bytes;
        let n = self.view_len as usize;
        let mut lo = 0usize;
        let mut hi = n;
        while lo < hi {
            let mid = (lo + hi) / 2;
            let o = crate::canon::HEADER_LEN + mid * 8;
            let k = u64::from_le_bytes(bytes[o..o + 8].try_into().expect("8"));
            if k < key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo < n {
            let o = crate::canon::HEADER_LEN + lo * 8;
            let k = u64::from_le_bytes(bytes[o..o + 8].try_into().expect("8"));
            if k == key {
                let o2 = crate::canon::HEADER_LEN + n * 8 + lo * 2;
                return u16::from_le_bytes(bytes[o2..o2 + 2].try_into().expect("2"));
            }
        }
        self.default_bucket
    }

    pub fn len(&self) -> u64 {
        self.view_len
    }

    pub fn is_empty(&self) -> bool {
        self.view_len == 0
    }
}

/// River bucketing: no table — exact equity quantile × texture (SPECS/02 §3).
pub struct RiverBucketer {
    pub edges: Vec<f64>,
    pub texture_classes: u32,
}

impl RiverBucketer {
    pub fn new(meta: &RiverMeta, cfg: &AbstractionConfig) -> RiverBucketer {
        RiverBucketer {
            edges: meta.river_eq_edges.clone(),
            texture_classes: cfg.n_texture_classes(),
        }
    }

    /// Board texture class (deterministic, board-aware):
    /// class = paired * 1 + monotone * 2 + connected * 4 (mod texture_classes),
    /// where paired = board has a rank pair, monotone = ≥3 same suit,
    /// connected = some 3 board ranks within a 4-window (incl. wheel).
    pub fn texture(&self, board: &[Card]) -> u32 {
        let mut rank_count = [0u8; 13];
        let mut suit_count = [0u8; 4];
        let mut mask = 0u16;
        for c in board {
            rank_count[c.rank() as usize] += 1;
            suit_count[c.suit() as usize] += 1;
            mask |= 1u16 << c.rank();
        }
        let paired = rank_count.iter().any(|&n| n >= 2);
        let monotone = suit_count.iter().any(|&n| n >= 3);
        let mut connected = false;
        for h in (4..=12).rev() {
            let need = ((1u16 << (h + 1)) - 1) & !((1u16 << (h - 4)) - 1);
            // 3 of the 5 window ranks present on the board
            if (mask & need).count_ones() >= 3 {
                connected = true;
                break;
            }
        }
        let wheel = (1u16 << 12) | 0b1111;
        if (mask & wheel).count_ones() >= 3 {
            connected = true;
        }
        let mut cls = 0u32;
        if paired {
            cls += 1;
        }
        if monotone {
            cls += 2;
        }
        if connected {
            cls += 4;
        }
        if self.texture_classes >= 8 {
            cls
        } else if self.texture_classes == 4 {
            // fold monotone into the low bits: paired + connected only
            (paired as u32) + 2 * (connected as u32)
        } else {
            cls % self.texture_classes
        }
    }

    #[inline]
    fn eq_bin(&self, equity: f64) -> usize {
        // edges ascending; equity ∈ [0,1] — clamp into the last bin at 1.0
        let idx = self.edges.partition_point(|&e| e <= equity);
        idx.saturating_sub(1).min(self.edges.len() - 2)
    }

    /// Bucket id = texture * eq_bins + eq_bin (≤ 512 at full spec).
    pub fn bucket(&self, equity: f64, board: &[Card]) -> u16 {
        let bins = (self.edges.len() - 1) as u32;
        let b = self.texture(board) * bins + self.eq_bin(equity) as u32;
        b as u16
    }
}

/// Load `meta.json` from a bucket directory.
pub fn load_meta(dir: &Path) -> Result<RiverMeta, EngineError> {
    let p = dir.join("meta.json");
    let text = std::fs::read_to_string(&p).map_err(|e| EngineError::Artifact {
        path: p.clone(),
        reason: format!("read meta: {e}"),
    })?;
    serde_json::from_str(&text).map_err(|e| EngineError::Meta(format!("parse: {e}")))
}

/// Convenience: exact river equity of `hero` vs uniform on a 5-card board.
pub fn river_equity(hero: Hand2, board: &[Card; 5]) -> f64 {
    let range = cham_core::eval::Range::all();
    let (w, t) = cham_core::eval::equity_exact(hero, &range, board);
    w + t / 2.0
}

/// Preflop bucket = the pinned 169-class id.
#[inline]
pub fn preflop_bucket(hole: Hand2) -> u16 {
    hole.class_id() as u16
}

/// Flop/turn bucket via the orbit tables; falls back to the meta default on a
/// table miss (M1-tiny sampled tables carry a deterministic default).
#[inline]
pub fn orbit_bucket(table: &MmapTable, hole: Hand2, board: &[Card]) -> u16 {
    let key = canonical_key(hole, board);
    table.lookup(key)
}
