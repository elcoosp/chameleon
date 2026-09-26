//! Router runtime (SPECS/05 §5): per-hand sharpened weights with Bayesian
//! session fusion (v3 §5.2) and the drift shield. Weights are FROZEN for the
//! whole hand (review A7-2).

use serde::{Deserialize, Serialize};

use crate::RouterError;
use crate::model::SoftmaxModel;

pub const N_EXPERTS: usize = 5; // 4 specialists + robust

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
    /// last hand's posterior variance per specialist (B2 confidence gate input)
    last_post_var: [f64; N_EXPERTS - 1],
    /// last hand's weights (trajectory/debug; reset per session)
    w_prev: [f64; N_EXPERTS],
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
        }
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
        let n0 = self.prior_strength.max(1e-9);
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
    }

    pub fn from_model_bytes(model_bytes: &[u8]) -> Result<RouterRuntime, RouterError> {
        let model: SoftmaxModel = serde_json::from_slice(model_bytes)?;
        model.validate()?;
        Ok(RouterRuntime::new(model, 0.7, 8.0, 0.5, -1.5))
    }
}
