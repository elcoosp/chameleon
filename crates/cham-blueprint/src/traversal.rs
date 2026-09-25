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

/// Update sink for hero-node writes (PERF-PLAN T3).
///
/// `Deterministic` training writes through [`DirectSink`] (the historical
/// behavior, bit-exact). `Snapbatch` training buffers through
/// [`SnapBatchSink`] and flushes one atomic op per slot.
pub trait RegretSink {
    fn add_regret(&mut self, table: &RegretTable, off: u32, a: usize, delta: f32);
    fn add_strat(&mut self, table: &RegretTable, off: u32, w: usize, a: usize, delta: f32);
    fn add_weight(&mut self, table: &RegretTable, off: u32, w: usize, delta: f32);
    fn add_visit(&mut self, table: &RegretTable, off: u32, w: usize);
}

/// Direct atomic writes (existing behavior; `Deterministic` is bit-exact).
pub struct DirectSink;

impl RegretSink for DirectSink {
    fn add_regret(&mut self, table: &RegretTable, off: u32, a: usize, delta: f32) {
        table.regret_add_cfr_plus(off, a, delta);
    }
    fn add_strat(&mut self, table: &RegretTable, off: u32, w: usize, a: usize, delta: f32) {
        table.strat_add(off, w, a, delta);
    }
    fn add_weight(&mut self, table: &RegretTable, off: u32, w: usize, delta: f32) {
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
}

impl SnapBatchSink {
    pub fn new() -> SnapBatchSink {
        SnapBatchSink {
            buf: DeltaBuffer::new(),
        }
    }

    /// Flush buffered deltas into the table (one atomic op per slot).
    pub fn flush(&mut self, table: &RegretTable) {
        self.buf.flush(table);
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
            self.buf.flush(table);
        }
    }
    fn add_strat(&mut self, table: &RegretTable, off: u32, w: usize, a: usize, delta: f32) {
        self.buf.push_strat(off, w, a, delta);
        if self.buf.strats_full() {
            self.buf.flush(table);
        }
    }
    fn add_weight(&mut self, _table: &RegretTable, off: u32, w: usize, delta: f32) {
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
        RbpConfig {
            theta0: 0.0,
            delta: 1.0,
        }
    }
}

/// One traversal context: bound to a table + encoder; the opponent is an
/// `Agent` consumed ONLY through `action_probs` in Exploit modes (normative).
pub struct Traversal<'a> {
    pub table: &'a mut RegretTable,
    pub opp: &'a mut dyn Agent,
    pub rbp: RbpConfig,
    pub iteration: u64,
    pub total_iters: u64,
    pub mode: TrainModeTag,
    /// counters for the seat histogram / pruning stats (printed by the trainer)
    pub hero_nodes: u64,
    pub pruned_nodes: u64,
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
        let mut sink = DirectSink;
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
            // In Robust mode the opponent is the other seat's CURRENT strategy
            // sampled from its own rows; in Exploit modes the scripted oracle.
            let dist: Vec<(Action, f64)> = if self.mode == TrainModeTag::Robust {
                // One ladder derivation per visit (PERF-PLAN T4): slots feed
                // the key, the width and the action mapping together.
                let slots = enc.slots(&obs, seq);
                let key = enc.key_for(&obs, seq, &slots);
                let w_slots = slots.len();
                match self.table.find(key.0) {
                    Some(off) => {
                        let sigma = self.table.sigma_rms(off, w_slots);
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
        let (off, _w) = self.table.entry_or_insert(key.0, w_slots);
        self.hero_nodes += 1;

        // Regret-based pruning (Pluribus trick, adapted to RM+ — decision D-011):
        // an action with R_a == 0 (zero positive regret, outside the RM+ support)
        // is skipped after a decaying visit threshold θ_t = θ0·δ^t; its subtree is
        // not sampled and its value is treated pessimistically (min of the rest),
        // which keeps its regret floored at 0. Fresh rows enumerate everything.
        let theta_t = self.rbp.theta0 * self.rbp.delta.powi(self.iteration.min(1 << 30) as i32);
        let visits = self.table.visits(off, w_slots) as f64;

        let sigma = self.table.sigma_rms(off, w_slots);
        let mut v = [0f64; 12];
        let mut computed: Vec<usize> = Vec::with_capacity(w_slots);
        for a in 0..w_slots {
            let zero_regret = self.table.regret(off, w_slots, a) <= 0.0;
            if zero_regret && visits > theta_t && sigma[a] <= 0.0 {
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
        let v_bar: f64 = (0..w_slots).map(|a| sigma[a] * v[a]).sum();
        // regret-matching+ floors at zero (SPECS/04 §4; pinned by rm_plus_floors).
        // Writes go through the sink: direct (bit-exact) or snapbatch-buffered.
        for a in 0..w_slots {
            sink.add_regret(self.table, off, a, (v[a] - v_bar) as f32);
        }
        for a in 0..w_slots {
            sink.add_strat(self.table, off, w_slots, a, (w_t * sigma[a]) as f32);
        }
        sink.add_weight(self.table, off, w_slots, w_t as f32);
        sink.add_visit(self.table, off, w_slots);
        v_bar
    }
}

/// Sample an action index from probabilities.
pub fn sample_index(probs: &[f64], rng: &mut Rng) -> usize {
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
