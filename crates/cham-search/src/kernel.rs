//! Vector-form river kernels (F10, 2026-10-02).
//!
//! The report (`docs/plans/F10-VECTOR-SOLVER-PLAN-2026-10-02.md`) calls
//! for a vector CFR+ over full combo ranges. Its one non-trivial piece
//! is the showdown counterfactual value (CFV) with card removal in
//! O(n) rather than O(n^2). This module ports that kernel.
//!
//! Conventions
//! -----------
//! - `hands[i]` is a pair of card indices in `0..52`.
//! - `rank[i]` is the hand strength, **ascending** (larger = stronger).
//!   Ties share the same value.
//! - `opp_reach[j]` is the opponent's reach weight for hand `j`.
//! - The returned value for hand `i` is the expected payoff to `i`
//!   against the opponent's reach distribution, in units of the pot
//!   fraction at the showdown node. Ties contribute 0.
//! - Opponent hands that share a card with `i` are excluded (card
//!   removal): a hand cannot be held by both players.

/// O(n) showdown CFV with card removal.
///
/// For each hand `i`:
///
///     out[i] = sum_j opp_reach[j] * sign(rank[i] - rank[j])
///                        * [hands i and j share no card]
///
/// where `sign` is +1 for a win, -1 for a loss, 0 for a tie.
///
/// The naive evaluation is O(n^2). This implementation is O(n log n)
/// for the sort plus O(n) for the two sweeps.
pub fn showdown_cfv(hands: &[[u8; 2]], rank: &[u32], opp_reach: &[f64], out: &mut [f64]) {
    let n = hands.len();
    assert_eq!(rank.len(), n, "rank length mismatch");
    assert_eq!(opp_reach.len(), n, "opp_reach length mismatch");
    assert_eq!(out.len(), n, "out length mismatch");
    if n == 0 {
        return;
    }

    // Permutation sorted by rank ascending. Ties are grouped in the
    // sweeps; their relative order is irrelevant.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| rank[i]);

    // --- Pass 1: strictly-below mass with card removal ---
    let mut below_total = 0.0_f64;
    let mut below_card = [0.0_f64; 52];
    let mut below_no_overlap = vec![0.0_f64; n];

    let mut g = 0usize;
    while g < n {
        let r = rank[order[g]];
        let mut h = g;
        while h < n && rank[order[h]] == r {
            h += 1;
        }
        // All hands order[g..h] share rank r; compute their below term
        // from the frozen state, THEN fold the group into the state.
        for &i in &order[g..h] {
            let a = hands[i][0] as usize;
            let b = hands[i][1] as usize;
            below_no_overlap[i] = below_total - below_card[a] - below_card[b];
        }
        for &i in &order[g..h] {
            let w = opp_reach[i];
            below_total += w;
            below_card[hands[i][0] as usize] += w;
            below_card[hands[i][1] as usize] += w;
        }
        g = h;
    }

    // --- Pass 2: strictly-above mass with card removal (descending) ---
    let mut above_total = 0.0_f64;
    let mut above_card = [0.0_f64; 52];
    let mut above_no_overlap = vec![0.0_f64; n];

    let mut g = n;
    while g > 0 {
        let r = rank[order[g - 1]];
        let mut h = g;
        while h > 0 && rank[order[h - 1]] == r {
            h -= 1;
        }
        for &i in &order[h..g] {
            let a = hands[i][0] as usize;
            let b = hands[i][1] as usize;
            above_no_overlap[i] = above_total - above_card[a] - above_card[b];
        }
        for &i in &order[h..g] {
            let w = opp_reach[i];
            above_total += w;
            above_card[hands[i][0] as usize] += w;
            above_card[hands[i][1] as usize] += w;
        }
        g = h;
    }

    for i in 0..n {
        out[i] = below_no_overlap[i] - above_no_overlap[i];
    }
}

/// O(n) fold-terminal CFV with card removal.
///
/// If the opponent folds, the acting player wins the opponent's
/// contribution to the pot. In CFV terms, for each hand `i`:
///
///     out[i] = -hero_invested * (mass of opponent hands disjoint from i)
///
/// The sign is negative because `hero_invested` is what the hero has
/// already put in and loses by folding — the terminal value of a fold
/// is `-hero_invested` times the still-live opponent mass.
///
/// The input `hands` is assumed to be a set of unique combos. Callers
/// passing duplicate combos get incorrect card-removal correction; the
/// heads-up subgame never does.
pub fn fold_cfv(hands: &[[u8; 2]], opp_reach: &[f64], hero_invested: f64, out: &mut [f64]) {
    let n = hands.len();
    assert_eq!(opp_reach.len(), n, "opp_reach length mismatch");
    assert_eq!(out.len(), n, "out length mismatch");
    if n == 0 {
        return;
    }

    let total: f64 = opp_reach.iter().sum();
    let mut card_mass = [0.0_f64; 52];
    for (i, h) in hands.iter().enumerate() {
        card_mass[h[0] as usize] += opp_reach[i];
        card_mass[h[1] as usize] += opp_reach[i];
    }

    for i in 0..n {
        let a = hands[i][0] as usize;
        let b = hands[i][1] as usize;
        // The hand sharing BOTH cards with i is hand i itself (the input
        // is a set of unique combos; heads-up ranges cannot double-count).
        // Its mass is `opp_reach[i]`, so the inclusion-exclusion
        // correction is O(1).
        let both = opp_reach[i];
        let overlap = card_mass[a] + card_mass[b] - both;
        let disjoint = total - overlap;
        out[i] = -hero_invested * disjoint;
    }
}

// =====================================================================
// Two-range form (F10 step 1b).
//
// The symmetric kernel above assumes a single hand pool where every pair
// is card-disjoint. Real heads-up river play is hero range vs villain
// range, both drawn from the same 52-card deck. This form expresses that
// directly: `hero[i]` vs `villain[j]`, with card removal when the two
// hands share a card.
// =====================================================================

/// O(n) showdown CFV for hero hands `i` vs villain hands `j`.
///
/// For each hero hand `i`:
///
///     out[i] = sum_j villain_reach[j] * sign(rank_h[i] - rank_v[j])
///                        * [hero[i] and villain[j] share no card]
///
/// Ties contribute 0. `out` has length `hero.len()`. Ranks must be
/// ascending with ties equal on both sides.
pub fn showdown_cfv_two(
    hero: &[[u8; 2]],
    rank_h: &[u32],
    villain: &[[u8; 2]],
    rank_v: &[u32],
    villain_reach: &[f64],
    out: &mut [f64],
) {
    let nh = hero.len();
    let nv = villain.len();
    assert_eq!(rank_h.len(), nh, "rank_h length mismatch");
    assert_eq!(rank_v.len(), nv, "rank_v length mismatch");
    assert_eq!(villain_reach.len(), nv, "villain_reach length mismatch");
    assert_eq!(out.len(), nh, "out length mismatch");
    if nh == 0 || nv == 0 {
        for v in out.iter_mut() {
            *v = 0.0;
        }
        return;
    }

    // Sort villain by rank ascending; hero by rank ascending (indices
    // kept so we can scatter results back).
    let mut ord_v: Vec<usize> = (0..nv).collect();
    ord_v.sort_by_key(|&j| rank_v[j]);
    let mut ord_h: Vec<usize> = (0..nh).collect();
    ord_h.sort_by_key(|&i| rank_h[i]);

    let mut below = vec![0.0_f64; nh];
    let mut above = vec![0.0_f64; nh];

    // --- Below sweep: hero in ascending rank order; villain pointer
    // advances while rank_v < rank_h[i]. Running state holds the mass of
    // villains strictly below the current hero rank, indexed by card and
    // by (canonical) hand for the both-cards correction. ---
    {
        let mut total = 0.0_f64;
        let mut card = [0.0_f64; 52];
        let mut both: std::collections::HashMap<(u8, u8), f64> = std::collections::HashMap::new();
        let mut jj = 0usize;
        for &i in &ord_h {
            let rh = rank_h[i];
            while jj < nv && rank_v[ord_v[jj]] < rh {
                let j = ord_v[jj];
                let w = villain_reach[j];
                let a = villain[j][0];
                let b = villain[j][1];
                let (lo, hi) = if a < b { (a, b) } else { (b, a) };
                total += w;
                card[a as usize] += w;
                card[b as usize] += w;
                *both.entry((lo, hi)).or_insert(0.0) += w;
                jj += 1;
            }
            let a = hero[i][0];
            let b = hero[i][1];
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            let bh = *both.get(&(lo, hi)).unwrap_or(&0.0);
            below[i] = total - card[a as usize] - card[b as usize] + bh;
        }
    }

    // --- Above sweep: hero in DESCENDING rank order; villain pointer
    // decrements while rank_v > rank_h[i]. ---
    {
        let mut total = 0.0_f64;
        let mut card = [0.0_f64; 52];
        let mut both: std::collections::HashMap<(u8, u8), f64> = std::collections::HashMap::new();
        let mut jj = nv;
        for &i in ord_h.iter().rev() {
            let rh = rank_h[i];
            while jj > 0 && rank_v[ord_v[jj - 1]] > rh {
                jj -= 1;
                let j = ord_v[jj];
                let w = villain_reach[j];
                let a = villain[j][0];
                let b = villain[j][1];
                let (lo, hi) = if a < b { (a, b) } else { (b, a) };
                total += w;
                card[a as usize] += w;
                card[b as usize] += w;
                *both.entry((lo, hi)).or_insert(0.0) += w;
            }
            let a = hero[i][0];
            let b = hero[i][1];
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            let bh = *both.get(&(lo, hi)).unwrap_or(&0.0);
            above[i] = total - card[a as usize] - card[b as usize] + bh;
        }
    }

    for i in 0..nh {
        out[i] = below[i] - above[i];
    }
}
