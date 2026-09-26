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

/// Snapshot format version. Bump on any change to the `Snap` layout or the
/// serializer. v1 was bincode; v2 (current) is postcard. Old files are
/// rejected by `restore` with a clear message — regenerate with train-bp.
const SNAP_VERSION: u8 = 2;

/// Threading mode recorded in provenance (SPECS/00 §3.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadMode {
    Deterministic,
    Hogwild,
    /// Batched Hogwild-class updates (PERF-PLAN T3): each worker accumulates
    /// regret/strategy/weight/visit deltas in a thread-local [`DeltaBuffer`]
    /// and flushes one atomic op per slot. Plain adds are associative, so
    /// results stay statistically identical; single-threaded `Deterministic`
    /// math is untouched (bit-exact).
    Snapbatch,
}

/// Thread-local snapbatch delta buffer (PERF-PLAN T3).
///
/// Workers push per-visit `(slot, delta)` pairs with zero atomics; flush
/// sorts each lane by slot, sums runs, and applies ONE atomic op per slot
/// (`fetch_add` for strategy/weight/visits, one CFR+ CAS for regrets).
/// Pre-reserved at 4096 entries; flushes when full or every
/// [`DeltaBuffer::FLUSH_EVERY`] traversals (K = 64 default).
#[derive(Clone, Debug, Default)]
pub struct DeltaBuffer {
    /// packed key `(off << 8) | (w << 4) | a`, delta
    regrets: Vec<(u64, f32)>,
    strats: Vec<(u64, f32)>,
    /// packed key `(off << 8) | w`, delta
    weights: Vec<(u64, f32)>,
    /// packed key `(off << 8) | w`
    visits: Vec<u64>,
    traversals_since_flush: u32,
}

impl DeltaBuffer {
    /// Flush cadence in traversals (K = 64 default).
    pub const FLUSH_EVERY: u32 = 64;
    /// Buffer capacity per lane before a forced flush.
    pub const CAP: usize = 4096;

    pub fn new() -> DeltaBuffer {
        DeltaBuffer {
            regrets: Vec::with_capacity(Self::CAP),
            strats: Vec::with_capacity(Self::CAP),
            weights: Vec::with_capacity(1024),
            visits: Vec::with_capacity(1024),
            traversals_since_flush: 0,
        }
    }

    #[inline]
    fn regret_key(off: u32, a: usize) -> u64 {
        ((off as u64) << 8) | (a as u64 & 0xff)
    }

    #[inline]
    fn row_key(off: u32, w: usize) -> u64 {
        ((off as u64) << 8) | (w as u64 & 0xff)
    }

    #[inline]
    fn strat_key(off: u32, w: usize, a: usize) -> u64 {
        ((off as u64) << 12) | ((w as u64 & 0xf) << 8) | (a as u64 & 0xff)
    }

    pub fn push_regret(&mut self, off: u32, a: usize, delta: f32) {
        self.regrets.push((Self::regret_key(off, a), delta));
    }

    pub fn push_strat(&mut self, off: u32, w: usize, a: usize, delta: f32) {
        self.strats.push((Self::strat_key(off, w, a), delta));
    }

    pub fn push_weight(&mut self, off: u32, w: usize, delta: f32) {
        self.weights.push((Self::row_key(off, w), delta));
    }

    pub fn push_visit(&mut self, off: u32, w: usize) {
        self.visits.push(Self::row_key(off, w));
    }

    pub fn len(&self) -> usize {
        self.regrets.len() + self.strats.len() + self.weights.len() + self.visits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn regrets_full(&self) -> bool {
        self.regrets.len() >= Self::CAP
    }

    pub fn strats_full(&self) -> bool {
        self.strats.len() >= Self::CAP
    }

    /// Record one finished traversal; returns true when the buffer should be
    /// flushed (full, or K traversals since the last flush).
    pub fn note_traversal(&mut self) -> bool {
        self.traversals_since_flush += 1;
        self.regrets.len() >= Self::CAP
            || self.strats.len() >= Self::CAP
            || self.traversals_since_flush >= Self::FLUSH_EVERY
    }

    /// Aggregate per slot and apply one atomic op per slot, then clear.
    pub fn flush(&mut self, table: &RegretTable) {
        if self.regrets.is_empty()
            && self.strats.is_empty()
            && self.weights.is_empty()
            && self.visits.is_empty()
        {
            self.traversals_since_flush = 0;
            return;
        }
        // CFR+ regrets: sum per slot, one floored CAS per slot.
        self.regrets.sort_by_key(|&(k, _)| k);
        let mut i = 0;
        while i < self.regrets.len() {
            let k = self.regrets[i].0;
            let mut sum = 0.0f32;
            while i < self.regrets.len() && self.regrets[i].0 == k {
                sum += self.regrets[i].1;
                i += 1;
            }
            let slot = (k >> 8) as usize + (k & 0xff) as usize;
            table.regret_add_cfr_plus_slot(slot, sum);
        }
        // Strategy sums: plain associative adds, one per slot.
        Self::flush_pairs(&mut self.strats, &mut |key, sum| {
            let slot = (key >> 12) as usize + ((key >> 8) & 0xf) as usize + (key & 0xff) as usize;
            table.add_f32_slot(slot, sum);
        });
        // Average weights: one per row.
        Self::flush_pairs(&mut self.weights, &mut |key, sum| {
            let off = (key >> 8) as u32;
            let w = (key & 0xff) as usize;
            table.add_weight(off, w, sum);
        });
        // Visits: counted runs, one fetch_add per row.
        self.visits.sort_unstable();
        let mut j = 0;
        while j < self.visits.len() {
            let k = self.visits[j];
            let mut n = 0u32;
            while j < self.visits.len() && self.visits[j] == k {
                n += 1;
                j += 1;
            }
            table.add_visits((k >> 8) as u32, (k & 0xff) as usize, n);
        }
        self.clear();
    }

    /// Same as [`flush`] but applies DCFR regret discount per merged slot
    /// (`new = max(0, old*discount + sum)`). `discount == 1.0` reproduces
    /// the exact CFR+ path.
    pub fn flush_with_discount(&mut self, table: &RegretTable, discount: f32) {
        if discount >= 1.0 {
            return self.flush(table);
        }
        if self.regrets.is_empty()
            && self.strats.is_empty()
            && self.weights.is_empty()
            && self.visits.is_empty()
        {
            self.traversals_since_flush = 0;
            return;
        }
        self.regrets.sort_by_key(|&(k, _)| k);
        let mut i = 0;
        while i < self.regrets.len() {
            let k = self.regrets[i].0;
            let mut sum = 0.0f32;
            while i < self.regrets.len() && self.regrets[i].0 == k {
                sum += self.regrets[i].1;
                i += 1;
            }
            let slot = (k >> 8) as usize + (k & 0xff) as usize;
            table.regret_add_cfr_plus_discounted_slot(slot, sum, discount);
        }
        Self::flush_pairs(&mut self.strats, &mut |key, sum| {
            let slot = (key >> 12) as usize + ((key >> 8) & 0xf) as usize + (key & 0xff) as usize;
            table.add_f32_slot(slot, sum);
        });
        Self::flush_pairs(&mut self.weights, &mut |key, sum| {
            let off = (key >> 8) as u32;
            let w = (key & 0xff) as usize;
            table.add_weight(off, w, sum);
        });
        self.visits.sort_unstable();
        let mut j = 0;
        while j < self.visits.len() {
            let k = self.visits[j];
            let mut n = 0u32;
            while j < self.visits.len() && self.visits[j] == k {
                n += 1;
                j += 1;
            }
            table.add_visits((k >> 8) as u32, (k & 0xff) as usize, n);
        }
        self.clear();
    }

    fn flush_pairs(lane: &mut [(u64, f32)], apply: &mut impl FnMut(u64, f32)) {
        lane.sort_by_key(|a| a.0);
        let mut i = 0;
        while i < lane.len() {
            let k = lane[i].0;
            let mut sum = 0.0f32;
            while i < lane.len() && lane[i].0 == k {
                sum += lane[i].1;
                i += 1;
            }
            apply(k, sum);
        }
    }

    pub fn clear(&mut self) {
        self.regrets.clear();
        self.strats.clear();
        self.weights.clear();
        self.visits.clear();
        self.traversals_since_flush = 0;
    }
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
        Arena {
            cells: (0..n)
                .map(|_| std::sync::atomic::AtomicU32::new(0))
                .collect(),
        }
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

/// Second hash for double-hash probing (v3 §3.2). Returns an ODD step so the
/// probe sequence `(h1 + k·step) mod 2^m` is a full cycle over power-of-two
/// tables for every key — primary clustering (the unbounded worst-case chain
/// of linear probing near load 0.7) is gone by construction, with no new
/// dependency: `hashbrown` is NOT in the closed workspace whitelist
/// (SPECS/00 §2 — adding it needs a human decision), so this is the
/// no-new-trust-surface equivalent. The `hashbrown::raw::RawTable` swap from
/// the roadmap stays the documented escalation path behind whitelist
/// amendment + the `benches/mccfr.rs` kill-criterion measurement.
#[inline]
fn hash_step(key: u64, mask: usize) -> usize {
    // Independent mix (different constant + rotation from hash_key), forced
    // odd ⇒ coprime to 2^m ⇒ full-cycle probe for every key. `| 1` also
    // guarantees nonzero, so progress is unconditional.
    let mut z = key.wrapping_mul(0xC2B2_AE35_27D4_EB4F).rotate_left(29);
    z ^= z >> 27;
    ((z as usize) & mask) | 1
}

impl RegretTable {
    pub fn new(mode: ThreadMode) -> RegretTable {
        RegretTable::with_capacity(mode, 1024)
    }

    pub fn with_capacity(mode: ThreadMode, cap: usize) -> RegretTable {
        let slots_cap = cap.next_power_of_two();
        RegretTable {
            slots: vec![
                Slot {
                    key: 0,
                    off: 0,
                    w: 0
                };
                slots_cap
            ],
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

    /// Lookup a row offset; None if absent (double-hash probe, v3 §3.2).
    #[inline]
    pub fn find(&self, key: u64) -> Option<u32> {
        if key == 0 {
            return None;
        }
        let step = hash_step(key, self.mask);
        let mut i = hash_key(key) & self.mask;
        loop {
            let s = self.slots[i];
            if s.key == key {
                return Some(s.off);
            }
            if s.key == 0 {
                return None;
            }
            i = (i + step) & self.mask;
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
            self.arena
                .cells
                .reserve(self.arena_len as usize - self.arena.len());
        }
        while self.arena.len() < self.arena_len as usize {
            self.arena.cells.push(std::sync::atomic::AtomicU32::new(0));
        }
        let step = hash_step(key, self.mask);
        let mut i = hash_key(key) & self.mask;
        while self.slots[i].key != 0 {
            i = (i + step) & self.mask;
        }
        self.slots[i] = Slot {
            key,
            off,
            w: w as u8,
        };
        self.n += 1;
        (off, w)
    }

    fn grow(&mut self) {
        let new_cap = (self.mask + 1) * 2;
        let mut slots = vec![
            Slot {
                key: 0,
                off: 0,
                w: 0
            };
            new_cap
        ];
        let mask = new_cap - 1;
        for s in &self.slots {
            if s.key != 0 {
                let step = hash_step(s.key, mask);
                let mut i = hash_key(s.key) & mask;
                while slots[i].key != 0 {
                    i = (i + step) & mask;
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

    /// Read the accumulated strategy-sum for slot `a` of row `off`.
    /// Mirrors `avg_weight` / `regret` — pure getter, no side effects.
    /// Exposed for the external-sampling audit (`strat_sum` has no reach
    /// factor by SPECS/04 §4; the test asserts exact deltas).
    pub fn strat_sum(&self, off: u32, w: usize, a: usize) -> f32 {
        f32::from_bits(self.arena.load(self.slot_of(off, w + a)))
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

    /// Batched visit adds (snapbatch flush path): one `fetch_add` per slot.
    pub fn add_visits(&self, off: u32, w: usize, n: u32) {
        if n > 0 {
            self.arena.fetch_add(self.slot_of(off, 2 * w + 1), n);
        }
    }

    /// Batched CFR+ regret add at an absolute arena slot (snapbatch flush path).
    /// DCFR-style discounting: `new = max(0, old * discount + delta)`.
    /// Same CAS shape as `regret_add_cfr_plus`; `discount == 1.0` gives the
    /// identical formula. Used only when the trainer is configured with
    /// `regret_discount < 1.0`.
    pub fn regret_add_cfr_plus_discounted(&self, off: u32, a: usize, delta: f32, discount: f32) {
        use std::sync::atomic::Ordering::*;
        let cell = &self.arena.cells[self.slot_of(off, a)];
        let mut cur = cell.load(Relaxed);
        loop {
            let new_val = (f32::from_bits(cur) * discount + delta).max(0.0);
            match cell.compare_exchange_weak(cur, new_val.to_bits(), Relaxed, Relaxed) {
                Ok(_) => return,
                Err(observed) => cur = observed,
            }
        }
    }
    /// Discounted slot-based variant used by `DeltaBuffer::flush_with_discount`.
    pub fn regret_add_cfr_plus_discounted_slot(&self, slot: usize, delta: f32, discount: f32) {
        use std::sync::atomic::Ordering::*;
        let cell = &self.arena.cells[slot];
        let mut cur = cell.load(Relaxed);
        loop {
            let new_val = (f32::from_bits(cur) * discount + delta).max(0.0);
            match cell.compare_exchange_weak(cur, new_val.to_bits(), Relaxed, Relaxed) {
                Ok(_) => return,
                Err(observed) => cur = observed,
            }
        }
    }

    pub fn regret_add_cfr_plus_slot(&self, slot: usize, delta: f32) {
        use std::sync::atomic::Ordering::*;
        let cell = &self.arena.cells[slot];
        let mut cur = cell.load(Relaxed);
        loop {
            let new_val = (f32::from_bits(cur) + delta).max(0.0);
            match cell.compare_exchange_weak(cur, new_val.to_bits(), Relaxed, Relaxed) {
                Ok(_) => return,
                Err(observed) => cur = observed,
            }
        }
    }

    /// Batched plain float add at an absolute arena slot (snapbatch flush path).
    pub fn add_f32_slot(&self, slot: usize, delta: f32) {
        self.arena.add_f32(slot, delta);
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
        (0..w)
            .map(|a| (self.strat(off, w, a) / total) as f64)
            .collect()
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
            self.arena.store(
                self.slot_of(off, w + a),
                (cur / 2f32.powi(k as i32)).to_bits(),
            );
        }
        let wgt = self.avg_weight(off, w);
        self.arena.store(
            self.slot_of(off, 2 * w),
            (wgt / 2f32.powi(k as i32)).to_bits(),
        );
        self.renorm_events += 1;
        true
    }

    /// Iterate all (key, off) pairs in slot order (for snapshots / warm-start).
    pub fn iter(&self) -> impl Iterator<Item = (u64, u32)> + '_ {
        self.slots
            .iter()
            .filter(|s| s.key != 0)
            .map(|s| (s.key, s.off))
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

    // ---------- snapshots (postcard + zstd, tmp+rename) ----------

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
        let snap = Snap {
            n: self.n,
            mode: self.mode,
            rows,
        };
        let raw = postcard::to_allocvec(&snap).unwrap_or_default();
        let body = zstd::bulk::compress(&raw, 3).unwrap_or_default();
        let mut out = Vec::with_capacity(body.len() + 1);
        out.push(SNAP_VERSION);
        out.extend_from_slice(&body);
        out
    }

    /// Restore a snapshot (replaces contents).
    pub fn restore(&mut self, bytes: &[u8]) -> Result<(), BlueprintError> {
        #[derive(Serialize, Deserialize)]
        struct Snap {
            n: usize,
            mode: ThreadMode,
            rows: Vec<(u64, u8, Vec<u32>)>,
        }
        let (version, body) = bytes
            .split_first()
            .ok_or_else(|| BlueprintError::Table("empty snapshot".into()))?;
        if *version != SNAP_VERSION {
            return Err(BlueprintError::Table(format!(
                "snapshot v{version} unsupported (current v{SNAP_VERSION}); regenerate with train-bp"
            )));
        }
        let raw = zstd::bulk::decompress(body, 1 << 30)
            .map_err(|e| BlueprintError::Table(format!("zstd: {e}")))?;
        let snap: Snap = postcard::from_bytes(&raw)
            .map_err(|e| BlueprintError::Table(format!("postcard: {e}")))?;
        let slots_cap = (snap.n.max(16) * 2).next_power_of_two();
        self.slots = vec![
            Slot {
                key: 0,
                off: 0,
                w: 0
            };
            slots_cap
        ];
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

#[cfg(test)]
mod probe_tests {
    use super::*;

    #[test]
    fn hash_step_is_always_odd_and_nonzero() {
        for k in [0u64, 1, 2, 0xFFFF_FFFF_FFFF_FFFF, 0x9E37_79B9_7F4A_7C15] {
            for mask in [15usize, 1023, 65535] {
                let s = hash_step(k, mask);
                assert!(s & 1 == 1, "step must be odd (full cycle on 2^m)");
                assert!(s != 0, "step must be nonzero (progress)");
            }
        }
    }

    #[test]
    fn double_hash_find_after_grows() {
        // 5000 inserts past several 0.70-load grows; every key must resolve
        // to its own offset with its width intact (probing change is
        // behavior-preserving by construction).
        let mut t = RegretTable::with_capacity(ThreadMode::Deterministic, 16);
        let mut want: Vec<(u64, u32, usize)> = Vec::new();
        for k in 1..=5000u64 {
            let key = k.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(7);
            let key = if key == 0 { 1 } else { key };
            let w = (k % 12 + 1) as usize;
            let (off, w_out) = t.entry_or_insert(key, w);
            assert_eq!(w_out, w);
            want.push((key, off, w));
        }
        assert_eq!(t.len(), 5000);
        for (key, off, w) in &want {
            assert_eq!(t.find(*key), Some(*off), "key {key} must resolve");
            // re-insert is a pure hit (no dup row, same offset)
            assert_eq!(t.entry_or_insert(*key, *w).0, *off);
        }
        assert_eq!(t.find(0), None, "key 0 is reserved-empty");
    }
}
