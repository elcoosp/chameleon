//! Uniform board sampling for PCS.
//!
//! A board is 5 distinct cards drawn uniformly without replacement from
//! the 52-card deck. The implementation is a partial Fisher-Yates over
//! a fresh index buffer, then the first five entries. For the intended
//! iteration counts (10^6-10^7) the allocation and shuffle cost is
//! dwarfed by the tree walk, so no attempt is made to reuse buffers
//! here — a later profiling pass can optimize if needed.

use cham_core::card::Card;
use cham_core::rng::{Rng, next_f64};

/// Sample a 5-card board uniformly without replacement.
pub fn sample_board(rng: &mut Rng) -> [Card; 5] {
    let mut deck: [u8; 52] = [0; 52];
    for i in 0..52 {
        deck[i] = i as u8;
    }
    for i in (1..52).rev() {
        let j = (next_f64(rng) * (i + 1) as f64) as usize;
        deck.swap(i, j);
    }
    [
        Card(deck[0]),
        Card(deck[1]),
        Card(deck[2]),
        Card(deck[3]),
        Card(deck[4]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use cham_core::rng::rng_from_seed;
    use std::collections::HashSet;

    #[test]
    fn sampled_board_has_five_distinct_cards() {
        let mut rng = rng_from_seed(0xDEAD_BEEF);
        for _ in 0..100 {
            let board = sample_board(&mut rng);
            let s: HashSet<u8> = board.iter().map(|c| c.idx()).collect();
            assert_eq!(s.len(), 5, "board has duplicates: {board:?}");
        }
    }

    #[test]
    fn different_seeds_produce_different_boards() {
        let b1 = sample_board(&mut rng_from_seed(1));
        let b2 = sample_board(&mut rng_from_seed(2));
        let s1: [u8; 5] = b1.map(|c| c.idx());
        let s2: [u8; 5] = b2.map(|c| c.idx());
        assert_ne!(s1, s2, "different seeds gave identical boards");
    }

    #[test]
    fn same_seed_is_deterministic() {
        let a = sample_board(&mut rng_from_seed(7));
        let b = sample_board(&mut rng_from_seed(7));
        let sa: [u8; 5] = a.map(|c| c.idx());
        let sb: [u8; 5] = b.map(|c| c.idx());
        assert_eq!(sa, sb);
    }

    #[test]
    fn samples_vary_over_many_draws() {
        let mut rng = rng_from_seed(11);
        let mut seen: HashSet<[u8; 5]> = HashSet::new();
        for _ in 0..50 {
            let b: [u8; 5] = sample_board(&mut rng).map(|c| c.idx());
            let mut sorted = b;
            sorted.sort();
            seen.insert(sorted);
        }
        assert!(
            seen.len() > 40,
            "sampler produced only {} distinct boards in 50 draws",
            seen.len()
        );
    }
}
