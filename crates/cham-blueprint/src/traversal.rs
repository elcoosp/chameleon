//! The traversal (SPECS/04 §4) — the VALID estimator.
//!
//! ```text
//! walk(state, hero_seat, w_t) -> hero utility (bb):
//!   terminal            -> payoff(hero_seat) / 100
//!   chance (street deal)-> the engine deals from the iteration's shuffled deck
//!   opponent node       -> probs = opp.action_probs(obs)  (analytic, fresh)
//!                          sample ONCE; sampling IS the reach weighting
//!                          (multiplying by reach_opp would double-count — v1 bug)
//!   hero node           -> ENUMERATE all W ladder slots
//!                          sigma = regret-matching+ over the row
//!                          R_a += v_a − v̄      (NO reach factor, NO baseline)
//!                          S_a += w_t·sigma_a;  visits += 1
//!   RBP                 -> skip enumeration when Σ max(R,0) < θ_t (Pluribus trick)
//! ```

use cham_core::engine::{Action, State};
use cham_core::obs::{Agent, Observables, Player};
use cham_core::rng::Rng;

use crate::modes::TrainModeTag;
use crate::table::{DeltaBuffer, RegretTable};
use arrayvec::ArrayVec;

/// Update sink for hero-node writes (PERF-PLAN T3).
///
/// `Deterministic` training writes through [`DirectSink`] (the historical
/// behavior, bit-exact). `Snapbatch` training buffers through
/// [`SnapBatchSink`] and flushes one atomic op per slot.
pub trait RegretSink {
    fn add_regret(&mut self, table: &RegretTable, off: u32, a: usize, delta: f32);
    fn add_strat(&mut self, table: &RegretTable, off: u32, w: usize, a: usize, delta: f64);
    fn add_weight(&mut self, table: &RegretTable, off: u32, w: usize, delta: f64);
    fn add_visit(&mut self, table: &RegretTable, off: u32, w: usize);
}

/// Direct atomic writes (existing behavior; `Deterministic` is bit-exact).
pub struct DirectSink {
    pub regret_discount: f32,
}

impl DirectSink {
    pub fn new(regret_discount: f32) -> DirectSink {
        DirectSink { regret_discount }
    }
}

impl RegretSink for DirectSink {
    fn add_regret(&mut self, table: &RegretTable, off: u32, a: usize, delta: f32) {
        if self.regret_discount < 1.0 {
            table.regret_add_cfr_plus_discounted(off, a, delta, self.regret_discount);
        } else {
            table.regret_add_cfr_plus(off, a, delta);
        }
    }
    fn add_strat(&mut self, table: &RegretTable, off: u32, w: usize, a: usize, delta: f64) {
        table.strat_add(off, w, a, delta);
    }
    fn add_weight(&mut self, table: &RegretTable, off: u32, w: usize, delta: f64) {
        table.add_weight(off, w, delta);
    }
    fn add_visit(&mut self, table: &RegretTable, off: u32, w: usize) {
        table.add_visit(off, w);
    }
}

/// Buffered snapbatch writes: zero atomics per visit; flush aggregates to
/// one atomic op per slot. Flushes automatically when the buffer fills; the
/// owner flushes leftovers every K traversals (or at iteration end).
pub struct SnapBatchSink {
    pub buf: DeltaBuffer,
    pub regret_discount: f32,
}

impl SnapBatchSink {
    pub fn new() -> SnapBatchSink {
        SnapBatchSink {
            buf: DeltaBuffer::new(),
            regret_discount: 1.0,
        }
    }

    pub fn with_discount(regret_discount: f32) -> SnapBatchSink {
        SnapBatchSink {
            buf: DeltaBuffer::new(),
            regret_discount,
        }
    }

    /// Flush buffered deltas into the table (one atomic op per slot).
    /// The regret discount is applied per merged slot at flush time, so the
    /// effective number of discounts differs from DirectSink (which applies
    /// per-call). Documented; only active when `regret_discount < 1.0`.
    pub fn flush(&mut self, table: &RegretTable) {
        self.buf.flush_with_discount(table, self.regret_discount);
    }

    pub fn pending(&self) -> usize {
        self.buf.len()
    }
}

impl Default for SnapBatchSink {
    fn default() -> Self {
        Self::new()
    }
}

impl RegretSink for SnapBatchSink {
    fn add_regret(&mut self, table: &RegretTable, off: u32, a: usize, delta: f32) {
        self.buf.push_regret(off, a, delta);
        if self.buf.regrets_full() {
            self.flush(table);
        }
    }
    fn add_strat(&mut self, table: &RegretTable, off: u32, w: usize, a: usize, delta: f64) {
        self.buf.push_strat(off, w, a, delta);
        if self.buf.strats_full() {
            self.buf.flush(table);
        }
    }
    fn add_weight(&mut self, _table: &RegretTable, off: u32, w: usize, delta: f64) {
        self.buf.push_weight(off, w, delta);
    }
    fn add_visit(&mut self, _table: &RegretTable, off: u32, w: usize) {
        self.buf.push_visit(off, w);
    }
}

/// Regret-based pruning configuration (SPECS/04 §4).
#[derive(Clone, Copy, Debug)]
pub struct RbpConfig {
    pub theta0: f64, // bb
    pub delta: f64,  // per-iteration decay
}

impl Default for RbpConfig {
    /// D-011: node-level Σ-form RBP deadlocks under CFR+ regret flooring, and the
    /// per-action adaptation (skip zero-regret actions) needs revival semantics we
    /// do not trust yet — so pruning is DISABLED by default (θ0 = 0) and its
    /// recalibration is deferred to the M2 throughput spike (SPECS/11 fallback
    /// table). The machinery stays; `rbp_matches_full` pins no-corruption.
    fn default() -> Self {
        // TEST-ONLY diagnostic (session 2026-09-27): CHAM_RBP_THETA0 lets
        // the sb_internals test (and the CLI) override the pruning
        // threshold without editing source. Unset → 0.0, which disables
        // pruning entirely (the traversal's `prune_enabled = theta_t > 0.0`
        // gate treats theta0 = 0 as off). Set a POSITIVE value to enable.
        // (Bug hunt 2026-10-02: the previous comment claimed theta0 = 0
        // pruned everything and that 1e18 disabled; both were stale — the
        // traversal gate was fixed, the comment was not.)
        //
        // §3.6 perf: `default()` runs per traversal (per iteration), so the
        // env read is cached process-wide instead of hitting the syscall
        // table every iteration.
        use std::sync::OnceLock;
        static THETA0: OnceLock<f64> = OnceLock::new();
        let theta0 = *THETA0.get_or_init(|| {
            std::env::var("CHAM_RBP_THETA0")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0)
        });
        RbpConfig { theta0, delta: 1.0 }
    }
}

/// One traversal context: bound to a table + encoder; the opponent is an
/// `Agent` consumed ONLY through `action_probs` in Exploit modes (normative).
/// How a `Traversal` accesses the `RegretTable`.
///
/// PERF (2026-09-29): the parallel trainer needs multiple workers to share
/// one table, but `Traversal` historically took `&mut RegretTable` because
/// `entry_or_insert` may grow the slot array. All the atomic update
/// methods (`regret_add_cfr_plus`, `strat_add`, `add_weight`, `add_visit`)
/// already take `&self` (they CAS into `AtomicU32` cells), so reads and
/// updates are safe under a shared reference. Only insert needs `&mut`.
///
/// `Exclusive` = single-threaded path; full access, insert allowed.
/// `Shared`    = parallel worker; reads + atomic updates only. A key that
///               is not yet in the table is skipped for that iteration
///               (the next warmup slice will insert it).
pub enum TableRef<'a> {
    Exclusive(&'a mut RegretTable),
    Shared(&'a RegretTable),
}

impl<'a> TableRef<'a> {
    /// Read-only view (works for both variants).
    #[inline]
    /// Borrow the underlying table regardless of variant. Named `as_ref`
    /// intentionally to mirror the previous API; the clippy lint about
    /// `AsRef` trait is suppressed deliberately (TableRef is not a
    /// generic wrapper, and switching to the trait would obscure the
    /// variant distinction that is the point of this enum).
    #[allow(clippy::should_implement_trait)]
    pub fn as_ref(&self) -> &RegretTable {
        match self {
            TableRef::Exclusive(t) => t,
            TableRef::Shared(t) => t,
        }
    }
    /// Insert a row, growing the table if needed. Returns None on a
    /// `Shared` reference (parallel workers cannot grow the table).
    #[inline]
    pub fn entry_or_insert(&mut self, key: u64, w: usize) -> Option<(u32, usize)> {
        match self {
            TableRef::Exclusive(t) => Some(t.entry_or_insert(key, w)),
            TableRef::Shared(_) => None,
        }
    }
}

pub struct Traversal<'a> {
    pub table: TableRef<'a>,
    pub opp: &'a mut dyn Agent,
    pub rbp: RbpConfig,
    /// §3.6 cold-row telemetry: hero nodes skipped because the row is
    /// missing and `allow_insert == false` (parallel phase). The whole
    /// traversal's ancestors skip their update (selection bias); the
    /// trainer prints `cold_rows / hero_nodes` so the rate is known.
    /// >1% on rich/medium trees ⇒ implement per-thread shard insert.
    pub cold_rows: u64,
    pub iteration: u64,
    pub mode: TrainModeTag,
    /// counters for the seat histogram / pruning stats (printed by the trainer)
    pub hero_nodes: u64,
    pub pruned_nodes: u64,
    /// DCFR regret discount (1.0 = pure CFR+). Applied at add_regret.
    pub regret_discount: f32,
    /// PERF (2026-09-29): when false, a hero node whose key is absent from
    /// the table is skipped (no `entry_or_insert` call) and its subtree is
    /// not sampled. This is the precondition for parallel (Hogwild) training:
    /// the table's `add_*` methods take `&self` (atomic CAS), but
    /// `entry_or_insert` takes `&mut self` (it may grow the slot array).
    /// A single-threaded warmup pass populates the table; parallel workers
    /// then only touch existing rows. New infosets discovered during the
    /// parallel phase are effectively ignored for that iteration; the next
    /// warmup interval catches them.
    pub allow_insert: bool,
    /// WARMUP ONLY (2026-09-29): when true, this traversal INSERT-ONLY —
    /// it walks the tree to discover and create rows but performs no
    /// CFR+ updates at all.
    ///
    /// Why: the parallel trainer's warmup burst used to run FULL traversals
    /// with allow_insert=true. Every slice therefore re-applied CFR+ updates
    /// to rows that already carried accumulated state from previous slices —
    /// reducing their strategy-sum mass in place. That is the likely cause
    /// of the "tiny peaks at 5M" pattern: the more slices, the more the
    /// accumulated sum is overwritten by warmup's redundant updates.
    pub warmup_only: bool,
    /// F9 (2026-10-01): training-time exploration floor for `sigma_rms`
    /// at this traversal's node updates. Replaces the previous use of
    /// the process-global `TRAIN_EXPLORE_EPS` static. 0.0 = no floor.
    pub explore_eps: f64,
}

impl<'a> Traversal<'a> {
    /// Single traversal returning hero utility (bb). `Deterministic`
    /// behavior: writes go directly to the table (bit-exact).
    #[allow(clippy::too_many_arguments)]
    pub fn walk(
        &mut self,
        state: &mut State,
        hero_seat: usize,
        w_t: f64,
        seq: &mut cham_engine::encoder::ActionSeq,
        enc: &mut cham_engine::Encoder,
        rng: &mut Rng,
    ) -> f64 {
        let mut sink = DirectSink::new(self.regret_discount);
        self.walk_with_sink(state, hero_seat, w_t, seq, enc, rng, &mut sink)
    }

    /// Traversal with an explicit update sink (`Snapbatch` buffers deltas;
    /// see [`RegretSink`]). Recursive calls thread the same sink through.
    #[allow(clippy::too_many_arguments)]
    pub fn walk_with_sink(
        &mut self,
        state: &mut State,
        hero_seat: usize,
        w_t: f64,
        seq: &mut cham_engine::encoder::ActionSeq,
        enc: &mut cham_engine::Encoder,
        rng: &mut Rng,
        sink: &mut dyn RegretSink,
    ) -> f64 {
        if state.is_terminal() {
            return state.payoffs()[hero_seat] as f64 / 100.0;
        }
        let p = state.to_act();

        // ---- opponent node ----
        if p != hero_seat {
            let obs = Observables::view(state, Player::from_usize(p));
            // v3 §6 (M6): sync the current-path history into sequence-aware
            // opponents (FrozenAgent) before the oracle query, so frozen keys
            // match the victim's live keys exactly. Stateless opponents
            // return None and skip this (zero behavior change).
            if let Some(any) = self.opp.as_any_mut() {
                if let Some(f) = any.downcast_mut::<cham_opponents::frozen::FrozenAgent>() {
                    f.set_seq(*seq);
                }
            }
            // In Robust mode the opponent is the other seat's CURRENT strategy
            // sampled from its own rows; in Exploit modes the scripted oracle.
            let dist: ArrayVec<(Action, f64), 12> = if self.mode == TrainModeTag::Robust {
                // One ladder derivation per visit (PERF-PLAN T4): slots feed
                // the key, the width and the action mapping together.
                let slots = enc.slots(&obs, seq);
                let key = enc.key_for(&obs, seq, &slots);
                let w_slots = slots.len();
                match self.table.as_ref().find(key.0) {
                    Some(off) => {
                        let sigma =
                            self.table
                                .as_ref()
                                .sigma_rms_eps(off, w_slots, self.explore_eps);
                        // F3 (2026-10-01, chameleon-competitiveness-report):
                        // accumulate the AVERAGE strategy at the OPPONENT
                        // node. In external-sampling MCCFR the traverser
                        // enumerates its own actions, so the opponent node
                        // is where the sampling itself supplies the
                        // reach-weight the average needs. The previous
                        // site (hero node, no reach factor) is the
                        // production scheme the report flagged.
                        if !self.warmup_only {
                            for a in 0..w_slots {
                                sink.add_strat(
                                    self.table.as_ref(),
                                    off,
                                    w_slots,
                                    a,
                                    w_t * sigma[a],
                                );
                            }
                            sink.add_weight(self.table.as_ref(), off, w_slots, w_t);
                        }
                        sigma
                            .iter()
                            .enumerate()
                            .map(|(i, pr)| (slots[i].action, *pr))
                            .collect()
                    }
                    None => slots
                        .iter()
                        .map(|s| (s.action, 1.0 / w_slots as f64))
                        .collect(),
                }
            } else {
                match self.opp.action_probs(&obs) {
                    Ok(d) => d.iter().map(|(a, p)| (*a, *p)).collect(),
                    Err(_) => {
                        let slots = enc.slots(&obs, seq);
                        let w_slots = slots.len();
                        slots
                            .iter()
                            .map(|s| (s.action, 1.0 / w_slots as f64))
                            .collect()
                    }
                }
            };
            // EXPLORATION (session 2026-09-27): with probability ε, replace the
            // opponent's σ with uniform over the legal slots. This forces the
            // hero's regret-matching to keep seeing every opponent action, so
            // CFR+'s regret floor doesn't permanently freeze an action at zero
            // (the pure-strategy collapse diagnosed this session). Unset → 0.0,
            // historical behavior bit-identical.
            // F9 (2026-10-01): cached per process, not read per node.
            // §3.6: single source of truth is `self.explore_eps`
            // (TrainerConfig); the process-global env static is only a
            // fallback when the field is 0.0, so cfg and env can no
            // longer silently disagree.
            let eps: f64 = if self.explore_eps > 0.0 {
                self.explore_eps
            } else {
                use std::sync::OnceLock;
                static EPS: OnceLock<f64> = OnceLock::new();
                *EPS.get_or_init(|| {
                    std::env::var("CHAM_EXPLORE_EPS")
                        .ok()
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(0.0)
                })
            };
            let dist: ArrayVec<(Action, f64), 12> =
                if eps > 0.0 && cham_core::rng::next_f64(rng) < eps {
                    let n = dist.len().max(1) as f64;
                    dist.iter().map(|(a, _)| (*a, 1.0 / n)).collect()
                } else {
                    dist
                };
            let a = sample_action(&dist, rng);
            enc.record(&obs, Player::from_usize(p), a, seq);
            let out = state.apply(a).expect("sampled action is legal");
            let _ = out;
            return self.walk_with_sink(state, hero_seat, w_t, seq, enc, rng, sink);
        }

        // ---- hero node (or the updating seat in Robust mode) ----
        // One ladder derivation per visit (PERF-PLAN T4): slots feed the key,
        // the row width and the action mapping together.
        let obs = Observables::view(state, Player::from_usize(p));
        let slots = enc.slots(&obs, seq);
        let key = enc.key_for(&obs, seq, &slots);
        let w_slots = slots.len();
        // 2026-10-01 (F3): in parallel (allow_insert=false) mode, a missing
        // row means the subtree was never evaluated. Returning 0.0 biases
        // the ancestor's regret update toward whichever branch happened to
        // hit cold rows. Return f64::NAN instead — a sentinel that says
        // "no information about this subtree". Callers detect NaN and skip
        // the CFR+ update entirely (rather than fold a wrong value).
        // Legitimate payoffs are finite bb values, so NaN cannot collide.
        let off = match self.table.as_ref().find(key.0) {
            Some(off) => off,
            None => {
                if !self.allow_insert {
                    self.cold_rows += 1;
                    return f64::NAN;
                }
                match self.table.entry_or_insert(key.0, w_slots) {
                    Some((off, _w)) => off,
                    None => return f64::NAN,
                }
            }
        };
        self.hero_nodes += 1;

        // Regret-based pruning (Pluribus trick, adapted to RM+ — decision D-011):
        // an action with R_a == 0 (zero positive regret, outside the RM+ support)
        // is skipped after a decaying visit threshold θ_t = θ0·δ^t; its subtree is
        // not sampled and its value is treated pessimistically (min of the rest),
        // which keeps its regret floored at 0. Fresh rows enumerate everything.
        // RBP gate. `theta0 <= 0.0` DISABLES pruning entirely (the doc-comment
        // contract — see RbpConfig::default). The previous form
        //   `visits > theta_t` with theta0 = 0
        // is `visits > 0`, i.e. TRUE from the first visit — so the "disabled"
        // default actually pruned every zero-regret action always, which
        // froze regret-matching+ into a pure strategy on the first few
        // iterations (session 2026-09-27 SB-root internals dump: 3 of 4
        // regrets pinned at 0, σ one-hot, avg == σ). Fix: skip the prune
        // branch unconditionally when theta_t <= 0.0.
        let theta_t = self.rbp.theta0 * self.rbp.delta.powi(self.iteration.min(1 << 30) as i32);
        let prune_enabled = theta_t > 0.0;
        let visits = self.table.as_ref().visits(off, w_slots) as f64;

        let sigma = self
            .table
            .as_ref()
            .sigma_rms_eps(off, w_slots, self.explore_eps);
        let mut v = [0f64; 12];
        // F9-alloc (2026-10-01, competitiveness report): ArrayVec instead
        // of a per-node Vec. Slot count ≤ 12 (invariant I8).
        let mut computed: ArrayVec<usize, 12> = ArrayVec::new();
        for a in 0..w_slots {
            let zero_regret = self.table.as_ref().regret(off, w_slots, a) <= 0.0;
            if prune_enabled && zero_regret && visits > theta_t && sigma[a] <= 0.0 {
                self.pruned_nodes += 1;
                continue; // subtree skipped; v[a] filled below
            }
            let real = slots[a].action;
            let mut s2 = *state;
            let mut seq2 = *seq;
            enc.record(&obs, Player::from_usize(p), real, &mut seq2);
            s2.apply(real).expect("slot action is legal");
            v[a] = self.walk_with_sink(&mut s2, hero_seat, w_t, &mut seq2, enc, rng, sink);
            computed.push(a);
        }
        if !computed.is_empty() && computed.len() < w_slots {
            let worst = computed.iter().map(|&a| v[a]).fold(f64::INFINITY, f64::min);
            for a in 0..w_slots {
                if !computed.contains(&a) {
                    v[a] = worst; // pessimistic: regret stays floored at 0
                }
            }
        }
        // 2026-10-01 (F3): if any action's subtree returned NaN, we have no
        // information about this node's value. Skip the CFR+ update rather
        // than fold a wrong (biased) value. The sentinel propagates to the
        // root, where the trainer discards that traversal.
        if v[..w_slots].iter().any(|x| x.is_nan()) {
            return f64::NAN;
        }
        let v_bar: f64 = (0..w_slots).map(|a| sigma[a] * v[a]).sum();
        // WARMUP ONLY (2026-09-29): skip all CFR+ updates. The row was
        // created above by entry_or_insert; the parallel phase fills it.
        if self.warmup_only {
            return v_bar;
        }
        // regret-matching+ floors at zero (SPECS/04 §4; pinned by rm_plus_floors).
        // Writes go through the sink: direct (bit-exact) or snapbatch-buffered.
        for a in 0..w_slots {
            sink.add_regret(self.table.as_ref(), off, a, (v[a] - v_bar) as f32);
        }
        // F3 (2026-10-01): in Robust mode the average strategy is
        // accumulated at the OPPONENT node (see above). Skip it here so
        // it is not double-counted. Exploit modes (one-sided vs a
        // scripted opponent) keep the hero-node accumulation — there is
        // no opponent row to accumulate at.
        if self.mode != TrainModeTag::Robust {
            for a in 0..w_slots {
                sink.add_strat(self.table.as_ref(), off, w_slots, a, w_t * sigma[a]);
            }
            sink.add_weight(self.table.as_ref(), off, w_slots, w_t);
        }
        sink.add_visit(self.table.as_ref(), off, w_slots);
        v_bar
    }
}

/// Sample an action index from probabilities.
///
/// L-7 fix (2026-09-27): the previous `probs.len() - 1` on an empty slice
/// underflowed usize → an enormous index that callers would then use to
/// panic on the next slice access. An empty distribution is a caller bug;
/// we refuse loudly with a debug assert in debug builds and return 0 in
/// release (which is what the sole caller does on empty: nothing sensible
/// to sample from). Better to catch it here than to panic later with a
/// confusing "index out of bounds: the len is 0 but the index is
/// 18446744073709551615" from a distant call site.
pub fn sample_index(probs: &[f64], rng: &mut Rng) -> usize {
    if probs.is_empty() {
        debug_assert!(
            false,
            "sample_index called with an empty distribution — a caller bug"
        );
        return 0;
    }
    let u = cham_core::rng::next_f64(rng);
    let mut acc = 0.0;
    for (i, p) in probs.iter().enumerate() {
        acc += p;
        if u <= acc {
            return i;
        }
    }
    probs.len() - 1
}

/// Sample an action from (action, prob) pairs.
pub fn sample_action(dist: &[(Action, f64)], rng: &mut Rng) -> Action {
    let u = cham_core::rng::next_f64(rng);
    let mut acc = 0.0;
    for (a, p) in dist {
        acc += p;
        if u <= acc {
            return *a;
        }
    }
    dist.last().map(|(a, _)| *a).unwrap_or(Action::Check)
}
