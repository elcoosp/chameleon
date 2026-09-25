//! `RegretTable` (SPECS/04 §2): open-addressing arena keyed by `InfoSetKey`.
//!
//! Row layout (u32 slots): `[f32 regret × W][f32 strat_sum × W][f32 avg_weight][u32 visits]`
//! = 2W + 2 slots. ONE row per infoset in ALL modes (the key carries the position bit).
//!
//! Two thread modes behind one API (SPECS/00 §3.5): `Deterministic` runs
//! single-threaded with bit-identical results; `Hogwild` performs relaxed CAS-adds
//! of float bit patterns — interleaving-dependent by design, safe under
//! `#![forbid(unsafe_code)]` (atomics, never aliased `&mut`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::BlueprintError;

/// Threading mode recorded in provenance (SPECS/00 §3.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadMode {
    Deterministic,
    Hogwild,
}

#[derive(Clone, Copy)]
struct Slot {
    key: u64, // 0 = empty
    off: u32, // arena offset in u32 units
    w: u8,    // row width (legal-mask popcount), fixed at insert (invariant I8)
}

struct Arena {
    cells: Vec<std::sync::atomic::AtomicU32>,
}

impl Arena {
    fn len(&self) -> usize {
        self.cells.len()
    }
}

impl Arena {
    fn with_capacity(n: usize) -> Arena {
        Arena { cells: (0..n).map(|_| std::sync::atomic::AtomicU32::new(0)).collect() }
    }
    #[inline]
    fn load(&self, i: usize) -> u32 {
        use std::sync::atomic::Ordering::*;
        self.cells[i].load(Relaxed)
    }
    #[inline]
    fn store(&self, i: usize, v: u32) {
        use std::sync::atomic::Ordering::*;
        self.cells[i].store(v, Relaxed);
    }
    /// CAS-add a float bit pattern (Hogwild-safe; single-threaded = sequential).
    #[inline]
    fn add_f32(&self, i: usize, delta: f32) {
        use std::sync::atomic::Ordering::*;
        let cell = &self.cells[i];
        let mut cur = cell.load(Relaxed);
        loop {
            let new_val = f32::from_bits(cur) + delta;
            match cell.compare_exchange_weak(cur, new_val.to_bits(), Relaxed, Relaxed) {
                Ok(_) => return,
                Err(observed) => cur = observed,
            }
        }
    }
    #[inline]
    fn fetch_add(&self, i: usize, v: u32) {
        use std::sync::atomic::Ordering::*;
        self.cells[i].fetch_add(v, Relaxed);
    }
}

/// Open-addressing regret table. Rows are variable-width (W = popcount of the
/// infoset's legal mask, ≤ 12) and stored in a flat atomic arena.
pub struct RegretTable {
    slots: Vec<Slot>,
    mask: usize,
    n: usize,
    arena: Arena,
    arena_len: u32,
    pub mode: ThreadMode,
    /// rows scaled at snapshot time (f32 growth guard, review A8)
    pub renorm_events: u64,
}

const ROW_META: u32 = 2; // avg_weight + visits

#[inline]
fn hash_key(key: u64) -> usize {
    let mut z = key.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z ^= z >> 31;
    z as usize
}

impl RegretTable {
    pub fn new(mode: ThreadMode) -> RegretTable {
        RegretTable::with_capacity(mode, 1024)
    }

    pub fn with_capacity(mode: ThreadMode, cap: usize) -> RegretTable {
        let slots_cap = cap.next_power_of_two();
        RegretTable {
            slots: vec![Slot { key: 0, off: 0, w: 0 }; slots_cap],
            mask: slots_cap - 1,
            n: 0,
            arena: Arena::with_capacity(slots_cap * 4),
            arena_len: 0,
            mode,
            renorm_events: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Lookup a row offset; None if absent.
    #[inline]
    pub fn find(&self, key: u64) -> Option<u32> {
        if key == 0 {
            return None;
        }
        let mut i = hash_key(key) & self.mask;
        loop {
            let s = self.slots[i];
            if s.key == key {
                return Some(s.off);
            }
            if s.key == 0 {
                return None;
            }
            i = (i + 1) & self.mask;
        }
    }

    /// Find-or-insert a row of width W. Returns (offset, W).
    pub fn entry_or_insert(&mut self, key: u64, w: usize) -> (u32, usize) {
        debug_assert!((1..=12).contains(&w), "row width W ∈ [1,12], got {w}");
        if let Some(off) = self.find(key) {
            return (off, w);
        }
        // insert (grow if load ≥ 0.70)
        if (self.n + 1) as f64 / (self.mask + 1) as f64 > 0.70 {
            self.grow();
        }
        let row = (2 * w + ROW_META as usize) as u32;
        let off = self.arena_len;
        self.arena_len += row;
        if self.arena.len() < self.arena_len as usize {
            self.arena.cells.reserve(self.arena_len as usize - self.arena.len());
        }
        while self.arena.len() < self.arena_len as usize {
            self.arena.cells.push(std::sync::atomic::AtomicU32::new(0));
        }
        let mut i = hash_key(key) & self.mask;
        while self.slots[i].key != 0 {
            i = (i + 1) & self.mask;
        }
        self.slots[i] = Slot { key, off, w: w as u8 };
        self.n += 1;
        (off, w)
    }

    fn grow(&mut self) {
        let new_cap = (self.mask + 1) * 2;
        let mut slots = vec![Slot { key: 0, off: 0, w: 0 }; new_cap];
        let mask = new_cap - 1;
        for s in &self.slots {
            if s.key != 0 {
                let mut i = hash_key(s.key) & mask;
                while slots[i].key != 0 {
                    i = (i + 1) & mask;
                }
                slots[i] = *s;
            }
        }
        self.slots = slots;
        self.mask = mask;
    }

    // ---------- row accessors (offset-based; caller keeps W) ----------

    #[inline]
    fn slot_of(&self, off: u32, idx: usize) -> usize {
        off as usize + idx
    }

    pub fn regret(&self, off: u32, _w: usize, a: usize) -> f32 {
        f32::from_bits(self.arena.load(self.slot_of(off, a)))
    }

    pub fn regret_add(&self, off: u32, a: usize, delta: f32) {
        self.arena.add_f32(self.slot_of(off, a), delta);
    }

    /// CFR+ / regret-matching+ update: `R ← max(R + Δ, 0)` (floored at zero).
    pub fn regret_add_cfr_plus(&self, off: u32, a: usize, delta: f32) {
        use std::sync::atomic::Ordering::*;
        let cell = &self.arena.cells[self.slot_of(off, a)];
        let mut cur = cell.load(Relaxed);
        loop {
            let new_val = (f32::from_bits(cur) + delta).max(0.0);
            match cell.compare_exchange_weak(cur, new_val.to_bits(), Relaxed, Relaxed) {
                Ok(_) => return,
                Err(observed) => cur = observed,
            }
        }
    }

    pub fn strat(&self, off: u32, w: usize, a: usize) -> f32 {
        f32::from_bits(self.arena.load(self.slot_of(off, w + a)))
    }

    pub fn strat_add(&self, off: u32, w: usize, a: usize, delta: f32) {
        self.arena.add_f32(self.slot_of(off, w + a), delta);
    }

    pub fn avg_weight(&self, off: u32, w: usize) -> f32 {
        f32::from_bits(self.arena.load(self.slot_of(off, 2 * w)))
    }

    pub fn add_weight(&self, off: u32, w: usize, delta: f32) {
        self.arena.add_f32(self.slot_of(off, 2 * w), delta);
    }

    pub fn visits(&self, off: u32, w: usize) -> u32 {
        self.arena.load(self.slot_of(off, 2 * w + 1))
    }

    pub fn add_visit(&self, off: u32, w: usize) {
        self.arena.fetch_add(self.slot_of(off, 2 * w + 1), 1);
    }

    /// Regret-matching+ strategy: σ(a) ∝ max(R_a, 0); uniform if all ≤ 0.
    pub fn sigma_rms(&self, off: u32, w: usize) -> Vec<f64> {
        let mut pos = [0f64; 12];
        let mut total = 0.0;
        for a in 0..w {
            let r = self.regret(off, w, a) as f64;
            if r > 0.0 {
                pos[a] = r;
                total += r;
            }
        }
        if total <= 0.0 {
            return vec![1.0 / w as f64; w];
        }
        (0..w).map(|a| pos[a] / total).collect()
    }

    /// Current strategy from accumulated strat sums (for snapshots/inspection).
    pub fn avg_strategy(&self, off: u32, w: usize) -> Vec<f64> {
        let total: f32 = (0..w).map(|a| self.strat(off, w, a)).sum();
        if total <= 0.0 {
            return vec![1.0 / w as f64; w];
        }
        (0..w).map(|a| (self.strat(off, w, a) / total) as f64).collect()
    }

    /// f32 growth guard (SPECS/04 §2): scale strat_sum + avg_weight by 2^-k when
    /// either exceeds 2^22, bringing the max under 2^20. Lossless for normalized
    /// strategies.
    pub fn renorm_row(&mut self, off: u32, w: usize) -> bool {
        let mut max_abs: f32 = 0.0;
        for a in 0..w {
            max_abs = max_abs.max(self.strat(off, w, a).abs());
        }
        max_abs = max_abs.max(self.avg_weight(off, w).abs());
        if max_abs <= 4_194_304.0 {
            // 2^22
            return false;
        }
        let mut k = 0u32;
        let mut m = max_abs;
        while m > 1_048_576.0 {
            // 2^20
            m /= 2.0;
            k += 1;
        }
        for a in 0..w {
            let cur = self.strat(off, w, a);
            self.arena.store(self.slot_of(off, w + a), (cur / 2f32.powi(k as i32)).to_bits());
        }
        let wgt = self.avg_weight(off, w);
        self.arena.store(self.slot_of(off, 2 * w), (wgt / 2f32.powi(k as i32)).to_bits());
        self.renorm_events += 1;
        true
    }

    /// Iterate all (key, off) pairs in slot order (for snapshots / warm-start).
    pub fn iter(&self) -> impl Iterator<Item = (u64, u32)> + '_ {
        self.slots.iter().filter(|s| s.key != 0).map(|s| (s.key, s.off))
    }

    /// Row width for a stored key (None if absent).
    pub fn width_of(&self, key: u64) -> Option<usize> {
        self.slot_w(key)
    }

    /// Row width at an offset (linear scan — callers keep W; used by renorm passes).
    pub fn row_width(&self, off: u32) -> usize {
        for s in &self.slots {
            if s.key != 0 && s.off == off {
                return s.w as usize;
            }
        }
        2
    }

    fn slot_w(&self, key: u64) -> Option<usize> {
        let mut i = hash_key(key) & self.mask;
        loop {
            let s = self.slots[i];
            if s.key == key {
                return Some(s.w as usize);
            }
            if s.key == 0 {
                return None;
            }
            i = (i + 1) & self.mask;
        }
    }

    pub fn arena_len(&self) -> u32 {
        self.arena_len
    }

    // ---------- snapshots (bincode + zstd, tmp+rename) ----------

    /// Serialize: every slot carries its width W (invariant I8 bookkeeping).
    pub fn snapshot(&self) -> Vec<u8> {
        #[derive(Serialize, Deserialize)]
        struct Snap {
            n: usize,
            mode: ThreadMode,
            rows: Vec<(u64, u8, Vec<u32>)>, // key, w, cells
        }
        let mut rows = Vec::with_capacity(self.n);
        for s in &self.slots {
            if s.key == 0 {
                continue;
            }
            let w = s.w as usize;
            let mut cells = Vec::with_capacity(2 * w + 2);
            for i in 0..2 * w + 2 {
                cells.push(self.arena.load(s.off as usize + i));
            }
            rows.push((s.key, s.w, cells));
        }
        rows.sort_by_key(|r| r.0);
        let snap = Snap { n: self.n, mode: self.mode, rows };
        let raw = bincode::serialize(&snap).unwrap_or_default();
        zstd::bulk::compress(&raw, 3).unwrap_or_default()
    }

    /// Restore a snapshot (replaces contents).
    pub fn restore(&mut self, bytes: &[u8]) -> Result<(), BlueprintError> {
        #[derive(Serialize, Deserialize)]
        struct Snap {
            n: usize,
            mode: ThreadMode,
            rows: Vec<(u64, u8, Vec<u32>)>,
        }
        let raw = zstd::bulk::decompress(bytes, 1 << 30)
            .map_err(|e| BlueprintError::Table(format!("zstd: {e}")))?;
        let snap: Snap = bincode::deserialize(&raw)
            .map_err(|e| BlueprintError::Table(format!("bincode: {e}")))?;
        let slots_cap = (snap.n.max(16) * 2).next_power_of_two();
        self.slots = vec![Slot { key: 0, off: 0, w: 0 }; slots_cap];
        self.mask = slots_cap - 1;
        self.n = 0;
        self.arena = Arena::with_capacity(slots_cap * 4);
        self.arena_len = 0;
        self.mode = snap.mode;
        for (key, w, cells) in snap.rows {
            let (off, w_out) = self.entry_or_insert(key, w as usize);
            debug_assert_eq!(w_out, w as usize);
            for (i, c) in cells.iter().enumerate() {
                self.arena.store(off as usize + i, *c);
            }
        }
        Ok(())
    }

    /// Write a snapshot atomically (tmp + rename).
    pub fn save_to(&self, path: &Path) -> Result<PathBuf, BlueprintError> {
        let bytes = self.snapshot();
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, path)?;
        Ok(path.to_path_buf())
    }

    pub fn load_from(path: &Path) -> Result<RegretTable, BlueprintError> {
        let bytes = std::fs::read(path)?;
        let mut t = RegretTable::new(ThreadMode::Deterministic);
        t.restore(&bytes)?;
        Ok(t)
    }
}
