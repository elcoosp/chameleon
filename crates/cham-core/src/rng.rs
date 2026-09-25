//! RNG discipline (SPECS/01 §4, SPECS/00 §3): one RNG type everywhere —
//! `ChaCha8Rng`. Seeds are u64 and recorded; a hand is replayable from
//! `(match_seed, hand_index)` via deterministic child derivation.
//!
//! Note: edition 2024 reserves the `gen` keyword, and the whitelisted `rand` 0.8
//! names its method `gen`. All draws therefore go through the helpers below
//! (`Standard::sample`), keeping call sites keyword-free.

use rand::SeedableRng;
use rand::distributions::{Distribution, Standard};
use rand_chacha::ChaCha8Rng;

/// THE rng of the project.
pub type Rng = ChaCha8Rng;

/// Derive a fresh rng from a u64 seed.
pub fn rng_from_seed(seed: u64) -> Rng {
    ChaCha8Rng::seed_from_u64(seed)
}

/// Deterministic child derivation: `child(seed, label)` — every consumer derives its
/// own stream (iteration t, hand index, per-decision draws) from labeled children.
pub fn child(seed: u64, label: &str) -> Rng {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ seed;
    for &b in label.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    ChaCha8Rng::seed_from_u64(h)
}

/// Uniform f64 in [0, 1).
pub fn next_f64(rng: &mut Rng) -> f64 {
    Standard.sample(rng)
}

/// Uniform u32 across the full range.
pub fn next_u32(rng: &mut Rng) -> u32 {
    Standard.sample(rng)
}

/// Pick an index uniformly from `0..n`.
pub fn pick(rng: &mut Rng, n: usize) -> usize {
    use rand::Rng as _;
    rng.gen_range(0..n)
}

/// Weighted pick: returns an index into `weights` (need not sum to 1; must be positive).
pub fn weighted(rng: &mut Rng, weights: &[f64]) -> usize {
    let total: f64 = weights.iter().sum();
    debug_assert!(total > 0.0, "weighted pick needs positive mass");
    let mut u = next_f64(rng) * total;
    for (i, w) in weights.iter().enumerate() {
        u -= w;
        if u <= 0.0 {
            return i;
        }
    }
    weights.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_child_determinism() {
        let a = child(42, "iter3");
        let b = child(42, "iter3");
        let c = child(42, "iter4");
        let av: Vec<u32> = (0..8).map(|_| next_u32(&mut a.clone())).collect();
        let bv: Vec<u32> = (0..8).map(|_| next_u32(&mut b.clone())).collect();
        let cv: Vec<u32> = (0..8).map(|_| next_u32(&mut c.clone())).collect();
        assert_eq!(av, bv);
        assert_ne!(av, cv);
        assert_ne!(
            next_u32(&mut rng_from_seed(7)),
            next_u32(&mut rng_from_seed(8))
        );
    }
}
