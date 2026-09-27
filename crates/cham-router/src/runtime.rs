//! Router runtime (SPECS/05 §5): per-hand sharpened weights with Bayesian
//! session fusion (v3 §5.2) and the drift shield. Weights are FROZEN for the
//! whole hand (review A7-2).

use serde::{Deserialize, Serialize};

use crate::RouterError;
use crate::model::SoftmaxModel;

pub const N_EXPERTS: usize = 5; // 4 specialists + robust

/// Process-wide opt-in for the changepoint shield (v7 Item 6): set by the
/// `--router-changepoint-shield` CLI flag via [`enable_changepoint_global`]
/// so the flag threads through without `std::env::set_var` (which is
/// `unsafe` in this toolchain and forbidden by the crate's `#![forbid]`).
/// `RouterRuntime::new` enables the shield when this OR the
/// `CHAM_ROUTER_CHANGEPOINT=1` env var is set.
pub static CHANGEPOINT_FORCE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Enable the changepoint shield process-wide (CLI flag path).
pub fn enable_changepoint_global() {
    CHANGEPOINT_FORCE.store(true, std::sync::atomic::Ordering::SeqCst);
}

fn changepoint_requested() -> bool {
    if CHANGEPOINT_FORCE.load(std::sync::atomic::Ordering::SeqCst) {
        return true;
    }
    std::env::var("CHAM_ROUTER_CHANGEPOINT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("on"))
        .unwrap_or(false)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouterRuntime {
    pub model: SoftmaxModel,
    /// sharpening temperature T (default 0.7 — T < 1 SHARPENS)
    pub temp: f64,
    /// Dirichlet prior strength N0 (default 8.0). The sharpened softmax is
    /// the prior MEAN; N0 is how many hands of evidence it is worth. Larger
    /// N0 = slower to move off the model (graceful on weak evidence);
    /// smaller N0 = faster concentration on repeated votes.
    pub prior_strength: f64,
    /// shield: blend β toward robust when trend_z < shield_z
    pub shield_beta: f64,
    pub shield_z: f64,
    /// session vote counts per specialist (reset per session)
    dir_counts: [f64; N_EXPERTS - 1],
    /// v7 Item 6: Bayesian online changepoint shield (Adams & MacKay 2007).
    /// `None` = disabled (historical fixed-N0 behavior, bit-identical).
    #[serde(default)]
    changepoint: Option<ChangepointShield>,
    /// last hand's posterior variance per specialist (B2 confidence gate input)
    last_post_var: [f64; N_EXPERTS - 1],
    /// last hand's weights (trajectory/debug; reset per session)
    w_prev: [f64; N_EXPERTS],
}

/// Bayesian online changepoint shield (v7 Item 6 / B-8, Adams & MacKay
/// 2007-style run-length posterior) layered on the Dirichlet fusion.
///
/// The stationary Dirichlet model assumes one opponent type per session; a
/// switching manipulator violates that by construction, and no fixed N0 can
/// distinguish "noisy but stationary" from "just switched" after the fact.
/// This shield tracks P(run length = r | evidence): mass concentrating near
/// r=0 means "the type just changed" → decay accumulated counts faster via
/// [`ChangepointShield::effective_n0`]. O(1) amortized (history truncated at
/// 200 hands). Deterministic (fixed arithmetic, no RNG).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ChangepointShield {
    /// prior P(type-switch per hand), e.g. 1/200.
    pub hazard_rate: f64,
    /// P(run length = r | evidence), index = run length (truncated).
    run_length_posterior: Vec<f64>,
    /// last hand's per-archetype predictive log-likelihood (for tests/debug).
    last_loglik: Vec<f64>,
    /// argmax vote of the last update (switch detection).
    last_vote: Option<usize>,
}

impl Default for ChangepointShield {
    fn default() -> Self {
        Self::new(1.0 / 200.0)
    }
}

impl ChangepointShield {
    pub fn new(hazard_rate: f64) -> Self {
        let h = hazard_rate.clamp(1e-6, 0.5);
        let mut rl = vec![0.0; 200];
        rl[0] = 1.0; // session starts with run length 0 with certainty
        ChangepointShield { hazard_rate: h, run_length_posterior: rl, last_loglik: Vec::new(), last_vote: None }
    }

    /// Run-length recursion update on this hand's per-archetype evidence
    /// log-likelihoods (one per specialist, any scale — normalized inside).
    pub fn update(&mut self, hand_evidence_loglik: &[f64]) {
        if hand_evidence_loglik.is_empty() { return; }
        // predictive likelihoods from log-scale (softmax-normalized)
        let m = hand_evidence_loglik.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mut lik: Vec<f64> =
            hand_evidence_loglik.iter().map(|&l| (l - m).exp()).collect();
        let s: f64 = lik.iter().sum();
        if s > 0.0 { for v in lik.iter_mut() { *v /= s; } }
        // archetype-marginal likelihood for growth vs reset: use the max
        // (best-explaining type) so a clean switch still registers.
        let best = lik.iter().copied().fold(0.0f64, f64::max).max(1e-9);
        let h = self.hazard_rate;
        let n = self.run_length_posterior.len();
        let mut next = vec![0.0; n];
        // reset: P(r=0) ∝ hazard * Σ_r P(r) * lik
        let total: f64 = self.run_length_posterior.iter().sum();
        next[0] = h * total * best;
        // growth: P(r+1) ∝ (1-hazard) * P(r) * lik
        for r in 0..n - 1 {
            next[r + 1] = (1.0 - h) * self.run_length_posterior[r] * best;
        }
        // evidence-sharpening: when the vote disagrees with the accumulated
        // posterior mode, boost the reset mass (the "surprise" signal).
        // Switch detector: a change in the argmax vote means the opponent's
        // apparent type flipped — concentrate mass at run-length 0 so
        // effective_n0() drops and the Dirichlet counts decay faster.
        let vote = lik
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i);
        if let (Some(v), Some(lv)) = (vote, self.last_vote) {
            if v != lv {
                // repeated disagreement compounds: each switched-vote hand
                // moves ~half the mass to r=0, so ~5 hands saturate.
                let move_mass: f64 = self.run_length_posterior.iter().skip(5).sum::<f64>() * 0.5;
                next[0] += move_mass;
                for r in 5..n {
                    next[r] *= 0.5;
                }
            }
        }
        self.last_vote = vote;
        let tot: f64 = next.iter().sum();
        if tot > 0.0 { for v in next.iter_mut() { *v /= tot; } }
        self.run_length_posterior = next;
        self.last_loglik = hand_evidence_loglik.to_vec();
    }

    /// Effective Dirichlet prior strength: sharpen (lower N0) when
    /// run-length mass concentrates near 0. Floored at 0.1× so it never
    /// fully forgets.
    pub fn effective_n0(&self, base_n0: f64) -> f64 {
        let p_recent: f64 = self.run_length_posterior.iter().take(5).sum();
        base_n0 * (1.0 - p_recent).max(0.1)
    }

    /// P(run length < 5) — the "just switched" probability (tests/debug).
    pub fn p_recent_change(&self) -> f64 {
        self.run_length_posterior.iter().take(5).sum()
    }
}

impl RouterRuntime {
    pub fn new(
        model: SoftmaxModel,
        temp: f64,
        prior_strength: f64,
        shield_beta: f64,
        shield_z: f64,
    ) -> RouterRuntime {
        RouterRuntime {
            model,
            temp,
            prior_strength,
            shield_beta,
            shield_z,
            dir_counts: [0.0; N_EXPERTS - 1],
            last_post_var: [0.0; N_EXPERTS - 1],
            w_prev: [0.0; N_EXPERTS],
            changepoint: changepoint_requested().then(ChangepointShield::default),
        }
    }

    /// Enable the changepoint shield explicitly (v7 Item 6; also enabled
    /// via `CHAM_ROUTER_CHANGEPOINT=1`). Chainable for the EXP-015 A/B.
    pub fn with_changepoint_shield(mut self, hazard_rate: f64) -> Self {
        self.changepoint = Some(ChangepointShield::new(hazard_rate));
        self
    }

    pub fn changepoint_enabled(&self) -> bool {
        self.changepoint.is_some()
    }

    /// Called ONCE PER HAND (hand start) with the hand-frozen features; result is
    /// FROZEN for the whole hand by the caller (cham-agent). Weights (v3 §5.2:
    /// proper Bayesian fusion replaces the ad hoc fixed-α smoother):
    ///
    /// ```text
    /// p      = model.forward(features)
    /// prior  = normalize(p_i^(1/T))              // SHARPENING (p^(1/0.7) ≠ softmax)
    /// α_k    = N0·prior_k + c_k                  // Dirichlet: prior pseudo-counts
    ///                                            // + session votes so far
    /// w_k    = α_k / Σ_j α_j  (k < 4)             // posterior mean
    /// c_o   += 1  where o = argmax(prior)        // this hand's vote informs the
    ///                                            // NEXT hand (hand 1 = pure prior)
    /// shield: if trend_z < shield_z: w = (1−β)·w + β·e_robust
    /// ```
    ///
    /// vs the old smoother: repeated consistent votes CONCENTRATE the
    /// posterior toward the voted specialist (fixed-α hysteresis asymptotes
    /// to a blend and can never concentrate beyond single-hand evidence);
    /// contradictory or weak evidence leaves the prior dominant (graceful
    /// degradation). The posterior variance ([`Self::posterior_variance`])
    /// is the B2 confidence-gate threshold — a real number, not a tuned
    /// constant.
    ///
    /// Returns [f64; 5]: four archetype weights + robust weight (robust enters via
    /// the shield here and via confidence-gated fallback at decision time).
    pub fn weights_for_hand(&mut self, features: &[f32; 20], trend_z: f64) -> [f64; N_EXPERTS] {
        let p = self.model.forward(features);
        // sharpening: prior ∝ p^(1/T)
        let mut prior = [0f64; 4];
        let mut total = 0.0;
        for (k, &pk) in p.iter().take(4).enumerate() {
            let sharpened = pk.powf(1.0 / self.temp);
            prior[k] = sharpened;
            total += sharpened;
        }
        if total <= 1e-12 {
            prior = [0.25; 4];
        } else {
            for v in prior.iter_mut() {
                *v /= total;
            }
        }
        // Dirichlet-multinomial posterior: pseudo-counts + session votes.
        // v7 Item 6: the changepoint shield (when enabled) replaces the
        // fixed N0 with effective_n0() — sharpens adaptation right after a
        // detected type switch, matches base N0 when stationary.
        let mut n0 = self.prior_strength.max(1e-9);
        if let Some(cp) = self.changepoint.as_mut() {
            // per-archetype evidence: log of the sharpened prior shares
            let ll: Vec<f64> = prior.iter().map(|&p| p.max(1e-9).ln()).collect();
            cp.update(&ll);
            n0 = cp.effective_n0(self.prior_strength.max(1e-9));
        }
        let c_total: f64 = self.dir_counts.iter().sum();
        let a0 = n0 + c_total;
        let mut w = [0f64; N_EXPERTS];
        for k in 0..4 {
            let a_k = n0 * prior[k] + self.dir_counts[k];
            w[k] = a_k / a0;
            // Dirichlet marginal variance: the B2 gate input.
            self.last_post_var[k] = if a0 > 0.0 {
                a_k * (a0 - a_k) / (a0 * a0 * (a0 + 1.0))
            } else {
                0.0
            };
        }
        // Sequential update: this hand's classification vote informs the next hand.
        let mut vote = 0usize;
        for k in 1..4 {
            if prior[k] > prior[vote] {
                vote = k;
            }
        }
        self.dir_counts[vote] += 1.0;
        // shield: blend toward robust on negative EV trend
        if trend_z < self.shield_z {
            let b = self.shield_beta;
            for k in 0..4 {
                w[k] *= 1.0 - b;
            }
            w[4] += b;
        }
        // renormalize to a proper 5-simplex point
        let total: f64 = w.iter().sum();
        if total > 1e-12 {
            for v in w.iter_mut() {
                *v /= total;
            }
        }
        self.w_prev = w;
        w
    }

    /// Posterior variance per specialist from the last [`Self::weights_for_hand`]
    /// call (Dirichlet marginals). B2's confidence gate thresholds on
    /// `max(posterior_variance())`: high variance = the session hasn't
    /// identified the villain yet = route conservatively (robust-leaning).
    pub fn posterior_variance(&self) -> [f64; N_EXPERTS - 1] {
        self.last_post_var
    }

    /// Reset per-session state (votes, variance, trajectory).
    pub fn reset_session(&mut self) {
        self.dir_counts = [0.0; N_EXPERTS - 1];
        self.last_post_var = [0.0; N_EXPERTS - 1];
        self.w_prev = [0.0; N_EXPERTS];
        if let Some(cp) = self.changepoint.as_mut() {
            *cp = ChangepointShield::new(cp.hazard_rate);
        }
    }

    pub fn from_model_bytes(model_bytes: &[u8]) -> Result<RouterRuntime, RouterError> {
        let model: SoftmaxModel = serde_json::from_slice(model_bytes)?;
        model.validate()?;
        Ok(RouterRuntime::new(model, 0.7, 8.0, 0.5, -1.5))
    }
}
