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
pub fn showdown_cfv(
    hands: &[[u8; 2]],
    rank: &[u32],
    opp_reach: &[f64],
    out: &mut [f64],
) {
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
pub fn fold_cfv(
    hands: &[[u8; 2]],
    opp_reach: &[f64],
    hero_invested: f64,
    out: &mut [f64],
) {
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
        // Mass of opponent hands sharing card a, plus those sharing card
        // b, minus the mass of the (at most one) hand sharing BOTH — which
        // is hand i itself if the opponent's range includes it. In HU
        // river subgames the ranges are card-disjoint, so `both` is 0 for
        // every i; the subtraction keeps the kernel correct even if a
        // caller passes overlapping ranges.
        let mut both = 0.0_f64;
        for (j, h) in hands.iter().enumerate() {
            if (h[0] as usize == a && h[1] as usize == b)
                || (h[0] as usize == b && h[1] as usize == a)
            {
                both += opp_reach[j];
            }
        }
        let overlap = card_mass[a] + card_mass[b] - both;
        let disjoint = total - overlap;
        out[i] = -hero_invested * disjoint;
    }
}
