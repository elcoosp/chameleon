//! Train modes (SPECS/04 §3, §5).

use serde::{Deserialize, Serialize};

use cham_opponents::OpponentSpec;

/// The mode tag for provenance records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrainModeTag {
    Exploit,
    ExploitBayes,
    Robust,
}

/// Belief-bin quantization for `ExploitBayes` (SPECS/04 §5): 4 argmax types × 3
/// confidence terciles + a "cold" bin (n < 30 hands) = 13 bins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeliefBins {
    pub n_families: usize,
}

pub const COLD_BIN: u8 = 12;
pub const N_BINS: u8 = 13;

impl BeliefBins {
    pub fn new(n_families: usize) -> BeliefBins {
        BeliefBins {
            n_families: n_families.max(1),
        }
    }

    /// Dirichlet-multinomial posterior with a uniform prior over `counts`, quantized
    /// to the belief bin. `noisy` corrupts the true frequencies (obs_noise).
    pub fn bin_of(
        &self,
        true_freq: &[f64],
        n_observed: u32,
        obs_noise: f64,
        rng: &mut cham_core::rng::Rng,
    ) -> u8 {
        let k = self.n_families;
        if n_observed < 30 {
            return COLD_BIN;
        }
        // noisy summary: Dirichlet-style noise around the true type frequencies,
        // concentration scaled by observed hands (decision D-010: deterministic
        // additive noise instead of Gamma sampling — analytic and seed-stable)
        let mut noisy = vec![0f64; k];
        for (i, f) in true_freq.iter().take(k).enumerate() {
            let noise = (cham_core::rng::next_f64(rng) * 2.0 - 1.0) * obs_noise
                / (n_observed as f64).sqrt();
            noisy[i] = (f + noise).max(1e-6);
        }
        let total: f64 = noisy.iter().sum();
        for v in noisy.iter_mut() {
            *v /= total;
        }
        // posterior mean with uniform prior
        let alpha = 1.0;
        let mut post = vec![0f64; k];
        for i in 0..k {
            post[i] =
                (noisy[i] * n_observed as f64 + alpha) / (n_observed as f64 + alpha * k as f64);
        }
        let mut argmax = 0usize;
        for (i, v) in post.iter().enumerate() {
            if *v > post[argmax] {
                argmax = i;
            }
        }
        let conf = (post[argmax] - 1.0 / k as f64) / (1.0 - 1.0 / k as f64); // ∈ [0,1]
        let tercile = (conf * 3.0).floor().min(2.0) as u8;
        (argmax as u8 * 3 + tercile).min(COLD_BIN - 1)
    }
}

/// Training mode (SPECS/04 §3).
#[derive(Clone)]
pub enum TrainMode {
    /// One-sided ES-MCCFR vs a scripted opponent distribution.
    Exploit {
        opponent: OpponentSpec,
        jitter_seed: u64,
    },
    /// ONE policy vs a per-session-sampled hidden type with a quantized belief bin.
    ExploitBayes {
        families: Vec<OpponentSpec>,
        obs_noise: f64,
        bins: BeliefBins,
    },
    /// Two-sided CFR+ self-play (regret matching+, alternating updates, discounting).
    Robust,
}

impl TrainMode {
    pub fn tag(&self) -> TrainModeTag {
        match self {
            TrainMode::Exploit { .. } => TrainModeTag::Exploit,
            TrainMode::ExploitBayes { .. } => TrainModeTag::ExploitBayes,
            TrainMode::Robust => TrainModeTag::Robust,
        }
    }
}
