//! The encoder (SPECS/02 §5): `InfoSetKey` — a pure, deterministic u64 mixing of
//! street | position | SPR band | our bucket | legal mask | canonicalized action
//! sequence. NO Monte Carlo anywhere on this path (structural test enforces).
//!
//! Depth-free keying (review A8/D8 alignment fix): the seq records
//! `(actor, class, size_bucket)` where size_bucket quantizes the fraction of the
//! EFFECTIVE STACK bet (not pot, not absolute chips) — fractionally-identical
//! sequences at different stack depths produce the same SPR band and the same
//! buckets, hence the same key. Fixed-size (bb-denominated) opens at different
//! depths differ in SPR → different keys.

use std::path::Path;

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use cham_core::card::Hand2;
use cham_core::engine::Street;
use cham_core::obs::{Observables, Player};

use crate::EngineError;
use crate::config::{AbstractionConfig, abstraction_hash, fnv1a, spr_band_index};
use crate::ladder::{ActionLadder, record_action};
use crate::tables::{MmapTable, RiverBucketer, RiverMeta, load_meta, orbit_bucket, preflop_bucket};

/// Action classes recorded in the key sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ActionClass {
    Fold = 0,
    Check = 1,
    Call = 2,
    Bet = 3,
    Raise = 4,
}

impl ActionClass {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
    pub fn from_u8(v: u8) -> ActionClass {
        match v {
            0 => ActionClass::Fold,
            1 => ActionClass::Check,
            2 => ActionClass::Call,
            3 => ActionClass::Bet,
            _ => ActionClass::Raise,
        }
    }
}

/// One recorded sequence entry (public info; depth-free).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeqEntry {
    pub street: u8,
    pub actor: u8,
    pub class: ActionClass,
    pub size_bucket: u8,
}

/// Deterministic action sequence: fixed capacity (4 streets × window 8 = 32).
/// Copy — lives inside traversals without allocation.
///
/// L-1 fix (2026-09-27): added `overflow: [u8; 4]`, a per-street counter of
/// actions dropped past the window. Before this, two DIFFERENT histories
/// that shared the first 8 actions of a street but diverged afterward hashed
/// to the SAME infoset key (raise war truncation) — a silent key collision.
/// `key_for` now folds the overflow counts into the hash, so any two
/// distinct histories produce distinct keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionSeq {
    pub entries: [SeqEntry; 32],
    pub lens: [u8; 4],
    /// Per-street count of actions dropped past the window (saturating u8:
    /// 255 distinct beyond-window actions on one street is already
    /// astronomically more than any real hand — the counter only needs to
    /// DISAMBIGUATE, never to be exact).
    pub overflow: [u8; 4],
}

impl Default for ActionSeq {
    fn default() -> Self {
        ActionSeq {
            entries: [SeqEntry {
                street: 0,
                actor: 0,
                class: ActionClass::Fold,
                size_bucket: 0,
            }; 32],
            lens: [0; 4],
            overflow: [0; 4],
        }
    }
}

impl ActionSeq {
    pub fn push(&mut self, street: Street, e: SeqEntryRaw) {
        let s = street.as_u8() as usize;
        let n = &mut self.lens[s];
        if (*n as usize) < 8 {
            let idx = s * 8 + *n as usize;
            self.entries[idx] = SeqEntry {
                street: street.as_u8(),
                actor: e.actor,
                class: e.class,
                size_bucket: e.size_bucket,
            };
            *n += 1;
        } else {
            // L-1 fix: record the overflow instead of dropping silently. The
            // window is a hard cap (ActionSeq is `Copy`, no allocation), but
            // the counter disambiguates any two histories that diverge past
            // the window. Saturating add: 255 is far beyond any real hand.
            self.overflow[s] = self.overflow[s].saturating_add(1);
        }
    }
    pub fn count_class(&self, street: Street, class: ActionClass) -> u32 {
        let s = street.as_u8() as usize;
        let mut c = 0;
        for i in 0..self.lens[s] as usize {
            if self.entries[s * 8 + i].class == class {
                c += 1;
            }
        }
        c
    }
    pub fn is_empty(&self) -> bool {
        self.lens == [0; 4]
    }
}

use crate::ladder::SeqEntryRaw;

/// Bucket lookup telemetry (§3.2a): table hits vs suit-blind-fallback
/// hits per street (index 0=preflop,1=flop,2=turn,3=river). Print at the
/// end of every train/ladder run; a sampled table shows ~1–2% flop hits.
#[derive(Clone, Copy, Debug, Default)]
pub struct BucketStats {
    pub table_hit: [u64; 4],
    pub fallback_hit: [u64; 4],
}

impl BucketStats {
    pub fn hit_rate(&self, street: usize) -> Option<f64> {
        let t = self.table_hit[street];
        let f = self.fallback_hit[street];
        if t + f == 0 {
            None
        } else {
            Some(t as f64 / (t + f) as f64)
        }
    }
}

/// Refuse sampled bucket tables (§3.2a). Flop/turn `meta.json` entries
/// carry `coverage: "full" | "sampled"`; a sampled table silently routes
/// ~98% of lookups through the suit-blind `strength_now` fallback.
/// Returns `Err` unless coverage is full or `CHAM_ALLOW_SAMPLED_BUCKETS`
/// is set. Call at train / ladder / play startup.
pub fn require_full_coverage(dir: &Path) -> Result<(), EngineError> {
    let raw = std::fs::read(dir.join("meta.json"))
        .map_err(|e| EngineError::Config(format!("require_full_coverage: read meta.json: {e}")))?;
    let v: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|e| EngineError::Config(format!("require_full_coverage: parse: {e}")))?;
    for st in ["flop", "turn"] {
        let cov = v
            .get(st)
            .and_then(|s| s.get("coverage"))
            .and_then(|c| c.as_str())
            .unwrap_or("sampled");
        if cov != "full" && std::env::var("CHAM_ALLOW_SAMPLED_BUCKETS").is_err() {
            return Err(EngineError::Config(format!(
                "{st} bucket table is sampled (fallback = suit-blind strength_now); \
                 rebuild with full coverage or set CHAM_ALLOW_SAMPLED_BUCKETS=1"
            )));
        }
    }
    Ok(())
}

/// The infoset key: nonzero u64 (FNV-1a mixed byte stream, high bit forced).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InfoSetKey(pub u64);

impl InfoSetKey {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// The encoder: ladders + bucket tables + river bucketer + equity cache.
#[derive(Clone)]
pub struct Encoder {
    pub cfg: AbstractionConfig,
    pub ladder: ActionLadder,
    flop: Option<MmapTable>,
    turn: Option<MmapTable>,
    river: RiverBucketer,
    meta: RiverMeta,
    hash: u64,
    /// river equity → bucket memo (deterministic; capped). Purity holds: values are
    /// input-determined; the cap only bounds memory (spec: cross-visit caching
    /// unnecessary at P1 speeds, but cheap and safe here).
    eq_cache: FxHashMap<u64, u16>,
    /// flop/turn fallback-bucket memo (PERF-PLAN T4): `strength_now` enumerates
    /// ~1326 villain combos per call, but training revisits the same
    /// (hole, board) thousands of times. The fallback is pure, so memoizing
    /// is behavior-identical (bit-exact keys) and removes the dominant
    /// per-visit cost on table-less encoders.
    fallback_cache: FxHashMap<u64, u16>,
    /// ExploitBayes belief bin (0 = non-Bayes default; SPECS/04 §5). Part of the key.
    belief_bin: u8,
    /// §3.2a hit/miss telemetry (interior: `bucket` takes `&mut self`).
    stats: BucketStats,
}

const EQ_CACHE_CAP: usize = 400_000;
/// Flop/turn fallback-bucket memo cap (bounded; purity-preserving clear).
const FALLBACK_CACHE_CAP: usize = 131_072;

impl Encoder {
    /// Alias kept for the CLI: same as from_config with a directory of artifacts.
    pub fn from_artifacts_dir(dir: &Path, cfg: AbstractionConfig) -> Result<Encoder, EngineError> {
        Self::from_config(cfg, dir)
    }

    /// Load from a bucket directory containing flop.bin/turn.bin/meta.json
    /// (either may be absent in the M1-tiny profile; missing tables fall back to
    /// the deterministic strength-quantile bucket, decision D-006).
    pub fn from_config(cfg: AbstractionConfig, models_dir: &Path) -> Result<Encoder, EngineError> {
        cfg.validate()?;
        let meta = load_meta(models_dir)?;
        let river = RiverBucketer::new(&meta, &cfg);
        let flop = p(models_dir, "flop.bin")
            .map(|p| MmapTable::open(&p))
            .transpose()?;
        let turn = p(models_dir, "turn.bin")
            .map(|p| MmapTable::open(&p))
            .transpose()?;
        let hash = {
            let toml_bytes = serde_json::to_vec(&cfg)
                .map_err(|e| EngineError::Config(format!("serialize: {e}")))?;
            let mut arts: Vec<Vec<u8>> = Vec::new();
            for f in ["flop.bin", "turn.bin", "meta.json"] {
                let fp = models_dir.join(f);
                if fp.exists() {
                    arts.push(std::fs::read(&fp)?);
                }
            }
            let refs: Vec<&[u8]> = arts.iter().map(|v| v.as_slice()).collect();
            abstraction_hash(&toml_bytes, &refs)
        };
        Ok(Encoder {
            ladder: ActionLadder::new(&cfg),
            flop,
            turn,
            river,
            meta,
            hash,
            cfg,
            eq_cache: FxHashMap::default(),
            fallback_cache: FxHashMap::default(),
            belief_bin: 0,
            stats: BucketStats::default(),
        })
    }

    /// Config-only encoder for tests/toys: no tables, deterministic fallbacks.
    pub fn cfg_only(cfg: AbstractionConfig) -> Result<Encoder, EngineError> {
        let meta = RiverMeta {
            river_eq_edges: equal_mass_edges(cfg.buckets.river_eq_bins as usize),
            kmeans: None,
            default_bucket: 0,
            blake3: String::new(),
        };
        let river = RiverBucketer::new(&meta, &cfg);
        Ok(Encoder {
            ladder: ActionLadder::new(&cfg),
            flop: None,
            turn: None,
            river,
            meta,
            hash: 0,
            cfg,
            eq_cache: FxHashMap::default(),
            fallback_cache: FxHashMap::default(),
            belief_bin: 0,
            stats: BucketStats::default(),
        })
    }

    /// Set the ExploitBayes belief bin (trainer, per session block).
    pub fn set_belief_bin(&mut self, bin: u8) {
        self.belief_bin = bin;
    }

    pub fn current_belief_bin(&self) -> u8 {
        self.belief_bin
    }

    pub fn abstraction_hash(&self) -> u64 {
        self.hash
    }

    pub fn meta(&self) -> &RiverMeta {
        &self.meta
    }

    /// §3.2a bucket hit/miss telemetry snapshot.
    pub fn bucket_stats(&self) -> BucketStats {
        self.stats
    }

    /// SPR band for this view.
    pub fn spr_band(&self, obs: &Observables<'_>) -> u8 {
        spr_band_index(&self.cfg.spr_bands, obs.spr())
    }

    /// Our bucket: preflop = 169-class id; flop/turn = orbit table (fallback =
    /// strength quantile, D-006); river = exact-equity quantile × texture.
    pub fn bucket(&mut self, obs: &Observables<'_>) -> u16 {
        let board = &obs.board[..obs.board_len as usize];
        match obs.street {
            Street::Preflop => preflop_bucket(obs.hole),
            Street::Flop | Street::Turn => {
                let has_table = match obs.street {
                    Street::Flop => self.flop.is_some(),
                    _ => self.turn.is_some(),
                };
                if has_table {
                    let table = match obs.street {
                        Street::Flop => self.flop.as_ref().expect("checked"),
                        _ => self.turn.as_ref().expect("checked"),
                    };
                    let b = orbit_bucket(table, obs.hole, board);
                    if b != crate::tables::MISS_SENTINEL {
                        self.stats.table_hit[obs.street.as_u8() as usize] += 1;
                        return b;
                    }
                }
                // Table-less (or miss) fallback: memoized pure function of
                // (hole, board) — bit-exact vs recomputation.
                let key = board_pack(obs.hole, board);
                if let Some(&b) = self.fallback_cache.get(&key) {
                    return b;
                }
                let b = self.fallback_bucket(obs, board);
                if self.fallback_cache.len() >= FALLBACK_CACHE_CAP {
                    self.fallback_cache.clear();
                }
                self.fallback_cache.insert(key, b);
                self.stats.fallback_hit[obs.street.as_u8() as usize] += 1;
                b
            }
            Street::River => {
                let key = seq_pack(obs.hole, board);
                if let Some(&b) = self.eq_cache.get(&key) {
                    return b;
                }
                let mut board5 = [cham_core::card::Card(0); 5];
                board5[..board.len()].copy_from_slice(board);
                let eq = crate::tables::river_equity(obs.hole, &board5);
                let b = self.river.bucket(eq, board);
                if self.eq_cache.len() >= EQ_CACHE_CAP {
                    self.eq_cache.clear();
                }
                self.eq_cache.insert(key, b);
                b
            }
        }
    }

    /// Deterministic fallback: strength_now quantile into k bands (pure, board-aware).
    fn fallback_bucket(&self, obs: &Observables<'_>, board: &[cham_core::card::Card]) -> u16 {
        let k = match obs.street {
            Street::Flop => self.cfg.buckets.flop_k,
            _ => self.cfg.buckets.turn_k,
        } as f64;
        let s = cham_core::eval::strength_now(obs.hole, board);
        ((s * (k - 1.0)).round() as u16).min(k as u16 - 1)
    }

    /// Ladder slots for this view (row width W candidates).
    pub fn slots(
        &self,
        obs: &Observables<'_>,
        seq: &ActionSeq,
    ) -> arrayvec::ArrayVec<crate::ladder::AbstractAction, 12> {
        self.ladder.slots(obs, seq)
    }

    /// The legal mask over ladder slots (bit i = slot i is engine-legal).
    pub fn legal_mask(&self, obs: &Observables<'_>, seq: &ActionSeq) -> u16 {
        let slots = self.ladder.slots(obs, seq);
        let mut mask: u16 = 0;
        for (i, s) in slots.iter().enumerate() {
            if cham_core::obs::is_legal(obs, s.action) {
                mask |= 1 << i;
            }
        }
        mask
    }

    /// Number of slots == row width W (invariant I8).
    pub fn n_slots(&self, obs: &Observables<'_>, seq: &ActionSeq) -> usize {
        self.ladder.slots(obs, seq).len()
    }

    /// THE key. Byte stream (fixed order):
    /// street u8 | position u8 | spr_band u8 | our_bucket u16 LE | legal_mask u16 LE
    /// | seq entries (actor u8, class u8, size_bucket u8) with per-street lengths.
    /// FNV-1a mixed; high bit OR-ed so the key is never 0.
    pub fn key(&mut self, obs: &Observables<'_>, seq: &ActionSeq) -> InfoSetKey {
        let slots = self.ladder.slots(obs, seq);
        self.key_for(obs, seq, &slots)
    }

    /// Key from precomputed ladder slots (PERF-PLAN T4): hot callers
    /// (traversal) compute slots once and reuse them for the mask, the width
    /// and the action mapping instead of re-deriving the ladder 3× per visit.
    /// Byte-identical to [`Encoder::key`].
    pub fn key_for(
        &mut self,
        obs: &Observables<'_>,
        seq: &ActionSeq,
        slots: &arrayvec::ArrayVec<crate::ladder::AbstractAction, 12>,
    ) -> InfoSetKey {
        let mut mask: u16 = 0;
        for (i, s) in slots.iter().enumerate() {
            if cham_core::obs::is_legal(obs, s.action) {
                mask |= 1 << i;
            }
        }
        let bucket = self.bucket(obs);
        // Fixed-size stack buffer (max 8 + 4×(1 + 8×3) = 108 bytes): the same
        // byte stream as before, with no per-visit allocation. FNV-1a over
        // the fixed array with plain indexing (no bounds checks in the loop).
        let mut bytes = [0u8; 128];
        let mut n = 0usize;
        bytes[n] = obs.street.as_u8();
        n += 1;
        bytes[n] = obs.player.as_usize() as u8;
        n += 1;
        bytes[n] = self.spr_band(obs);
        n += 1;
        bytes[n] = (bucket & 0xff) as u8;
        bytes[n + 1] = (bucket >> 8) as u8;
        n += 2;
        bytes[n] = self.belief_bin; // ExploitBayes bin (0 when unused)
        n += 1;
        bytes[n] = (mask & 0xff) as u8;
        bytes[n + 1] = (mask >> 8) as u8;
        n += 2;
        // §3.5 Patch 2: with history compression, full detail lives only
        // on the current street; finished streets collapse to a 2-byte
        // summary (pot/stack via the SPR band carry what earlier betting
        // implies). Gated — see `history_compression_enabled`.
        let cur = obs.street.as_u8() as usize;
        let compressed = self.history_compression_enabled();
        for street in 0..4usize {
            if compressed && street != cur && street < cur {
                // SUMMARY of a finished street.
                let (n_agg, last_aggr) = Self::summarize_street(seq, street);
                bytes[n] = n_agg.min(3);
                bytes[n + 1] = last_aggr;
                n += 2;
                continue;
            }
            if compressed && street > cur {
                continue; // future streets: nothing
            }
            let len = seq.lens[street] as usize;
            bytes[n] = seq.lens[street];
            n += 1;
            // L-1 fix (2026-09-27): emit the per-street overflow counter so
            // two histories that share the first 8 actions of a street but
            // diverge afterwards produce DIFFERENT keys. The buffer is 128
            // bytes; worst case is 8 header + 4×(1 + 8×3 + 1) = 8 + 4×26 = 112,
            // still under 128.
            bytes[n] = seq.overflow[street];
            n += 1;
            for i in 0..len {
                let e = &seq.entries[street * 8 + i];
                bytes[n] = e.actor;
                bytes[n + 1] = e.class.as_u8();
                bytes[n + 2] = e.size_bucket;
                n += 3;
            }
        }
        InfoSetKey(fnv1a(&bytes[..n]) | (1 << 63))
    }

    /// §3.5 Patch 2 — history compression gate. When `CHAM_COMPRESS_HISTORY=1`,
    /// `key_for` keeps FULL action detail on the current street and a 2-byte
    /// SUMMARY (aggressive-action count capped at 3, last-aggressor actor or
    /// 2=none) for finished streets; future streets contribute nothing.
    /// Imperfect recall (no convergence guarantee, standard in practice):
    /// KEYING CHANGE — retrain when enabling. Default OFF so shipped bundles
    /// keep their keys. Gate empirically with the fine-information BR (§3.3):
    /// keep only if fine-BR does not worsen at equal wall-clock while infosets
    /// drop several-fold.
    fn history_compression_enabled(&self) -> bool {
        // rule-4 (2026-10-07): v>=3 reads the config field (hashed); v2
        // keeps the legacy env flag.
        if self.cfg.version >= 3 {
            return self.cfg.compress_history;
        }
        use std::sync::OnceLock;
        static FLAG: OnceLock<bool> = OnceLock::new();
        *FLAG.get_or_init(|| {
            std::env::var("CHAM_COMPRESS_HISTORY")
                .ok()
                .map(|v| v == "1")
                .unwrap_or(false)
        })
    }

    /// Summary of a finished street: (aggressive-action count capped at 3,
    /// last-aggressor actor, or 2 when nobody aggressed).
    fn summarize_street(seq: &ActionSeq, street: usize) -> (u8, u8) {
        let mut n_agg: u8 = 0;
        let mut last_aggr: u8 = 2;
        for i in 0..seq.lens[street] as usize {
            let e = &seq.entries[street * 8 + i];
            if e.class == ActionClass::Bet || e.class == ActionClass::Raise {
                n_agg = n_agg.saturating_add(1).min(3);
                last_aggr = e.actor;
            }
        }
        (n_agg, last_aggr)
    }

    /// Record an action into the seq (deterministic; depth-free sizing).
    pub fn record(
        &self,
        obs_before: &Observables<'_>,
        actor: Player,
        a: cham_core::engine::Action,
        seq: &mut ActionSeq,
    ) {
        record_action(&self.ladder, obs_before, actor, a, seq);
    }
}

fn p(dir: &Path, name: &str) -> Option<std::path::PathBuf> {
    let f = dir.join(name);
    if f.exists() { Some(f) } else { None }
}

/// Pack (hole, board) for the equity cache key.
fn seq_pack(hole: Hand2, board: &[cham_core::card::Card]) -> u64 {
    let mut v = hole.0 as u64;
    for (i, c) in board.iter().enumerate() {
        v |= (c.idx() as u64) << (16 + 8 * i);
    }
    v
}

/// Pack (hole, board) for the flop/turn fallback-bucket memo. The board
/// length rides in the top byte: without it a turn board whose 4th card is
/// `Card(0)` packs identically to its 3-card flop prefix (zero shift payload),
/// aliasing flop-band and turn-band buckets. Caught by key-stream diff.
fn board_pack(hole: Hand2, board: &[cham_core::card::Card]) -> u64 {
    seq_pack(hole, board) | ((board.len() as u64) << 56)
}

/// Equal-mass quantile edges in [0,1] (runtime default when no meta is present).
pub fn equal_mass_edges(bins: usize) -> Vec<f64> {
    (0..=bins).map(|i| i as f64 / bins as f64).collect()
}
