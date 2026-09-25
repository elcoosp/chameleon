//! Router runtime (SPECS/05 §5): per-hand sharpened weights with hysteresis and
//! the drift shield. Weights are FROZEN for the whole hand (review A7-2).

use serde::{Deserialize, Serialize};

use crate::model::SoftmaxModel;
use crate::RouterError;

pub const N_EXPERTS: usize = 5; // 4 specialists + robust

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouterRuntime {
    pub model: SoftmaxModel,
    /// sharpening temperature T (default 0.7 — T < 1 SHARPENS)
    pub temp: f64,
    /// hand-level hysteresis α (default 0.3)
    pub alpha_hand: f64,
    /// shield: blend β toward robust when trend_z < shield_z
    pub shield_beta: f64,
    pub shield_z: f64,
    /// last hand's weights (for hysteresis; reset per session)
    w_prev: [f64; N_EXPERTS],
}

impl RouterRuntime {
    pub fn new(model: SoftmaxModel, temp: f64, alpha_hand: f64, shield_beta: f64, shield_z: f64) -> RouterRuntime {
        RouterRuntime {
            model,
            temp,
            alpha_hand,
            shield_beta,
            shield_z,
            w_prev: [0.0; N_EXPERTS],
        }
    }

    /// Called ONCE PER HAND (hand start) with the hand-frozen features; result is
    /// FROZEN for the whole hand by the caller (cham-agent). Weights:
    ///
    /// ```text
    /// p      = model.forward(features)
    /// w_inst = normalize(p_i^(1/T))              // SHARPENING (p^(1/0.7) ≠ softmax)
    /// w      = α·w_inst + (1−α)·w_prev_hand      // hand-to-hand hysteresis
    /// shield: if trend_z < shield_z: w = (1−β)·w + β·e_robust
    /// ```
    /// Returns [f64; 5]: four archetype weights + robust weight (robust enters via
    /// the shield here and via confidence-gated fallback at decision time).
    pub fn weights_for_hand(&mut self, features: &[f32; 20], trend_z: f64) -> [f64; N_EXPERTS] {
        let p = self.model.forward(features);
        // sharpening: w ∝ p^(1/T)
        let mut w_inst = [0f64; 4];
        let mut total = 0.0;
        for (k, &pk) in p.iter().take(4).enumerate() {
            let sharpened = pk.powf(1.0 / self.temp);
            w_inst[k] = sharpened;
            total += sharpened;
        }
        if total <= 1e-12 {
            w_inst = [0.25; 4];
        } else {
            for v in w_inst.iter_mut() {
                *v /= total;
            }
        }
        // hysteresis (hand-to-hand)
        let mut w = [0f64; N_EXPERTS];
        for k in 0..4 {
            w[k] = self.alpha_hand * w_inst[k] + (1.0 - self.alpha_hand) * self.w_prev[k];
        }
        // shield: blend toward robust on negative EV trend
        if trend_z < self.shield_z {
            let b = self.shield_beta;
            for k in 0..4 {
                w[k] *= 1.0 - b;
            }
            w[4] += b;
        }
        // renormalize to a proper 5-simplex point (the hysteresis blend starts at
        // w_prev = 0 on hand 1, so the raw blend under-sums)
        let total: f64 = w.iter().sum();
        if total > 1e-12 {
            for v in w.iter_mut() {
                *v /= total;
            }
        }
        self.w_prev = w;
        w
    }

    /// Reset per-session hysteresis (v1: reset-per-session — tested).
    pub fn reset_session(&mut self) {
        self.w_prev = [0.0; N_EXPERTS];
    }

    pub fn from_model_bytes(model_bytes: &[u8]) -> Result<RouterRuntime, RouterError> {
        let model: SoftmaxModel = serde_json::from_slice(model_bytes)?;
        model.validate()?;
        Ok(RouterRuntime::new(model, 0.7, 0.3, 0.5, -1.5))
    }
}
