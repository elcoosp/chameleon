//! Softmax model (K=4 archetypes, N=20 features) + minibatch SGD — pure Rust,
//! ~200 LOC, no linear-algebra deps (SPECS/05 §3).

use serde::{Deserialize, Serialize};

use crate::RouterError;

/// Which feature vector this model was trained against. Used by the
/// agent pipeline to dispatch the right feature constructor: a
/// `raw-opponent-19` model expects 19 opponent-only features, an
/// `opportunity-gated-20` model (the default, and every historical
/// artifact) expects the 20-dim tracker vector.
///
/// Default is `opportunity-gated-20` so all pre-2026-09-30 artifacts
/// load with their historical semantics. See
/// `docs/plans/ROUTER-INTEGRATION-DESIGN-2026-09-30.md`.
pub fn default_feature_set() -> String {
    "opportunity-gated-20".to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SoftmaxModel {
    /// weights[k][n]
    pub weights: Vec<Vec<f64>>,
    pub bias: Vec<f64>,
    pub n_features: usize,
    pub n_classes: usize,
    /// Feature set this model was trained against. Defaulted for
    /// backward compatibility with pre-2026-09-30 artifacts.
    #[serde(default = "default_feature_set")]
    pub feature_set: String,
}

impl SoftmaxModel {
    pub fn new(n_features: usize, n_classes: usize) -> SoftmaxModel {
        // deterministic small init (seeded externally via `init_seeded` if needed)
        let mut w = Vec::with_capacity(n_classes);
        for k in 0..n_classes {
            let mut row = vec![0.0; n_features];
            for (i, v) in row.iter_mut().enumerate() {
                *v = ((k + 1) as f64 * 0.01) * ((i % 7) as f64 - 3.0) / 3.0;
            }
            w.push(row);
        }
        SoftmaxModel {
            weights: w,
            bias: vec![0.0; n_classes],
            n_features,
            n_classes,
            feature_set: default_feature_set(),
        }
    }

    /// Chainable setter for `feature_set` (used by `train-router` after
    /// training to record which feature vector this model expects).
    pub fn with_feature_set(mut self, name: impl Into<String>) -> Self {
        self.feature_set = name.into();
        self
    }

    /// Fold a temperature-scaling calibration into the model's weights:
    /// divide every weight and bias by `T`. This makes the model's raw
    /// `forward()` produce the *calibrated* softmax `softmax(logits / T)`
    /// without any runtime change. Top-1 argmax is unchanged (temperature
    /// scaling is monotone on logits), but the confidence values match the
    /// observed accuracy. See
    /// `docs/plans/ROUTER-INTEGRATION-DESIGN-2026-09-30.md`.
    pub fn with_temperature(mut self, t: f64) -> Self {
        if t > 0.0 && (t - 1.0).abs() > 1e-9 {
            for row in self.weights.iter_mut() {
                for v in row.iter_mut() {
                    *v /= t;
                }
            }
            for v in self.bias.iter_mut() {
                *v /= t;
            }
        }
        self
    }

    /// Raw class scores (logits) without softmax. Used by temperature-
    /// scaling calibration (2026-09-30): `softmax(logits / T)` is the
    /// calibrated distribution, and T is chosen to minimize ECE.
    pub fn logits(&self, x: &[f32]) -> Vec<f64> {
        let mut scores = vec![0f64; self.n_classes];
        for (k, wk) in self.weights.iter().enumerate() {
            let mut s = self.bias[k];
            for (i, &xi) in x.iter().take(self.n_features).enumerate() {
                s += wk[i] * xi as f64;
            }
            scores[k] = s;
        }
        scores
    }

    /// Forward pass: softmax over class scores.
    pub fn forward(&self, x: &[f32]) -> Vec<f64> {
        let mut scores = vec![0f64; self.n_classes];
        for (k, wk) in self.weights.iter().enumerate() {
            let mut s = self.bias[k];
            for (i, &xi) in x.iter().take(self.n_features).enumerate() {
                s += wk[i] * xi as f64;
            }
            scores[k] = s;
        }
        let max = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let exps: Vec<f64> = scores.iter().map(|&s| (s - max).exp()).collect();
        let total: f64 = exps.iter().sum();
        exps.iter().map(|e| e / total).collect()
    }

    /// One SGD step on a minibatch (cross-entropy + L2). Returns mean loss.
    pub fn sgd_step(&mut self, batch: &[(Vec<f32>, usize)], lr: f64, l2: f64) -> f64 {
        let mut grad_w = vec![vec![0.0; self.n_features]; self.n_classes];
        let mut grad_b = vec![0.0; self.n_classes];
        let mut loss = 0.0;
        for (x, y) in batch {
            let p = self.forward(x);
            loss += -(p[*y].max(1e-12)).ln();
            for k in 0..self.n_classes {
                let d = p[k] - if k == *y { 1.0 } else { 0.0 };
                grad_b[k] += d;
                for (i, &xi) in x.iter().take(self.n_features).enumerate() {
                    grad_w[k][i] += d * xi as f64;
                }
            }
        }
        let n = batch.len().max(1) as f64;
        for k in 0..self.n_classes {
            self.bias[k] -= lr * grad_b[k] / n;
            for i in 0..self.n_features {
                self.weights[k][i] -= lr * (grad_w[k][i] / n + l2 * self.weights[k][i]);
            }
        }
        loss / n
    }

    pub fn validate(&self) -> Result<(), RouterError> {
        if self.n_classes != 4 || self.n_features != 20 {
            return Err(RouterError::Model("model must be 4×20".into()));
        }
        if self.weights.len() != self.n_classes {
            return Err(RouterError::Model("weight rows mismatch".into()));
        }
        Ok(())
    }
}
