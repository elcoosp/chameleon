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

use crate::config::{abstraction_hash, fnv1a, spr_band_index, AbstractionConfig};
use crate::ladder::{record_action, ActionLadder};
use crate::tables::{load_meta, orbit_bucket, preflop_bucket, MmapTable, RiverBucketer, RiverMeta};
use crate::EngineError;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionSeq {
    pub entries: [SeqEntry; 32],
    pub lens: [u8; 4],
}

impl Default for ActionSeq {
    fn default() -> Self {
        ActionSeq { entries: [SeqEntry { street: 0, actor: 0, class: ActionClass::Fold, size_bucket: 0 }; 32], lens: [0; 4] }
    }
}

impl ActionSeq {
    pub fn push(&mut self, street: Street, e: SeqEntryRaw) {
        let s = street.as_u8() as usize;
        let n = &mut self.lens[s];
        if (*n as usize) < 8 {
            let idx = s * 8 + *n as usize;
            self.entries[idx] = SeqEntry { street: street.as_u8(), actor: e.actor, class: e.class, size_bucket: e.size_bucket };
            *n += 1;
        }
        // beyond the window the entry is dropped deterministically (window 8)
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

/// The infoset key: nonzero u64 (FNV-1a mixed byte stream, high bit forced).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InfoSetKey(pub u64);

impl InfoSetKey {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// The encoder: ladders + bucket tables + river bucketer + equity cache.
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
    /// ExploitBayes belief bin (0 = non-Bayes default; SPECS/04 §5). Part of the key.
    belief_bin: u8,
}

const EQ_CACHE_CAP: usize = 400_000;

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
        let flop = p(models_dir, "flop.bin").map(|p| MmapTable::open(&p)).transpose()?;
        let turn = p(models_dir, "turn.bin").map(|p| MmapTable::open(&p)).transpose()?;
        let hash = {
            let toml_bytes = serde_json::to_vec(&cfg).map_err(|e| EngineError::Config(format!("serialize: {e}")))?;
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
            belief_bin: 0,
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
            belief_bin: 0,
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

    /// SPR band for this view.
    pub fn spr_band(&self, obs: &Observables<'_>) -> u8 {
        spr_band_index(&self.cfg.spr_bands, obs.spr())
    }

    /// Our bucket: preflop = 169-class id; flop/turn = orbit table (fallback =
    /// strength quantile, D-006); river = exact-equity quantile × texture.
    pub fn bucket(&mut self, obs: &Observables<'_>) -> u16 {
        let board: Vec<cham_core::card::Card> = obs.board[..obs.board_len as usize].to_vec();
        match obs.street {
            Street::Preflop => preflop_bucket(obs.hole),
            Street::Flop | Street::Turn => {
                let table = match obs.street {
                    Street::Flop => &self.flop,
                    _ => &self.turn,
                };
                match table {
                    Some(t) => {
                        let b = orbit_bucket(t, obs.hole, &board);
                        if b == crate::tables::MISS_SENTINEL {
                            self.fallback_bucket(obs, &board)
                        } else {
                            b
                        }
                    }
                    None => self.fallback_bucket(obs, &board),
                }
            }
            Street::River => {
                let key = seq_pack(obs.hole, &board);
                if let Some(&b) = self.eq_cache.get(&key) {
                    return b;
                }
                let mut board5 = [cham_core::card::Card(0); 5];
                board5[..board.len()].copy_from_slice(&board);
                let eq = crate::tables::river_equity(obs.hole, &board5);
                let b = self.river.bucket(eq, &board);
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
    pub fn slots(&self, obs: &Observables<'_>, seq: &ActionSeq) -> arrayvec::ArrayVec<crate::ladder::AbstractAction, 12> {
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
        let mut mask: u16 = 0;
        for (i, s) in slots.iter().enumerate() {
            if cham_core::obs::is_legal(obs, s.action) {
                mask |= 1 << i;
            }
        }
        let bucket = self.bucket(obs);
        let mut bytes: Vec<u8> = Vec::with_capacity(16 + 48);
        bytes.push(obs.street.as_u8());
        bytes.push(obs.player.as_usize() as u8);
        bytes.push(self.spr_band(obs));
        bytes.extend_from_slice(&bucket.to_le_bytes());
        bytes.push(self.belief_bin); // ExploitBayes bin (0 when unused)
        bytes.extend_from_slice(&mask.to_le_bytes());
        for street in 0..4u8 {
            let n = seq.lens[street as usize];
            bytes.push(n);
            for i in 0..n as usize {
                let e = &seq.entries[street as usize * 8 + i];
                bytes.push(e.actor);
                bytes.push(e.class.as_u8());
                bytes.push(e.size_bucket);
            }
        }
        InfoSetKey(fnv1a(&bytes) | (1 << 63))
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
    if f.exists() {
        Some(f)
    } else {
        None
    }
}

/// Pack (hole, board) for the equity cache key.
fn seq_pack(hole: Hand2, board: &[cham_core::card::Card]) -> u64 {
    let mut v = hole.0 as u64;
    for (i, c) in board.iter().enumerate() {
        v |= (c.idx() as u64) << (16 + 8 * i);
    }
    v
}

/// Equal-mass quantile edges in [0,1] (runtime default when no meta is present).
pub fn equal_mass_edges(bins: usize) -> Vec<f64> {
    (0..=bins).map(|i| i as f64 / bins as f64).collect()
}
