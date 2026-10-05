//! `ChameleonAgent` (SPECS/07 §4): the composed pipeline.
//!
//! ```text
//! 1. hand start → feats = tracker features (frozen at hand start); w[5] =
//!    router.weights_for_hand(feats); FROZEN for the hand
//! 2. per decision: σ_k per expert (uncovered ⇒ robust σ substitution)
//! 3. σ_mix(a|i) ∝ Σ_k w_k·π_k(i)·σ_k(a|i)  — reach-weighted behavioral mixture;
//!    π_k from this hand's own earlier actions under expert k; fallback plain
//!    weighted average if Σ w_k π_k = 0; confidence-gated fallback per decision
//! 4. mode dispatch: argmax → one-hot expert; robust-only → robust σ; bayes → σ
//! 5. search (if enabled & trigger): solver override per mode's solver
//! 6. sample a ~ σ_mix (mixture); argmax/bayes consume NO rng (replayability)
//! 7. real action; trace; per-expert reach update; return
//! ```
//!
//! The full public action sequence (needed for infoset keys) is maintained via the
//! `on_public_action` hook fed by the match driver; our own actions are recorded in
//! `act`.

use cham_blueprint::policy::BlueprintPolicy;
use cham_core::engine::Action;
use cham_core::obs::{Agent, Observables, Player};
use cham_core::rng::{Rng, next_f64};
use cham_engine::encoder::{ActionSeq, Encoder};
use cham_rec::Recorder;

use crate::AgentError;
use crate::modes::AgentMode;
use crate::trace::{DecisionTrace, record as trace_record};
use crate::tracker::Tracker;

/// Cached "is hedge debug enabled?" flag (2026-09-30). Reading env::var
/// per decision is a syscall on the hot path; cache once at first query.
/// The flag is read-only for the process lifetime; a debug session sets
/// CHAM_HEDGE_DEBUG=1 before invoking the binary.
pub(crate) fn hedge_debug_enabled() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};
    static CACHE: AtomicU8 = AtomicU8::new(2); // 2 = unset, 0 = off, 1 = on
    match CACHE.load(Ordering::Relaxed) {
        0 => false,
        1 => true,
        _ => {
            let on = std::env::var("CHAM_HEDGE_DEBUG").as_deref() == Ok("1");
            CACHE.store(if on { 1 } else { 0 }, Ordering::Relaxed);
            on
        }
    }
}

/// F6c (2026-10-01): whether to translate the opponent's off-tree sizes
/// into same-class abstract slots before recording them into the
/// infoset key stream. Default OFF — the shipped bundle was keyed
/// without translation, so enabling by default would break its key
/// match against real opponents. Set `CHAM_OFFTREE_TRANSLATE=1` to
/// enable (Phase 2 of the competitiveness report).
fn offtree_translate_enabled() -> bool {
    use std::sync::OnceLock;
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("CHAM_OFFTREE_TRANSLATE")
            .ok()
            .map(|v| v == "1")
            .unwrap_or(false)
    })
}

/// F6c: deterministic `u` in `[0, 1)` for `ActionLadder::translate`.
/// Derived from `(hand_idx, street, seq.lens)` via FNV-1a so that a
/// replayed hand always picks the same slot.
fn derive_translate_u(hand_idx: u64, street: u8, lens: [u8; 4]) -> f64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in hand_idx.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h ^= street as u64;
    h = h.wrapping_mul(0x100_0000_01b3);
    for l in lens {
        h ^= l as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    // Top 53 bits → [0, 1); matches f64 mantissa precision.
    (h >> 11) as f64 / (1u64 << 53) as f64
}

pub struct ChameleonAgent {
    pub mode: AgentMode,
    pub encoder: Encoder,
    pub router: cham_router::runtime::RouterRuntime,
    pub experts: Vec<BlueprintPolicy>, // 4 specialists
    pub robust: BlueprintPolicy,
    pub bayes: Option<BlueprintPolicy>,
    pub tracker: Tracker,
    pub recorder: Option<Recorder>,
    // per-hand state
    hand_idx: u64,
    /// Hero seat for the CURRENT hand (captured at first `act`), used by
    /// `on_hand_end` to feed the tracker the correct opponent. In duplicate
    /// matching the agent plays BOTH seats — the tracker must model the
    /// opposite one, not hard-coded seat 0 (H-2, 2026-09-27).
    hero_seat: Option<usize>,
    weights: [f64; 5],
    weights_fresh: bool,
    /// per-expert own-reach product for the current hand (π_k)
    reach: [f64; 5],
    argmax_k: Option<usize>,
    /// canonical public action sequence (infoset key input)
    seq: ActionSeq,
    /// §3.4 per-hand exploit commitment (routing "bounded" only):
    /// `Some(true)` = this whole hand plays the chosen expert's σ,
    /// `Some(false)` = this whole hand plays robust σ. `None` = not yet
    /// drawn this hand (drawn lazily on the first `act_impl`, which owns
    /// the rng; `start_hand_if_needed` has none). Per-hand commitment —
    /// not per-infoset mixing — is what carries the sequence-form
    /// guarantee ε ≤ (1−λ)·ε_robust + λ·ε_expert.
    hand_plays_expert: Option<bool>,
    /// last trace (read by tests/driver)
    pub last_trace: Option<DecisionTrace>,
}

impl ChameleonAgent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mode: AgentMode,
        encoder: Encoder,
        router: cham_router::runtime::RouterRuntime,
        experts: Vec<BlueprintPolicy>,
        robust: BlueprintPolicy,
        bayes: Option<BlueprintPolicy>,
        recorder: Option<Recorder>,
    ) -> Result<ChameleonAgent, AgentError> {
        mode.validate()?;
        if experts.len() != 4 {
            return Err(AgentError::Pipeline(
                "exactly 4 specialists required".into(),
            ));
        }
        Ok(ChameleonAgent {
            mode,
            encoder,
            router,
            experts,
            robust,
            bayes,
            tracker: Tracker::new(),
            recorder,
            hand_idx: 0,
            hero_seat: None,
            weights: [0.0; 5],
            weights_fresh: false,
            reach: [1.0; 5],
            argmax_k: None,
            seq: ActionSeq::default(),
            hand_plays_expert: None,
            last_trace: None,
        })
    }

    /// §3.4 exploit rate for the committed expert this hand.
    ///
    /// λ = min(router confidence in k, EXPLOIT_BUDGET_MB /
    /// expert_exploitability_mb[k]). The budget and per-expert
    /// exploitabilities come from env (`CHAM_EXPLOIT_BUDGET_MB`,
    /// `CHAM_EXPERT_EXPL_MB_0..3`, measured by fine-information BR §3.3 —
    /// NOT the same-abstraction number). Default budget 0 ⇒ λ = 0
    /// (robust only) until operators set a measured budget.
    ///
    /// Free function (not a method): `act_impl` destructures `self`
    /// mutably and cannot take a second borrow.
    fn bounded_lambda(k: usize, conf: f64) -> f64 {
        let budget: f64 = std::env::var("CHAM_EXPLOIT_BUDGET_MB")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        if budget <= 0.0 {
            return 0.0;
        }
        let expl: f64 = std::env::var(format!("CHAM_EXPERT_EXPL_MB_{k}"))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2000.0);
        conf.min((budget / expl.max(1.0)).min(1.0)).clamp(0.0, 1.0)
    }

    fn start_hand_if_needed(&mut self) {
        if self.weights_fresh {
            return;
        }
        let inputs = cham_router::features::FeatureInputs {
            hands_seen: self.tracker.hands,
            ewm: self.tracker.shrunk_ewm(),
            opportunity: self.tracker.opportunity_features(),
            trend_z: self.tracker.trend_z() / 3.0,
            hands_since_showdown: self.tracker.hands_since_showdown_feature(),
        };
        // 2026-09-30: dispatch the feature vector based on the loaded
        // model's `feature_set` string. A `raw-opponent-19` model expects
        // 19 opponent-only features; `opportunity-gated-20` (the default)
        // expects the historical tracker vector. See
        // docs/plans/ROUTER-INTEGRATION-DESIGN-2026-09-30.md.
        let trend_z = self.tracker.trend_z();
        let feature_set = self.router.model.feature_set.as_str();
        let features: Vec<f32> = match feature_set {
            "raw-opponent-19" => self
                .opponent_only_features_19()
                .iter()
                .map(|&x| x as f32)
                .collect(),
            "raw-opponent-11" => self
                .opponent_only_features_11()
                .iter()
                .map(|&x| x as f32)
                .collect(),
            "raw-opponent-10" => self
                .opponent_only_features()
                .iter()
                .map(|&x| x as f32)
                .collect(),
            _ => cham_router::features::from_inputs(&inputs)
                .map(|f: cham_engine::RouterFeatures| f.0)
                .unwrap_or([0.5; 20])
                .to_vec(),
        };
        self.weights = self.router.weights_for_hand(&features, trend_z);
        self.reach = [1.0; 5];
        self.hand_plays_expert = None; // fresh commitment draw this hand
        self.argmax_k = if self.mode.routing == "argmax"
            || self.mode.routing == "sample-expert"
            || self.mode.routing == "hedged"
            || self.mode.routing == "bounded"
        {
            let mut best = 0usize;
            for (k, &w) in self.weights.iter().take(4).enumerate() {
                if w > self.weights[best] {
                    best = k;
                }
            }
            Some(best)
        } else {
            None
        };
        self.weights_fresh = true;
    }
}

// struct field order: encoder declared at the end so the helper above compiles
// (Rust allows any order; the accessor is for external users)
impl ChameleonAgent {
    /// L-18 test accessor (2026-09-27): expose the canonical action sequence
    /// so tests can assert that villain actions land in it. The seq is
    /// internal state normally; tests should not have to reconstruct it.
    pub fn seq_for_tests(&self) -> &cham_engine::encoder::ActionSeq {
        &self.seq
    }

    /// Test/measurement companion to `seq_for_tests`: overwrite the internal
    /// action sequence so an external caller (e.g. an LBR harness walking
    /// the same tree) can keep the agent's infoset keys in sync with the
    /// harness's own recursion.
    pub fn set_seq_for_tests(&mut self, seq: cham_engine::encoder::ActionSeq) {
        self.seq = seq;
    }

    /// 20-dim router feature vector as of the last hand boundary.
    /// Used to produce REAL router training data from instrumented sessions
    /// (as opposed to `collect`'s synthetic stub). Same computation as
    /// `start_hand_if_needed` uses; safe to call mid-hand.
    /// PERF (2026-09-29): the honest opponent-only feature vector — 10
    /// raw action frequencies that are a function of the opponent's
    /// behaviour, not of the (opponent, hero-policy) pair. See
    /// docs/plans/ROUTER-FEATURE-LEAK-2026-09-29.md.
    pub fn opponent_only_features(&self) -> [f64; 10] {
        self.tracker.raw_opponent_frequencies()
    }

    /// 11-dim honest opponent-only feature vector (2026-09-30): the same
    /// 10 raw frequencies plus the preflop/postflop aggression tilt.
    /// The tilt is the concrete TAG-vs-LAG discriminator identified in
    /// `ROUTER-TILT-FEATURE-DESIGN-2026-09-30.md`; the raw-frequency
    /// vector alone cannot separate those two archetypes (top-1 recall
    /// 0.52 vs 0.58). This accessor is emitted by
    /// `collect --real --raw-opponent-11` and is the training target for
    /// the next router.
    pub fn opponent_only_features_11(&self) -> [f64; 11] {
        let f = self.tracker.raw_opponent_frequencies();
        let tilt = self.tracker.preflop_postflop_tilt();
        let mut out = [0.0f64; 11];
        out[..10].copy_from_slice(&f);
        out[10] = tilt;
        out
    }

    /// 19-dim honest opponent-only feature vector (2026-09-30):
    ///  * 10 raw opponent action frequencies (as in the 10-dim accessor)
    ///  * 8 postflop bet-size histogram buckets (0.25-pot-fraction bins)
    ///  * 1 preflop/postflop aggression tilt
    ///
    /// The bet-size histogram is the "which hands the opponent raises
    /// with" signal that the raw-frequency vectors lack; see
    /// `docs/plans/ROUTER-BET-SIZE-FEATURE-DESIGN-2026-09-30.md`. This is
    /// the training target for the next router (`collect --real
    /// --raw-opponent-19`). Nothing here violates I9: the tracker's
    /// input is `PublicHistory` only, and bet sizes are public info.
    pub fn opponent_only_features_19(&self) -> [f64; 19] {
        let f = self.tracker.raw_opponent_frequencies();
        let hist = self.tracker.opponent_bet_size_hist();
        let tilt = self.tracker.preflop_postflop_tilt();
        let mut out = [0.0f64; 19];
        out[..10].copy_from_slice(&f);
        out[10..18].copy_from_slice(&hist);
        out[18] = tilt;
        out
    }

    pub fn tracker_features(&self) -> [f32; 20] {
        let inputs = cham_router::features::FeatureInputs {
            hands_seen: self.tracker.hands,
            ewm: self.tracker.shrunk_ewm(),
            opportunity: self.tracker.opportunity_features(),
            trend_z: self.tracker.trend_z() / 3.0,
            hands_since_showdown: self.tracker.hands_since_showdown_feature(),
        };
        cham_router::features::from_inputs(&inputs)
            .map(|f| f.0)
            .unwrap_or([0.5; 20])
    }

    /// Mixture LBR measurement hook (2026-09-28): produce the same
    /// per-action probability distribution that `act_impl` would sample
    /// from, WITHOUT sampling and WITHOUT advancing any per-hand state
    /// (encoder seq, reach, tracker weights). Callers that want to measure
    /// the shipped policy's exploitability (LBR harnesses) need this —
    /// `cham_core::obs::Agent::action_probs` cannot supply it because it
    /// takes `&self` and needs the stateful seq.
    ///
    /// Determinism: uses `start_hand_if_needed` (idempotent) but does NOT
    /// call `encoder.record`, so calling this method twice on the same
    /// state returns bit-identical results and does not perturb the
    /// pipeline's own action sequence.
    ///
    /// Matches `act_impl`'s mixture composition exactly for routing modes
    /// `mixture`, `argmax`, and `robust-only`. `bayes` routing is not
    /// covered (it consumes the bayes blueprint on the pipeline path);
    /// callers needing bayes should use the `argmax`-style branch.
    pub fn action_distribution(
        &mut self,
        obs: &Observables<'_>,
    ) -> Option<Vec<(cham_core::engine::Action, f64)>> {
        self.start_hand_if_needed();
        let encoder = &mut self.encoder;
        let seq = &self.seq;
        let slots = encoder.slots(obs, seq);
        let n = slots.len();
        if n == 0 {
            return None;
        }
        let w = self.weights;
        let reach = self.reach;
        let legacy_substitute = self.mode.fallback_mode == "substitute"
            || std::env::var("CHAM_FALLBACK_MODE").as_deref() == Ok("substitute");

        // gather per-tier strategies (no substitution)
        let mut expert_sigma: Vec<Option<Vec<f64>>> = Vec::with_capacity(4);
        for k in 0..4 {
            expert_sigma.push(self.experts[k].strategy(obs, encoder, seq));
        }
        let robust_sigma: Option<Vec<f64>> = self.robust.strategy(obs, encoder, seq);

        let mut mix = vec![0.0f64; n];
        let mut _fallback_bit = false;
        if legacy_substitute {
            let mut weight_mass = 0.0;
            for k in 0..4 {
                if w[k] <= 1e-9 {
                    continue;
                }
                let sigma = match expert_sigma[k].as_ref() {
                    Some(s) => s.clone(),
                    None => match robust_sigma.as_ref() {
                        Some(s) => {
                            _fallback_bit = true;
                            s.clone()
                        }
                        None => {
                            _fallback_bit = true;
                            vec![1.0 / n as f64; n]
                        }
                    },
                };
                let pi = reach[k];
                weight_mass += w[k] * pi;
                for a in 0..n {
                    mix[a] += w[k] * pi * sigma.get(a).copied().unwrap_or(0.0);
                }
            }
            {
                let sigma = match robust_sigma.as_ref() {
                    Some(s) => s.clone(),
                    None => {
                        _fallback_bit = true;
                        vec![1.0 / n as f64; n]
                    }
                };
                weight_mass += w[4] * reach[4];
                for a in 0..n {
                    mix[a] += w[4] * reach[4] * sigma.get(a).copied().unwrap_or(0.0);
                }
            }
            if weight_mass <= 1e-12 {
                _fallback_bit = true;
                mix = vec![0.0; n];
                for k in 0..4 {
                    if w[k] <= 1e-9 {
                        continue;
                    }
                    let sigma = expert_sigma[k]
                        .as_ref()
                        .cloned()
                        .unwrap_or_else(|| vec![1.0 / n as f64; n]);
                    for a in 0..n {
                        mix[a] += w[k] * sigma.get(a).copied().unwrap_or(0.0);
                    }
                }
            }
            let total: f64 = mix.iter().sum();
            if total <= 1e-12 {
                _fallback_bit = true;
                mix = vec![1.0 / n as f64; n];
            } else {
                for v in mix.iter_mut() {
                    *v /= total;
                }
            }
        } else {
            // R2 semantics: drop missed, renormalize, fallback only if empty
            let mut mass = 0.0;
            for k in 0..4 {
                if w[k] <= 1e-9 || expert_sigma[k].is_none() {
                    continue;
                }
                mass += w[k] * reach[k];
            }
            if robust_sigma.is_some() {
                mass += w[4] * reach[4];
            }
            let reach_mass_zero = mass <= 1e-12;
            let any_tier = (0..4).any(|k| expert_sigma[k].is_some()) || robust_sigma.is_some();
            if !any_tier {
                _fallback_bit = true;
                mix = vec![1.0 / n as f64; n];
            } else if reach_mass_zero {
                let mut m2 = 0.0;
                for k in 0..4 {
                    if w[k] <= 1e-9 {
                        continue;
                    }
                    if let Some(s) = expert_sigma[k].as_ref() {
                        for a in 0..n {
                            mix[a] += w[k] * s.get(a).copied().unwrap_or(0.0);
                        }
                        m2 += w[k];
                    }
                }
                if let Some(s) = robust_sigma.as_ref() {
                    for a in 0..n {
                        mix[a] += w[4] * s.get(a).copied().unwrap_or(0.0);
                    }
                    m2 += w[4];
                }
                if m2 > 1e-12 {
                    for v in mix.iter_mut() {
                        *v /= m2;
                    }
                }
            } else {
                for k in 0..4 {
                    if w[k] <= 1e-9 {
                        continue;
                    }
                    if let Some(s) = expert_sigma[k].as_ref() {
                        for a in 0..n {
                            mix[a] += w[k] * reach[k] * s.get(a).copied().unwrap_or(0.0);
                        }
                    }
                }
                if let Some(s) = robust_sigma.as_ref() {
                    for a in 0..n {
                        mix[a] += w[4] * reach[4] * s.get(a).copied().unwrap_or(0.0);
                    }
                }
                let total: f64 = mix.iter().sum();
                if total > 1e-12 {
                    for v in mix.iter_mut() {
                        *v /= total;
                    }
                }
            }
        }

        // dispatch matching act_impl's routing mode.
        //
        // §3.3(5): this hook measures the DEPLOYED agent. `argmax` plays
        // the mode as a pure strategy, so the hook returns the ONE-HOT
        // modal action — returning the expert's σ here would understate
        // the deployed agent's exploitability. Same for the confident
        // branch of `hedged` and greedy `bayes`.
        let dist: Vec<f64> = match self.mode.routing.as_str() {
            "robust-only" => robust_sigma
                .clone()
                .unwrap_or_else(|| vec![1.0 / n as f64; n]),
            "argmax" => {
                let k = self.argmax_k.unwrap_or(0);
                let sigma = expert_sigma[k].clone().unwrap_or_else(|| {
                    robust_sigma
                        .clone()
                        .unwrap_or_else(|| vec![1.0 / n as f64; n])
                });
                one_hot(&sigma)
            }
            "sample-expert" => {
                let k = self.argmax_k.unwrap_or(0);
                expert_sigma[k].clone().unwrap_or_else(|| {
                    robust_sigma
                        .clone()
                        .unwrap_or_else(|| vec![1.0 / n as f64; n])
                })
            }
            "hedged" => {
                let k = self.argmax_k.unwrap_or(0);
                let threshold = std::env::var("CHAM_HEDGE_THRESHOLD")
                    .ok()
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(0.5);
                if w[k] >= threshold {
                    let sigma = expert_sigma[k].clone().unwrap_or_else(|| {
                        robust_sigma
                            .clone()
                            .unwrap_or_else(|| vec![1.0 / n as f64; n])
                    });
                    one_hot(&sigma) // confident branch plays the mode purely
                } else {
                    mix.clone()
                }
            }
            "bayes" => {
                // bayes path is not modeled here (needs bayes blueprint + its
                // own strategy decode); return None so the caller falls back.
                return None;
            }
            "bounded" => {
                // §3.4/§3.3(5): expectation over the per-hand commitment —
                // (1−λ)·robust + λ·expert_k. The BR harness plays this
                // mixture, matching the hand-average of the deployed agent.
                let k = self.argmax_k.unwrap_or(0);
                let lam = Self::bounded_lambda(k, w[k]);
                let expert = expert_sigma[k].clone().unwrap_or_else(|| {
                    robust_sigma
                        .clone()
                        .unwrap_or_else(|| vec![1.0 / n as f64; n])
                });
                let robust_v = robust_sigma
                    .clone()
                    .unwrap_or_else(|| vec![1.0 / n as f64; n]);
                expert
                    .iter()
                    .zip(robust_v.iter())
                    .map(|(e, r)| lam * e + (1.0 - lam) * r)
                    .collect()
            }
            _ => mix,
        };
        Some(
            slots
                .iter()
                .enumerate()
                .map(|(i, s)| (s.action, dist.get(i).copied().unwrap_or(0.0)))
                .collect(),
        )
    }

    fn act_impl(&mut self, obs: &Observables<'_>, rng: &mut Rng) -> Action {
        let Self {
            mode,
            encoder,
            router: _,
            experts,
            robust,
            bayes,
            weights,
            reach,
            argmax_k,
            seq,
            hero_seat,
            tracker,
            ..
        } = self;
        // H-2 fix: record the hero's seat for this hand so on_hand_end feeds
        // the tracker the correct opponent (seat 1 - hero_seat), not 0.
        *hero_seat = Some(obs.player.as_usize());
        let slots = encoder.slots(obs, seq);
        let n = slots.len();
        let w = *weights;

        // per-expert strategies + reach products (disjoint field borrows)
        // EXP-013 R2: gather per-tier strategies WITHOUT substitution; a
        // missed tier is dropped from the mixture and the remaining weights
        // are renormalized (legacy "substitute" path kept behind
        // `mode.fallback_mode == "substitute"` for A/B).
        let legacy_substitute = mode.fallback_mode == "substitute"
            || std::env::var("CHAM_FALLBACK_MODE").as_deref() == Ok("substitute");
        let mut expert_sigma: Vec<Option<Vec<f64>>> = Vec::with_capacity(4);
        let mut expert_missed = [false; 4];
        let mut robust_missed = false;
        for k in 0..4 {
            match experts[k].strategy(obs, encoder, seq) {
                Some(s) => expert_sigma.push(Some(s)),
                None => {
                    expert_missed[k] = true;
                    expert_sigma.push(None);
                }
            }
        }
        let robust_sigma: Option<Vec<f64>> = match robust.strategy(obs, encoder, seq) {
            Some(s) => Some(s),
            None => {
                robust_missed = true;
                None
            }
        };
        let mut expert_visits = [0u32; 4];
        for k in 0..4 {
            let c = experts[k].confidence(obs, encoder, seq).unwrap_or(0.0);
            expert_visits[k] = ((c * 64.0) / (1.0 - c).max(1e-9)) as u32;
        }

        // Mixture composition.
        let mut mix: Vec<f64> = vec![0.0; n];
        #[allow(clippy::needless_late_init)]
        let mix_fallback: bool;
        #[allow(clippy::needless_late_init)]
        let reach_mass_zero: bool;
        #[allow(clippy::needless_late_init)]
        let mix_zero: bool;
        if legacy_substitute {
            // ---- legacy semantics (pre-R2): substitute + old fallback bit ----
            let mut fallback_used = false;
            let mut weight_mass = 0.0;
            for k in 0..4 {
                if w[k] <= 1e-9 {
                    continue;
                }
                let sigma = match expert_sigma[k].as_ref() {
                    Some(s) => s.clone(),
                    None => match robust_sigma.as_ref() {
                        Some(s) => {
                            fallback_used = true;
                            s.clone()
                        }
                        None => {
                            fallback_used = true;
                            vec![1.0 / n as f64; n]
                        }
                    },
                };
                let pi = reach[k];
                weight_mass += w[k] * pi;
                for a in 0..n {
                    mix[a] += w[k] * pi * sigma.get(a).copied().unwrap_or(0.0);
                }
            }
            {
                let sigma = match robust_sigma.as_ref() {
                    Some(s) => s.clone(),
                    None => {
                        fallback_used = true;
                        vec![1.0 / n as f64; n]
                    }
                };
                let pi = reach[4];
                weight_mass += w[4] * pi;
                for a in 0..n {
                    mix[a] += w[4] * pi * sigma.get(a).copied().unwrap_or(0.0);
                }
            }
            reach_mass_zero = weight_mass <= 1e-12;
            if reach_mass_zero {
                fallback_used = true;
                mix = vec![0.0; n];
                for k in 0..4 {
                    if w[k] <= 1e-9 {
                        continue;
                    }
                    let sigma: Vec<f64> = match expert_sigma[k].as_ref() {
                        Some(s) => s.clone(),
                        None => vec![1.0 / n as f64; n],
                    };
                    for a in 0..n {
                        mix[a] += w[k] * sigma.get(a).copied().unwrap_or(0.0);
                    }
                }
            }
            let mix_total: f64 = mix.iter().sum();
            mix_zero = mix_total <= 1e-12;
            if mix_zero {
                fallback_used = true;
                mix = vec![1.0 / n as f64; n];
            } else {
                for v in mix.iter_mut() {
                    *v /= mix_total;
                }
            }
            mix_fallback = fallback_used;
        } else {
            // ---- R2 semantics: DROP missed tiers, renormalize, fallback only
            // ---- when the mixture is genuinely empty (mix_zero). ----
            // Available mass (router weight × reach) over non-missed tiers.
            let mut mass = 0.0;
            for k in 0..4 {
                if w[k] <= 1e-9 || expert_sigma[k].is_none() {
                    continue;
                }
                mass += w[k] * reach[k];
            }
            if robust_sigma.is_some() {
                mass += w[4] * reach[4];
            }
            reach_mass_zero = mass <= 1e-12;
            // Any tier available at all (ignoring reach)? Determines mix_zero.
            let any_tier = (0..4).any(|k| expert_sigma[k].is_some()) || robust_sigma.is_some();
            mix_zero = !any_tier;
            if mix_zero {
                mix = vec![1.0 / n as f64; n]; // only NOW a true fallback
            } else if reach_mass_zero {
                // reach collapsed but tiers exist: plain router-weighted
                // average over AVAILABLE tiers (no reach, no uniform).
                let mut m2 = 0.0;
                for k in 0..4 {
                    if w[k] <= 1e-9 {
                        continue;
                    }
                    if let Some(s) = expert_sigma[k].as_ref() {
                        for a in 0..n {
                            mix[a] += w[k] * s.get(a).copied().unwrap_or(0.0);
                        }
                        m2 += w[k];
                    }
                }
                if let Some(s) = robust_sigma.as_ref() {
                    for a in 0..n {
                        mix[a] += w[4] * s.get(a).copied().unwrap_or(0.0);
                    }
                    m2 += w[4];
                }
                if m2 > 1e-12 {
                    for v in mix.iter_mut() {
                        *v /= m2;
                    }
                }
            } else {
                for k in 0..4 {
                    if w[k] <= 1e-9 {
                        continue;
                    }
                    if let Some(s) = expert_sigma[k].as_ref() {
                        for a in 0..n {
                            mix[a] += w[k] * reach[k] * s.get(a).copied().unwrap_or(0.0);
                        }
                    }
                }
                if let Some(s) = robust_sigma.as_ref() {
                    for a in 0..n {
                        mix[a] += w[4] * reach[4] * s.get(a).copied().unwrap_or(0.0);
                    }
                }
                let total: f64 = mix.iter().sum();
                if total > 1e-12 {
                    for v in mix.iter_mut() {
                        *v /= total;
                    }
                }
            }
            mix_fallback = mix_zero;
        }

        // mode dispatch — EXP-012 R3: trace.fallback_used comes from the
        // DECISION path (the arm actually used), not the mixture path.
        let mut tier_missed = false;
        let mut robust_covered_expert_miss = false;
        let mut action = match mode.routing.as_str() {
            "robust-only" => {
                let sigma = match robust_sigma.as_ref() {
                    Some(s) => s.clone(),
                    None => {
                        tier_missed = true;
                        vec![1.0 / n as f64; n]
                    }
                };
                slots[sample_index(&sigma, rng)].action
            }
            "argmax" => {
                let k = argmax_k.unwrap_or(0);
                let sigma = match expert_sigma[k].as_ref() {
                    Some(s) => s.clone(),
                    None => match robust_sigma.as_ref() {
                        Some(s) => {
                            tier_missed = true; // chosen expert missed, robust covered
                            s.clone()
                        }
                        None => {
                            tier_missed = true;
                            vec![1.0 / n as f64; n]
                        }
                    },
                };
                // R3 policy: argmax counts as fallback only when the decision
                // itself is uniform (both tiers missed). A robust-covered
                // expert miss is recoverable, not a fallback.
                if expert_missed[k] && robust_sigma.is_none() {
                    tier_missed = true;
                } else if expert_missed[k] && robust_sigma.is_some() {
                    tier_missed = false;
                    robust_covered_expert_miss = true;
                }
                slots[argmax_of(&sigma)].action // NO rng (replayability)
            }
            "bounded" => {
                // §3.4 bounded exploitation with a sequence-form guarantee.
                // Whole-hand commitment drawn once per hand: with prob λ
                // play the chosen expert's σ, else robust σ. SAMPLE the
                // committed σ (never the mode). Default λ = 0 (robust
                // only) until operators set CHAM_EXPLOIT_BUDGET_MB.
                let k = argmax_k.unwrap_or(0);
                let plays_expert = match self.hand_plays_expert {
                    Some(v) => v,
                    None => {
                        let conf = weights[k];
                        let lam = Self::bounded_lambda(k, conf);
                        let v = next_f64(rng) < lam;
                        self.hand_plays_expert = Some(v);
                        v
                    }
                };
                let sigma = if plays_expert {
                    match expert_sigma[k].as_ref() {
                        Some(s) => s.clone(),
                        None => match robust_sigma.as_ref() {
                            Some(s) => {
                                tier_missed = true;
                                s.clone()
                            }
                            None => {
                                tier_missed = true;
                                vec![1.0 / n as f64; n]
                            }
                        },
                    }
                } else {
                    match robust_sigma.as_ref() {
                        Some(s) => s.clone(),
                        None => {
                            tier_missed = true;
                            vec![1.0 / n as f64; n]
                        }
                    }
                };
                if expert_missed[k] && robust_sigma.is_none() {
                    tier_missed = true;
                }
                slots[sample_index(&sigma, rng)].action
            }
            "sample-expert" => {
                // 2026-10-01 (F7): same routing decision as `argmax`
                // (pick the argmax expert k), but sample that expert's
                // mixed strategy σ_k instead of playing its mode. The
                // mode-taking version is more exploitable (BR loses
                // nothing to mixing) and discards the blueprint's
                // calibrated bluff frequencies; sampling is the
                // equilibrium-correct deployment. Kept as a separate
                // routing mode so existing measurements using `argmax`
                // stay valid; A/B via the ladder.
                let k = argmax_k.unwrap_or(0);
                let sigma = match expert_sigma[k].as_ref() {
                    Some(s) => s.clone(),
                    None => match robust_sigma.as_ref() {
                        Some(s) => {
                            tier_missed = true;
                            s.clone()
                        }
                        None => {
                            tier_missed = true;
                            vec![1.0 / n as f64; n]
                        }
                    },
                };
                if expert_missed[k] && robust_sigma.is_none() {
                    tier_missed = true;
                } else if expert_missed[k] && robust_sigma.is_some() {
                    tier_missed = false;
                }
                slots[sample_index(&sigma, rng)].action
            }
            "bayes" => {
                let sigma = match bayes {
                    Some(bp) => match bp.strategy(obs, encoder, seq) {
                        Some(s) => s,
                        None => {
                            tier_missed = true;
                            vec![1.0 / n as f64; n]
                        }
                    },
                    None => {
                        tier_missed = true;
                        vec![1.0 / n as f64; n]
                    }
                };
                slots[argmax_of(&sigma)].action // bayes-greedy consumes NO rng
            }
            "hedged" => {
                // PERF (2026-09-29): hedge on ROUTER CONFIDENCE, not on
                // archetype identity. If the router is confident (top
                // weight above a threshold), play that expert purely. If
                // not, fall back to the mixture (which is a hedge against
                // exactly the case the router can't decide).
                //
                // The motivation: argmax beats mixture on the ladder
                // because mixture averages 3 wrong picks when the router
                // is wrong. But argmax commits hard even when the router
                // is uncertain. Hedged routing takes argmax's win when
                // confident and mixture's hedge when not.
                //
                // Threshold via CHAM_HEDGE_THRESHOLD (default 0.5).
                let threshold = std::env::var("CHAM_HEDGE_THRESHOLD")
                    .ok()
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(0.5);
                let top = argmax_k.unwrap_or(0);
                let top_weight = weights[top];
                if crate::pipeline::hedge_debug_enabled() {
                    eprintln!(
                        "hedged: top={top} top_weight={top_weight:.6} threshold={threshold:.3}",
                    );
                }
                if top_weight >= threshold {
                    // confident: play the top expert purely
                    let sigma = match expert_sigma[top].as_ref() {
                        Some(s) => s.clone(),
                        None => robust_sigma
                            .as_ref()
                            .cloned()
                            .unwrap_or_else(|| vec![1.0 / n as f64; n]),
                    };
                    tier_missed = expert_missed[top] && robust_sigma.is_none();
                    slots[argmax_of(&sigma)].action
                } else {
                    // uncertain: fall back to the mixture
                    tier_missed = mix_fallback;
                    slots[sample_index(&mix, rng)].action
                }
            }
            _ => {
                tier_missed = mix_fallback; // mixture modes keep mixture bit
                slots[sample_index(&mix, rng)].action // mixture
            }
        };
        let fallback_used = tier_missed;

        // per-expert reach update: π_k *= σ_k(a_chosen | i)
        // PERF (v5-deepdive-audit item 1): reuse expert_sigma/robust_sigma
        // computed above instead of recomputing — obs/encoder/seq are
        // unchanged since those were computed (encoder.record happens
        // below, after this block), so the values are byte-identical;
        // this removes 5 redundant enc.key() + policy-decode + Vec<f64>
        // allocations per decision.
        // 2026-10-01 (F1): live river search. Opt-in via `AgentMode.search.enabled`
        // (default OFF in every shipped bundle; the `SearchCfg` lockout refuses
        // enabled search unless the caller opts in explicitly). Runs BEFORE
        // `chosen_slot` is computed so the reach update and trace stay in sync
        // with the finalized action.
        let mut search_trace: Option<(String, bool, String, u32, bool, f64)> = None;
        if let Some(cfg) = crate::search_bridge::SearchBridgeCfg::from_mode(mode) {
            if crate::search_bridge::would_trigger(&cfg, obs) {
                match crate::search_bridge::try_solve(&cfg, tracker, encoder, robust, obs, seq) {
                    Some(outcome) => {
                        // §3.1: the solver emits an AVERAGE strategy — sample
                        // it, never take its mode. EXCEPT under the
                        // CHAM_SEARCH_ARGMAX=1 diagnostic (2026-10-05): take
                        // the mode instead, to test whether the ON-vs-OFF loss
                        // is the SAMPLING of a correct mixed strategy scored
                        // against a pure-best-response-to-bots metric, rather
                        // than a wrong range.
                        let probs: Vec<f64> = outcome.distribution.iter().map(|x| x.1).collect();
                        action = if probs.is_empty() {
                            outcome.action
                        } else if search_argmax_enabled() {
                            outcome.distribution[argmax_of(&probs)].0
                        } else {
                            outcome.distribution[sample_index(&probs, rng)].0
                        };
                        search_trace = Some((
                            outcome.solver,
                            true,
                            "live".into(),
                            outcome.iters,
                            outcome.truncated,
                            outcome.lbr,
                        ));
                    }
                    None => {
                        search_trace = Some((
                            format!("{:?}", cfg.solver),
                            true,
                            "attempt".into(),
                            0,
                            true,
                            0.0,
                        ));
                    }
                }
            }
        }

        // per-expert reach update: π_k *= σ_k(a_chosen | i)
        let chosen_slot = slots.iter().position(|s| s.action == action).unwrap_or(0);
        for k in 0..4 {
            if w[k] <= 1e-9 {
                continue;
            }
            if let Some(sigma) = expert_sigma[k].as_ref() {
                reach[k] *= sigma.get(chosen_slot).copied().unwrap_or(0.0);
            }
        }
        if let Some(sigma) = robust_sigma.as_ref() {
            reach[4] *= sigma.get(chosen_slot).copied().unwrap_or(0.0);
        }

        // record our own action into the canonical seq
        encoder.record(obs, obs.player, action, seq);

        // trace
        let t = DecisionTrace {
            hand_idx: self.hand_idx,
            street: obs.street.as_u8(),
            slot: chosen_slot,
            action: action.to_str(),
            weights_frozen: *weights,
            argmax_k: *argmax_k,
            search: search_trace.clone(),
            expert_visits,
            fallback_used,
            abstraction_hash: encoder.abstraction_hash(),
            expert_missed,
            robust_missed,
            reach_mass_zero,
            mix_zero,
            expert_missed_robust_covered: robust_covered_expert_miss,
        };
        let _ = trace_record(self.recorder.as_mut(), "agent", &t);
        self.last_trace = Some(t);
        action
    }
}

/// Diagnostic (2026-10-05): when `CHAM_SEARCH_ARGMAX=1`, the search bridge
/// takes the solver's modal action instead of sampling its mixed strategy.
/// Used to isolate "search loses because it samples a correct mixed
/// strategy" from "search loses because the villain range is wrong".
fn search_argmax_enabled() -> bool {
    use std::sync::OnceLock;
    static F: OnceLock<bool> = OnceLock::new();
    *F.get_or_init(|| std::env::var("CHAM_SEARCH_ARGMAX").as_deref() == Ok("1"))
}

fn argmax_of(p: &[f64]) -> usize {
    let mut best = 0usize;
    for (i, &v) in p.iter().enumerate() {
        if v > p[best] {
            best = i;
        }
    }
    best
}

/// One-hot on the modal action (§3.3(5)): what the deployed pure-strategy
/// (`argmax`) agent actually plays. Ties break to the first index,
/// matching `argmax_of`.
fn one_hot(p: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0f64; p.len()];
    if !out.is_empty() {
        out[argmax_of(p)] = 1.0;
    }
    out
}

fn sample_index(probs: &[f64], rng: &mut Rng) -> usize {
    // Bug hunt pass 3 (2026-10-02): mirror the L-7 guard that landed in
    // cham-blueprint::traversal::sample_index. This copy was missed, so
    // an empty distribution underflowed `probs.len() - 1` here too.
    if probs.is_empty() {
        debug_assert!(
            false,
            "sample_index called with an empty distribution - a caller bug"
        );
        return 0;
    }
    let u = next_f64(rng);
    let mut acc = 0.0;
    for (i, p) in probs.iter().enumerate() {
        acc += p;
        if u <= acc {
            return i;
        }
    }
    probs.len() - 1
}

impl Agent for ChameleonAgent {
    fn name(&self) -> &str {
        "chameleon"
    }

    fn act(&mut self, obs: &Observables<'_>, rng: &mut Rng) -> Action {
        self.start_hand_if_needed();
        self.act_impl(obs, rng)
    }

    /// Public action feed (driver calls for EVERY action with the PRE-action view;
    /// our own actions are re-recorded idempotently by the driver path too — the
    /// encoder's record is deterministic per (obs, actor, action), so duplicates
    /// must be avoided: the driver skips the actor's own feed when it is us).
    fn on_public_action(&mut self, obs: &Observables<'_>, player: Player, action: Action) {
        // our own actions are recorded in act(); skip re-feeding them here
        if player == obs.player {
            return;
        }
        // F6c (2026-10-01): when enabled, translate off-tree aggressive
        // sizes into a same-class abstract slot BEFORE recording, so the
        // encoded key matches what the training tree produced. The
        // pseudo-harmonic mapping uses a deterministic u derived from
        // (hand_idx, street, seq.lens), so replay stays bit-exact.
        // Default off (see `offtree_translate_enabled`).
        let recorded = if offtree_translate_enabled() {
            let u = derive_translate_u(self.hand_idx, obs.street.as_u8(), self.seq.lens);
            self.encoder.ladder.translate(obs, &self.seq, action, u)
        } else {
            action
        };
        self.encoder.record(obs, player, recorded, &mut self.seq);
    }

    fn on_hand_end(&mut self, ph: &cham_core::engine::PublicHistory, hero_net: i64) {
        // H-2 fix (2026-09-27): pass the ACTUAL hero seat recorded during the
        // hand. The previous hardcoded `0` made the tracker model the agent
        // itself in every seat-1 seating of duplicate matching. Default to 0
        // only if `act` was never called (empty hand, shouldn't happen).
        let hero_seat = self.hero_seat.take().unwrap_or(0);
        self.tracker.observe_hand(ph, hero_net, hero_seat);
        self.hand_idx += 1;
        self.weights_fresh = false; // next hand re-derives weights
        self.seq = ActionSeq::default();
    }
}

#[cfg(test)]
mod translate_u_tests {
    use super::derive_translate_u;

    /// Output must lie in [0, 1). The derivation shifts off the top 53
    /// bits of a u64 and divides by 2^53, so the maximum value is
    /// (2^53 - 1) / 2^53 < 1.
    #[test]
    fn u_is_in_unit_interval() {
        for hand in [0u64, 1, 7, 1_000_000, u64::MAX] {
            for street in 0u8..4 {
                for lens in [[0u8; 4], [1, 0, 0, 0], [8, 8, 8, 8], [255; 4]] {
                    let u = derive_translate_u(hand, street, lens);
                    assert!(
                        (0.0..1.0).contains(&u),
                        "u={u} out of range for ({hand},{street},{lens:?})"
                    );
                }
            }
        }
    }

    /// Purity: same inputs → same output, every time.
    #[test]
    fn u_is_deterministic() {
        let a = derive_translate_u(42, 2, [3, 1, 0, 0]);
        let b = derive_translate_u(42, 2, [3, 1, 0, 0]);
        assert_eq!(a.to_bits(), b.to_bits(), "not bit-identical");
    }

    /// Sensitivity: changing any one input must change the output.
    /// A weak hash that ignored a field would silently break replay
    /// determinism per (hand, street, seq).
    #[test]
    fn u_is_sensitive_to_each_input() {
        let base = derive_translate_u(100, 1, [2, 2, 0, 0]);
        assert_ne!(
            base.to_bits(),
            derive_translate_u(101, 1, [2, 2, 0, 0]).to_bits(),
            "hand_idx"
        );
        assert_ne!(
            base.to_bits(),
            derive_translate_u(100, 2, [2, 2, 0, 0]).to_bits(),
            "street"
        );
        assert_ne!(
            base.to_bits(),
            derive_translate_u(100, 1, [3, 2, 0, 0]).to_bits(),
            "lens[0]"
        );
        assert_ne!(
            base.to_bits(),
            derive_translate_u(100, 1, [2, 3, 0, 0]).to_bits(),
            "lens[1]"
        );
        assert_ne!(
            base.to_bits(),
            derive_translate_u(100, 1, [2, 2, 1, 0]).to_bits(),
            "lens[2]"
        );
        assert_ne!(
            base.to_bits(),
            derive_translate_u(100, 1, [2, 2, 0, 1]).to_bits(),
            "lens[3]"
        );
    }

    /// Rough uniformity: over many consecutive hand indices the outputs
    /// should not clump. A coarse histogram with 10 bins should have
    /// every bin non-empty for 10000 samples.
    #[test]
    fn u_is_roughly_uniform() {
        let mut bins = [0u32; 10];
        for hand in 0..10_000u64 {
            let u = derive_translate_u(hand, 0, [0, 0, 0, 0]);
            let b = ((u * 10.0) as usize).min(9);
            bins[b] += 1;
        }
        for (i, c) in bins.iter().enumerate() {
            assert!(*c > 0, "bin {i} empty (clumping): {bins:?}");
        }
    }
}
