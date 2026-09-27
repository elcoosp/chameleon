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
            last_trace: None,
        })
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
        let features = cham_router::features::from_inputs(&inputs)
            .map(|f: cham_engine::RouterFeatures| f.0)
            .unwrap_or([0.5; 20]);
        let trend_z = self.tracker.trend_z();
        self.weights = self.router.weights_for_hand(&features, trend_z);
        self.reach = [1.0; 5];
        self.argmax_k = if self.mode.routing == "argmax" {
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
        let action = match mode.routing.as_str() {
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
                }
                slots[argmax_of(&sigma)].action // NO rng (replayability)
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
            search: None,
            expert_visits,
            fallback_used,
            abstraction_hash: encoder.abstraction_hash(),
            expert_missed,
            robust_missed,
            reach_mass_zero,
            mix_zero,
        };
        let _ = trace_record(self.recorder.as_mut(), "agent", &t);
        self.last_trace = Some(t);
        action
    }
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

fn sample_index(probs: &[f64], rng: &mut Rng) -> usize {
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
        self.encoder.record(obs, player, action, &mut self.seq);
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
