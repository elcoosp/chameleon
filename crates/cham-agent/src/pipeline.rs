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
use cham_core::rng::{next_f64, Rng};
use cham_engine::encoder::{ActionSeq, Encoder};
use cham_rec::Recorder;

use crate::modes::AgentMode;
use crate::trace::{record as trace_record, DecisionTrace};
use crate::tracker::Tracker;
use crate::AgentError;

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
            return Err(AgentError::Pipeline("exactly 4 specialists required".into()));
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
            ..
        } = self;
        let n_slots_at_decision = encoder.slots(obs, seq).len();
        let slots = encoder.slots(obs, seq);
        let n = n_slots_at_decision.max(slots.len());
        let w = *weights;

        // per-expert strategies + reach products (disjoint field borrows)
        let mut mix: Vec<f64> = vec![0.0; n];
        let mut weight_mass = 0.0;
        let mut fallback_used = false;
        let mut expert_visits = [0u32; 4];

        for k in 0..4 {
            if w[k] <= 1e-9 {
                continue;
            }
            let sigma = match experts[k].strategy(obs, encoder, seq) {
                Some(s) => s,
                None => match robust.strategy(obs, encoder, seq) {
                    Some(s) => {
                        fallback_used = true;
                        s
                    }
                    None => vec![1.0 / n as f64; n],
                },
            };
            let pi = reach[k];
            weight_mass += w[k] * pi;
            for a in 0..n {
                mix[a] += w[k] * pi * sigma.get(a).copied().unwrap_or(0.0);
            }
            let c = experts[k].confidence(obs, encoder, seq).unwrap_or(0.0);
            expert_visits[k] = ((c * 64.0) / (1.0 - c).max(1e-9)) as u32;
        }
        // robust expert (weight w[4])
        {
            let sigma = match robust.strategy(obs, encoder, seq) {
                Some(s) => s,
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
        // reach-weighted fallback: Σ w_k π_k = 0 → plain weighted average of σ_k
        if weight_mass <= 1e-12 {
            fallback_used = true;
            mix = vec![0.0; n];
            // re-derive plain σ averages without reach
            for k in 0..4 {
                if w[k] <= 1e-9 {
                    continue;
                }
                let sigma = match experts[k].strategy(obs, encoder, seq) {
                    Some(s) => s,
                    None => vec![1.0 / n as f64; n],
                };
                for a in 0..n {
                    mix[a] += w[k] * sigma.get(a).copied().unwrap_or(0.0);
                }
            }
        }
        let mix_total: f64 = mix.iter().sum();
        if mix_total <= 1e-12 {
            fallback_used = true;
            mix = vec![1.0 / n as f64; n];
        } else {
            for v in mix.iter_mut() {
                *v /= mix_total;
            }
        }

        // mode dispatch
        let action = match mode.routing.as_str() {
            "robust-only" => {
                let sigma = robust
                    .strategy(obs, encoder, seq)
                    .unwrap_or_else(|| vec![1.0 / n as f64; n]);
                slots[sample_index(&sigma, rng)].action
            }
            "argmax" => {
                let k = argmax_k.unwrap_or(0);
                let sigma = match experts[k].strategy(obs, encoder, seq) {
                    Some(s) => s,
                    None => robust
                        .strategy(obs, encoder, seq)
                        .unwrap_or_else(|| vec![1.0 / n as f64; n]),
                };
                slots[argmax_of(&sigma)].action // NO rng (replayability)
            }
            "bayes" => {
                let sigma = match bayes {
                    Some(bp) => bp.strategy(obs, encoder, seq).unwrap_or_else(|| vec![1.0 / n as f64; n]),
                    None => vec![1.0 / n as f64; n],
                };
                slots[argmax_of(&sigma)].action // bayes-greedy consumes NO rng
            }
            _ => slots[sample_index(&mix, rng)].action, // mixture
        };

        // per-expert reach update: π_k *= σ_k(a_chosen | i)
        let chosen_slot = slots.iter().position(|s| s.action == action).unwrap_or(0);
        for k in 0..4 {
            if w[k] <= 1e-9 {
                continue;
            }
            let sigma = match experts[k].strategy(obs, encoder, seq) {
                Some(s) => s,
                None => continue,
            };
            let p = sigma.get(chosen_slot).copied().unwrap_or(0.0);
            reach[k] *= p;
        }
        let robust_p = robust
            .strategy(obs, encoder, seq)
            .and_then(|s| s.get(chosen_slot).copied())
            .unwrap_or(0.0);
        reach[4] *= robust_p;

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
        self.tracker.observe_hand(ph, hero_net, 0);
        self.hand_idx += 1;
        self.weights_fresh = false; // next hand re-derives weights
        self.seq = ActionSeq::default();
    }
}
