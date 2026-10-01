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
// H-9 fix (2026-09-27): bumped 2 → 3 for the `last_iter` field. Snapshots
// from earlier builds must be regenerated (they encode neither the resume
// position nor anything else that lets a resumed run continue the RNG
// bitstream). `restore` rejects v2 with a clear message.
const SNAP_VERSION: u8 = 4;

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
            table.add_f64_slot(slot, sum as f64);
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
            table.add_f64_slot(slot, sum as f64);
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

/// F4 (2026-10-01, competitiveness report): f64 sibling of [`Arena`] for
/// the strategy sum and the averaging weight. The previous f32-only path
/// lost precision past 2^22; the snapshot renorm "fixed" that by scaling
/// rows, which the report showed biases the average toward whatever came
/// *after* the scaling. f64 removes the ceiling.
struct Arena64 {
    cells: Vec<std::sync::atomic::AtomicU64>,
}

impl Arena64 {
    fn with_capacity(n: usize) -> Arena64 {
        Arena64 {
            cells: (0..n)
                .map(|_| std::sync::atomic::AtomicU64::new(0))
                .collect(),
        }
    }
    #[inline]
    fn load(&self, i: usize) -> f64 {
        use std::sync::atomic::Ordering::*;
        f64::from_bits(self.cells[i].load(Relaxed))
    }
    #[inline]
    #[allow(dead_code)]
    fn store(&self, i: usize, v: f64) {
        use std::sync::atomic::Ordering::*;
        self.cells[i].store(v.to_bits(), Relaxed);
    }
    /// CAS-add an f64 (Hogwild-safe; single-threaded = sequential).
    #[inline]
    fn add_f64(&self, i: usize, delta: f64) {
        use std::sync::atomic::Ordering::*;
        let cell = &self.cells[i];
        let mut cur = cell.load(Relaxed);
        loop {
            let new_val = f64::from_bits(cur) + delta;
            match cell.compare_exchange_weak(cur, new_val.to_bits(), Relaxed, Relaxed) {
                Ok(_) => return,
                Err(observed) => cur = observed,
            }
        }
    }
}

/// Open-addressing regret table. Rows are variable-width (W = popcount of the
/// infoset's legal mask, ≤ 12) and stored in a flat atomic arena.
pub struct RegretTable {
    slots: Vec<Slot>,
    mask: usize,
    n: usize,
    arena: Arena,
    /// F4 (2026-10-01): f64 sibling arena for strategy sum + weight cells.
    /// Same slot indexing as `arena`; the regret + visit cells stay f32/u32.
    arena64: Arena64,
    arena_len: u32,
    pub mode: ThreadMode,
    /// rows scaled at snapshot time (f32 growth guard, review A8)
    pub renorm_events: u64,
    /// H-9 fix (2026-09-27): the global iteration index this table has been
    /// trained through. 0 = fresh. Recorded in the snapshot so a resumed
    /// run can continue the RNG bitstream from the correct position instead
    /// of replaying iterations 0..N on top of the restored table (the
    /// pre-fix behaviour — the documented `train 100 + resume 100 ==
    /// train 200` contract was unimplementable).
    last_iter: u64,
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

/// Process-wide training exploration floor (2026-09-29).
///
/// Read by [`RegretTable::sigma_rms`] on every call. Set once at startup
/// from the `CHAM_TRAIN_EPS` env var by the trainer (or by tests). The
/// default is 0.0, which is bit-identical to the pre-2026-09-29 behavior.
///
/// Stored as `AtomicU64` holding an `f64::to_bits` value so it can be
/// read from any thread without a lock. Fraction of probability forced
/// uniform in regret matching+.
///
/// See docs/plans/RM-PLUS-FREEZE-2026-09-29.md.
static TRAIN_EXPLORE_EPS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0.0f64.to_bits());

/// Set the process-wide training exploration floor. Clamped to [0, 0.5).
pub fn set_train_explore_eps(eps: f64) {
    let clamped = eps.clamp(0.0, 0.5);
    TRAIN_EXPLORE_EPS.store(clamped.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Current process-wide training exploration floor.
pub fn train_explore_eps() -> f64 {
    f64::from_bits(TRAIN_EXPLORE_EPS.load(std::sync::atomic::Ordering::Relaxed))
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
            arena64: Arena64::with_capacity(slots_cap * 4),
            arena_len: 0,
            mode,
            renorm_events: 0,
            last_iter: 0,
        }
    }

    /// H-9: global iteration index this table has been trained through.
    pub fn last_iter(&self) -> u64 {
        self.last_iter
    }

    /// H-9: mark the table as trained through iteration `t`. Called by the
    /// trainer before each snapshot (and once more at exit, so a follow-up
    /// `save_to` records the correct resume position).
    pub fn set_last_iter(&mut self, t: u64) {
        self.last_iter = t;
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
        // F4 (2026-10-01): grow the f64 sibling arena in lockstep. This
        // was the missing piece: the initial patch added `arena64` but
        // only seeded it at construction; every subsequent row insert
        // pushed f32 cells without pushing f64 cells, so the f64 arena
        // ran out of slots at the first reallocation.
        if self.arena64.cells.len() < self.arena_len as usize {
            self.arena64
                .cells
                .reserve(self.arena_len as usize - self.arena64.cells.len());
        }
        while self.arena64.cells.len() < self.arena_len as usize {
            self.arena64
                .cells
                .push(std::sync::atomic::AtomicU64::new(0));
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
        // F4: read from the f64 arena; cast back to f32 for compat with
        // the many callers that expect f32.
        self.arena64.load(self.slot_of(off, w + a)) as f32
    }

    pub fn strat_add(&self, off: u32, w: usize, a: usize, delta: f32) {
        // F4: f64 accumulation, no f32 ceiling.
        self.arena64.add_f64(self.slot_of(off, w + a), delta as f64);
    }

    /// Read the accumulated strategy-sum for slot `a` of row `off`.
    /// Mirrors `avg_weight` / `regret` — pure getter, no side effects.
    /// Exposed for the external-sampling audit (`strat_sum` has no reach
    /// factor by SPECS/04 §4; the test asserts exact deltas).
    pub fn strat_sum(&self, off: u32, w: usize, a: usize) -> f32 {
        // F4: read from the f64 arena (see strat / strat_add).
        self.arena64.load(self.slot_of(off, w + a)) as f32
    }

    pub fn avg_weight(&self, off: u32, w: usize) -> f32 {
        // F4: read from the f64 arena.
        self.arena64.load(self.slot_of(off, 2 * w)) as f32
    }

    pub fn add_weight(&self, off: u32, w: usize, delta: f32) {
        // F4: f64 accumulation.
        self.arena64.add_f64(self.slot_of(off, 2 * w), delta as f64);
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

    /// F4 (2026-10-01): f64 sibling of `add_f32_slot`, used by
    /// `DeltaBuffer::flush` for strategy-sum deltas.
    pub fn add_f64_slot(&self, slot: usize, delta: f64) {
        self.arena64.add_f64(slot, delta);
    }

    /// Regret-matching+ strategy: σ(a) ∝ max(R_a, 0); uniform if all ≤ 0.
    ///
    /// **Deprecated (F9, 2026-10-01):** still consults the process-global
    /// exploration floor for backward compatibility with external callers,
    /// but the trainer's hot path (`Traversal`) now uses the per-traversal
    /// `explore_eps` field. New code should call `sigma_rms_eps(off, w, eps)`
    /// directly.
    pub fn sigma_rms(&self, off: u32, w: usize) -> Vec<f64> {
        self.sigma_rms_eps(off, w, train_explore_eps())
    }

    /// Regret-matching+ strategy with an EXPLORATION FLOOR (2026-09-29).
    ///
    /// Every action gets at least `eps / w` probability mass, and the
    /// remaining `1 - eps` is distributed by regret matching. When `eps`
    /// is 0 this is bit-identical to the original `sigma_rms`.
    ///
    /// Why this exists: RM+ floors regrets at zero, so once the positive
    /// part concentrates on a single action, that action is played with
    /// probability 1 forever and no other action's regret ever rises
    /// above zero again. The policy cannot re-explore. On the tiny
    /// abstraction this happens by ~20M iterations: the fraction of
    /// "soft" rows (max prob < 0.5) drops from 13.5 % at 500k to 2.3 %
    /// at 50M, and the AVERAGE strategy collapses to match the frozen
    /// current iterate (mean max prob 0.45 -> 0.86). Exploitability then
    /// degrades on the seat that needs mixing (BB) while it keeps
    /// improving on the seat that does not (SB).
    ///
    /// The floor keeps every action's regret channel alive, at the cost
    /// of a small amount of intended policy spread. Standard CFR+ on
    /// large games gets this implicitly from sampling noise; here the
    /// training is deterministic per iteration, so the floor is explicit.
    ///
    /// See docs/plans/RM-PLUS-FREEZE-2026-09-29.md.
    pub fn sigma_rms_eps(&self, off: u32, w: usize, eps: f64) -> Vec<f64> {
        let eps = eps.clamp(0.0, 0.99);
        let mut pos = [0f64; 12];
        let mut total = 0.0;
        for a in 0..w {
            let r = self.regret(off, w, a) as f64;
            if r > 0.0 {
                pos[a] = r;
                total += r;
            }
        }
        let floor = eps / w as f64;
        if total <= 0.0 {
            // All-zero regrets: uniform over actions is already the
            // natural response, whether or not eps > 0.
            return vec![1.0 / w as f64; w];
        }
        let free = (1.0 - eps).max(0.0);
        (0..w).map(|a| floor + free * pos[a] / total).collect()
    }

    /// Current strategy from accumulated strat sums (for snapshots/inspection).
    pub fn avg_strategy(&self, off: u32, w: usize) -> Vec<f64> {
        // F4: read the f64 strat sums directly (no f32 round trip).
        let mut sums: Vec<f64> = (0..w)
            .map(|a| self.arena64.load(self.slot_of(off, w + a)))
            .collect();
        let total: f64 = sums.iter().sum();
        if total <= 0.0 {
            return vec![1.0 / w as f64; w];
        }
        for v in sums.iter_mut() {
            *v /= total;
        }
        sums
    }

    /// f32 growth guard (SPECS/04 §2): scale strat_sum + avg_weight by 2^-k when
    /// either exceeds 2^22, bringing the max under 2^20. Lossless for normalized
    /// strategies.
    /// F4 (2026-10-01): obsolete. With the strategy sum and weight in an
    /// f64 arena there is no f32 precision ceiling to defend against. This
    /// stub is kept for API compatibility with the serial + parallel
    /// snapshot passes, which will all see `false`.
    pub fn renorm_row(&mut self, _off: u32, _w: usize) -> bool {
        false
    }

    /// Iterate all (key, off) pairs in slot order (for snapshots / warm-start).
    /// Iterate over (key, off, w) for every live row. `w` is the row's
    /// width, which is stored in the slot; callers that previously did
    /// `(k, off, table.row_width(off))` should consume this directly.
    /// Fixes an O(n^2) scan: see COMPETITIVE-REVIEW-2026-10-01 F6.
    pub fn iter(&self) -> impl Iterator<Item = (u64, u32, usize)> + '_ {
        self.slots
            .iter()
            .filter(|s| s.key != 0)
            .map(|s| (s.key, s.off, s.w as usize))
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
        // L-6 fix (2026-09-27): use the SAME double-hash probe sequence as
        // `find` and `entry_or_insert` (`hash_key` + `hash_step`). The old
        // version probed with a linear `+1` step, which is a DIFFERENT
        // sequence — it can report "absent" for a key that is actually
        // present in the table, because the linear walk never visits the
        // slots the double-hash walk populated.
        let step = hash_step(key, self.mask);
        let mut i = hash_key(key) & self.mask;
        loop {
            let s = self.slots[i];
            if s.key == key {
                return Some(s.w as usize);
            }
            if s.key == 0 {
                return None;
            }
            i = (i + step) & self.mask;
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
            /// F4: regret + visit cells (f32/u32 in the packed arena).
            rows: Vec<(u64, u8, Vec<u32>, Vec<u64>)>,
            /// H-9: global iteration index the table is trained through.
            last_iter: u64,
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
            let mut cells64 = Vec::with_capacity(2 * w + 2);
            for i in 0..2 * w + 2 {
                cells64.push(
                    self.arena64.cells[s.off as usize + i]
                        .load(std::sync::atomic::Ordering::Relaxed),
                );
            }
            rows.push((s.key, s.w, cells, cells64));
        }
        rows.sort_by_key(|r| r.0);
        let snap = Snap {
            n: self.n,
            mode: self.mode,
            rows,
            last_iter: self.last_iter,
        };
        // M-9 fix (2026-09-27): the previous code silently replaced a
        // serialization or compression failure with an empty body, producing
        // a snapshot file whose corrupt payload would then be reported as a
        // confusing "postcard:" or "zstd:" error at restore time. Panic here
        // instead — a failed snapshot is a hard invariant violation (the
        // table is serializable by construction; if it's not, the bug is
        // here, not at the reader).
        let raw = postcard::to_allocvec(&snap)
            .expect("cham-blueprint: table snapshot postcard-serialize");
        let body =
            zstd::bulk::compress(&raw, 3).expect("cham-blueprint: table snapshot zstd-compress");
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
            /// F4: regret/visit + f64 strategy/weight cells.
            rows: Vec<(u64, u8, Vec<u32>, Vec<u64>)>,
            /// H-9: resume position.
            last_iter: u64,
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
        self.arena64 = Arena64::with_capacity(slots_cap * 4);
        self.arena_len = 0;
        self.mode = snap.mode;
        self.last_iter = snap.last_iter;
        for (key, w, cells, cells64) in snap.rows {
            let (off, w_out) = self.entry_or_insert(key, w as usize);
            debug_assert_eq!(w_out, w as usize);
            for (i, c) in cells.iter().enumerate() {
                self.arena.store(off as usize + i, *c);
            }
            for (i, c) in cells64.iter().enumerate() {
                self.arena64.cells[off as usize + i]
                    .store(*c, std::sync::atomic::Ordering::Relaxed);
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

impl RegretTable {
    /// F5 (2026-10-01, competitiveness report): DCFR slice-boundary discount.
    /// Applies Brown & Sandholm 2019's telescoped discount to every regret
    /// cell: positive regrets scaled by `fp = Π s^α / (s^α + 1)`, negative
    /// by `fn = Π s^β / (s^β + 1)`. `α = β = 1.0` is a no-op.
    pub fn discount_all(&self, t_prev: u64, t_now: u64, alpha: f64, beta: f64) {
        if t_now <= t_prev {
            return;
        }
        if (alpha - 1.0).abs() < 1e-12 && (beta - 1.0).abs() < 1e-12 {
            return;
        }
        let fp = self.telescope_discount(t_prev, t_now, alpha);
        let fn_ = self.telescope_discount(t_prev, t_now, beta);
        if (fp - 1.0).abs() < 1e-15 && (fn_ - 1.0).abs() < 1e-15 {
            return;
        }
        for s in &self.slots {
            if s.key == 0 {
                continue;
            }
            let w = s.w as usize;
            for a in 0..w {
                let slot = self.slot_of(s.off, a);
                let cur = f32::from_bits(self.arena.load(slot));
                if cur == 0.0 {
                    continue;
                }
                let scaled = if cur > 0.0 {
                    (cur as f64) * fp
                } else {
                    (cur as f64) * fn_
                };
                self.arena.store(slot, (scaled as f32).to_bits());
            }
        }
    }

    fn telescope_discount(&self, t_prev: u64, t_now: u64, alpha: f64) -> f64 {
        if (alpha - 1.0).abs() < 1e-12 {
            return (t_prev as f64 + 1.0) / (t_now as f64 + 1.0);
        }
        if alpha.abs() < 1e-12 {
            let n = (t_now - t_prev) as f64;
            return 0.5_f64.powf(n);
        }
        let lo = t_prev.saturating_add(1).max(1);
        let hi = t_now.max(lo);
        let n = (hi - lo + 1).min(2_000_000);
        let mut log_sum = 0.0_f64;
        for s in lo..(lo + n) {
            let sa = (s as f64).powf(alpha);
            log_sum += (sa / (sa + 1.0)).ln();
        }
        log_sum.exp()
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
