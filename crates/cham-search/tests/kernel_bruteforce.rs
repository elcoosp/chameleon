//! F10 step 1 (2026-10-02): validate the O(n) showdown/fold kernels
//! against a naive O(n^2) brute force on random hands.
//!
//! The report validated its Python kernel over 1,081 combos with many
//! ties (max error 5.7e-13). This port is checked the same way, on
//! hand-crafted card-disjoint sets so card removal is well-defined.

use cham_search::kernel::{fold_cfv, showdown_cfv, showdown_cfv_two};

/// Minimal deterministic PRNG (xorshift64) so the test needs no deps
/// and is reproducible across runs.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Build `n` card-disjoint hands. `max_rank` controls tie frequency.
fn sample_hands(n: usize, max_rank: u32, seed: u64) -> (Vec<[u8; 2]>, Vec<u32>, Vec<f64>) {
    // Card-disjoint hands from a single 52-card deck: at most 26.
    assert!(
        n <= 26,
        "sample_hands: n={n} exceeds 26 card-disjoint hands"
    );
    let mut rng = Rng(seed | 1);
    let mut deck: Vec<u8> = (0..52).collect();
    // Fisher-Yates shuffle.
    for i in (1..deck.len()).rev() {
        let j = rng.below((i + 1) as u64) as usize;
        deck.swap(i, j);
    }
    let mut hands = Vec::with_capacity(n);
    for k in 0..n {
        let a = deck[2 * k];
        let b = deck[2 * k + 1];
        hands.push([a, b]);
    }
    let rank: Vec<u32> = (0..n)
        .map(|_| rng.below(max_rank as u64 + 1) as u32)
        .collect();
    let reach: Vec<f64> = (0..n).map(|_| rng.below(1000) as f64 / 1000.0).collect();
    (hands, rank, reach)
}

fn brute_showdown(hands: &[[u8; 2]], rank: &[u32], reach: &[f64]) -> Vec<f64> {
    let n = hands.len();
    let mut out = vec![0.0f64; n];
    for i in 0..n {
        let mut acc = 0.0;
        for j in 0..n {
            let disjoint = hands[i][0] != hands[j][0]
                && hands[i][0] != hands[j][1]
                && hands[i][1] != hands[j][0]
                && hands[i][1] != hands[j][1];
            if !disjoint {
                continue;
            }
            let s = if rank[i] > rank[j] {
                1.0
            } else if rank[i] < rank[j] {
                -1.0
            } else {
                0.0
            };
            acc += s * reach[j];
        }
        out[i] = acc;
    }
    out
}

fn brute_fold(hands: &[[u8; 2]], reach: &[f64], hero_invested: f64) -> Vec<f64> {
    let n = hands.len();
    let mut out = vec![0.0f64; n];
    for i in 0..n {
        let mut live = 0.0;
        for j in 0..n {
            let disjoint = hands[i][0] != hands[j][0]
                && hands[i][0] != hands[j][1]
                && hands[i][1] != hands[j][0]
                && hands[i][1] != hands[j][1];
            if disjoint {
                live += reach[j];
            }
        }
        out[i] = -hero_invested * live;
    }
    out
}

#[test]
fn showdown_matches_brute_force_many_ties() {
    // max_rank = 6 forces many ties, like the report's 1,081-combo case.
    let (hands, rank, reach) = sample_hands(26, 6, 0x1B2);
    let mut got = vec![0.0f64; hands.len()];
    showdown_cfv(&hands, &rank, &reach, &mut got);
    let want = brute_showdown(&hands, &rank, &reach);
    let mut max_err = 0.0f64;
    for i in 0..hands.len() {
        max_err = max_err.max((got[i] - want[i]).abs());
    }
    assert!(max_err < 1e-9, "showdown max error {max_err}");
}

#[test]
fn showdown_matches_brute_force_no_ties() {
    let (hands, mut rank, reach) = sample_hands(26, 6, 0x1B3);
    // Make all ranks distinct.
    for (i, r) in rank.iter_mut().enumerate() {
        *r = i as u32;
    }
    let mut got = vec![0.0f64; hands.len()];
    showdown_cfv(&hands, &rank, &reach, &mut got);
    let want = brute_showdown(&hands, &rank, &reach);
    for i in 0..hands.len() {
        assert!(
            (got[i] - want[i]).abs() < 1e-9,
            "hand {i}: got {} want {}",
            got[i],
            want[i]
        );
    }
}

#[test]
fn fold_matches_brute_force() {
    let (hands, _rank, reach) = sample_hands(26, 6, 0x1B4);
    let hero_invested = 12.5;
    let mut got = vec![0.0f64; hands.len()];
    fold_cfv(&hands, &reach, hero_invested, &mut got);
    let want = brute_fold(&hands, &reach, hero_invested);
    for i in 0..hands.len() {
        assert!(
            (got[i] - want[i]).abs() < 1e-9,
            "hand {i}: got {} want {}",
            got[i],
            want[i]
        );
    }
}

#[test]
fn showdown_empty_input_is_noop() {
    let mut out: Vec<f64> = vec![];
    showdown_cfv(&[], &[], &[], &mut out);
    assert!(out.is_empty());
}

#[test]
fn showdown_all_ties_gives_zero() {
    let (hands, _r, reach) = sample_hands(26, 1, 0x1B5);
    let rank = vec![7u32; hands.len()];
    let mut got = vec![0.0f64; hands.len()];
    showdown_cfv(&hands, &rank, &reach, &mut got);
    for (i, v) in got.iter().enumerate() {
        assert!(v.abs() < 1e-12, "hand {i}: expected 0, got {v}");
    }
}

// ---------------------------------------------------------------------
// Two-range form: hero pool vs villain pool, both from the same deck,
// pools may overlap (a hero hand and a villain hand may share a card).
// ---------------------------------------------------------------------

fn sample_range(n: usize, max_rank: u32, seed: u64) -> (Vec<[u8; 2]>, Vec<u32>, Vec<f64>) {
    assert!(n <= 26, "sample_range: n={n} exceeds 26");
    let mut rng = Rng(seed | 1);
    let mut deck: Vec<u8> = (0..52).collect();
    for i in (1..deck.len()).rev() {
        let j = rng.below((i + 1) as u64) as usize;
        deck.swap(i, j);
    }
    let mut hands = Vec::with_capacity(n);
    for k in 0..n {
        hands.push([deck[2 * k], deck[2 * k + 1]]);
    }
    let rank: Vec<u32> = (0..n)
        .map(|_| rng.below(max_rank as u64 + 1) as u32)
        .collect();
    let reach: Vec<f64> = (0..n).map(|_| rng.below(1000) as f64 / 1000.0).collect();
    (hands, rank, reach)
}

fn brute_showdown_two(
    hero: &[[u8; 2]],
    rank_h: &[u32],
    villain: &[[u8; 2]],
    rank_v: &[u32],
    vreach: &[f64],
) -> Vec<f64> {
    let mut out = vec![0.0f64; hero.len()];
    for i in 0..hero.len() {
        let mut acc = 0.0;
        for j in 0..villain.len() {
            let disjoint = hero[i][0] != villain[j][0]
                && hero[i][0] != villain[j][1]
                && hero[i][1] != villain[j][0]
                && hero[i][1] != villain[j][1];
            if !disjoint {
                continue;
            }
            let s = if rank_h[i] > rank_v[j] {
                1.0
            } else if rank_h[i] < rank_v[j] {
                -1.0
            } else {
                0.0
            };
            acc += s * vreach[j];
        }
        out[i] = acc;
    }
    out
}

#[test]
fn two_range_showdown_matches_brute_force_overlapping_pools() {
    // Hero and villain drawn from the SAME deck independently, so the
    // pools overlap and card removal actually bites.
    let (hero, rank_h, _) = sample_range(20, 6, 0x2B2);
    let (villain, rank_v, vreach) = sample_range(20, 6, 0x2B3);
    let mut got = vec![0.0f64; hero.len()];
    showdown_cfv_two(&hero, &rank_h, &villain, &rank_v, &vreach, &mut got);
    let want = brute_showdown_two(&hero, &rank_h, &villain, &rank_v, &vreach);
    let mut max_err = 0.0f64;
    for i in 0..hero.len() {
        max_err = max_err.max((got[i] - want[i]).abs());
    }
    assert!(max_err < 1e-9, "two-range showdown max error {max_err}");
}

#[test]
fn two_range_showdown_no_ties() {
    let (hero, mut rank_h, _) = sample_range(20, 6, 0x2B4);
    let (villain, mut rank_v, vreach) = sample_range(20, 6, 0x2B5);
    for (i, r) in rank_h.iter_mut().enumerate() {
        *r = 100 + i as u32;
    }
    for (j, r) in rank_v.iter_mut().enumerate() {
        *r = j as u32;
    }
    let mut got = vec![0.0f64; hero.len()];
    showdown_cfv_two(&hero, &rank_h, &villain, &rank_v, &vreach, &mut got);
    let want = brute_showdown_two(&hero, &rank_h, &villain, &rank_v, &vreach);
    for i in 0..hero.len() {
        assert!(
            (got[i] - want[i]).abs() < 1e-9,
            "hand {i}: got {} want {}",
            got[i],
            want[i]
        );
    }
}

#[test]
fn two_range_showdown_all_ties_gives_zero() {
    let (hero, _rh, _) = sample_range(15, 1, 0x2B6);
    let (villain, _rv, vreach) = sample_range(15, 1, 0x2B7);
    let rank_h = vec![5u32; hero.len()];
    let rank_v = vec![5u32; villain.len()];
    let mut got = vec![0.0f64; hero.len()];
    showdown_cfv_two(&hero, &rank_h, &villain, &rank_v, &vreach, &mut got);
    for (i, v) in got.iter().enumerate() {
        assert!(v.abs() < 1e-12, "hand {i}: expected 0, got {v}");
    }
}
