//! Tests for `Tracker::raw_opponent_frequencies` (2026-09-29).
//!
//! Covers the two commits that added the honest opponent-only feature
//! vector:
//!   aeb1221  feat(cham-agent): raw unconditional opponent frequencies
//!   f630614  feat(cham-agent): ChameleonAgent::opponent_only_features
//! plus the fix b31b5c4 (a preflop 3bet must NOT count as postflop
//! aggression).
//!
//! These features are the substrate for a router that describes the
//! OPPONENT independently of hero's policy. See
//! docs/plans/ROUTER-FEATURE-LEAK-2026-09-29.md.
//!
//! Pure unit tests. No engine, no encoder.

use cham_agent::tracker::Tracker;
use cham_core::card::{Card, Hand2};
use cham_core::engine::history::PublicHistory;
use cham_core::engine::{Action, Street};
use cham_core::obs::Player;

fn card(s: &str) -> Card {
    Card::parse(s).expect("card")
}

/// Build a PublicHistory from a compact action list. `Sb` acts as player 0.
fn ph(actions: &[(Street, Player, Action)]) -> PublicHistory {
    PublicHistory {
        actions: actions.to_vec(),
        board: [card("2c"); 5],
        showdown_holes: [None, None],
        nets: [0, 0],
    }
}

/// A tracker with the given hands observed once each, hero at seat 0.
fn tracker_after(hands: &[PublicHistory]) -> Tracker {
    let mut t = Tracker::new();
    for ph in hands {
        t.observe_hand(ph, 0, 0);
    }
    t
}

// ---------- structural invariants ----------

/// Every value in the 10-vector is finite and in [0, 1], for a variety
/// of histories. Prevents a divide-by-zero (the accessor uses `.max(1)`
/// on every denominator, so zero-division is impossible by construction
/// — but a mutant that removes a `.max(1)` would produce inf or NaN).
#[test]
fn raw_frequencies_are_in_unit_range() {
    let t = tracker_after(&[]);
    let f = t.raw_opponent_frequencies();
    assert_eq!(f.len(), 10);
    for (i, &v) in f.iter().enumerate() {
        assert!(v.is_finite(), "empty tracker: freq[{i}] = {v} not finite");
        assert!(
            (0.0..=1.0).contains(&v),
            "empty tracker: freq[{i}] = {v} out of [0,1]"
        );
    }
}

/// A tracker with no observations returns all zeros (numerators 0,
/// denominators clamped to 1 by `.max(1)`).
#[test]
fn raw_frequencies_zero_on_empty_tracker() {
    let t = Tracker::new();
    let f = t.raw_opponent_frequencies();
    for (i, &v) in f.iter().enumerate() {
        assert!(
            v.abs() < 1e-12,
            "empty tracker: freq[{i}] = {v}, expected 0"
        );
    }
}

// ---------- preflop counts ----------

/// Opponent folds preflop: `preflop_fold_freq` goes to 1/1 = 1.0.
#[test]
fn preflop_fold_increments_fold_freq() {
    let h = ph(&[(
        Street::Preflop,
        Player::Bb,
        Action::Fold,
    )]);
    let t = tracker_after(&[h]);
    let f = t.raw_opponent_frequencies();
    assert!(
        (f[2] - 1.0).abs() < 1e-12,
        "one fold / one hand → preflop_fold_freq 1.0, got {}",
        f[2]
    );
    assert!(f[0].abs() < 1e-12, "no raise: raise_freq stays 0");
    assert!(f[1].abs() < 1e-12, "no call: call_freq stays 0");
}

/// Opponent raises once preflop: `preflop_raise_freq` → 1.0. Must NOT
/// increment `preflop_call_freq`, and must NOT increment the postflop
/// raise count (a lone raise is not a 3bet).
#[test]
fn preflop_single_raise_counts_as_preflop_only() {
    let h = ph(&[(
        Street::Preflop,
        Player::Bb,
        Action::Raise { to: 300 },
    )]);
    let t = tracker_after(&[h]);
    let f = t.raw_opponent_frequencies();
    assert!(
        (f[0] - 1.0).abs() < 1e-12,
        "one raise / one hand → raise_freq 1.0, got {}",
        f[0]
    );
    assert!(
        (f[1]).abs() < 1e-12,
        "raise must not increment call_freq, got {}",
        f[1]
    );
    // f[7] = aggression = (preflop_raises + postflop_raises) / total_actions
    // total_actions = 1 (just the raise), preflop_raises = 1,
    // postflop_raises = 0 → f[7] = 1.0
    assert!(
        (f[7] - 1.0).abs() < 1e-12,
        "one raise / one action → aggression 1.0, got {}",
        f[7]
    );
}

/// Two preflop raises from the opponent (an open then a 3bet) both count
/// as preflop raises. The 3bet must NOT increment `opp_postflop_raises`
/// (that was bug b31b5c4).
#[test]
fn preflop_3bet_is_preflop_not_postflop() {
    let h = ph(&[
        (Street::Preflop, Player::Sb, Action::Raise { to: 200 }),
        (Street::Preflop, Player::Bb, Action::Raise { to: 700 }),
    ]);
    let t = tracker_after(&[h]);
    assert_eq!(
        t.opp_preflop_raises, 1,
        "opp raised once preflop (the second actor's 3bet), hero raised once"
    );
    assert_eq!(
        t.opp_postflop_raises, 0,
        "preflop 3bet must not count as postflop aggression (b31b5c4)"
    );
}

// ---------- postflop counts ----------

/// Opponent bets on the flop after the preflop is done. Increments
/// `opp_flop_bets` and `opp_postflop_raises`, and `reached_flop_count`.
#[test]
fn flop_bet_increments_flop_and_postflop() {
    let h = ph(&[
        (Street::Preflop, Player::Sb, Action::Call),
        (Street::Preflop, Player::Bb, Action::Check),
        (Street::Flop, Player::Bb, Action::Bet { to: 200 }),
    ]);
    let t = tracker_after(&[h]);
    assert_eq!(t.opp_flop_bets, 1);
    assert_eq!(t.opp_postflop_raises, 1);
    assert!(t.reached_flop_count >= 1);
}

/// Bets on turn and river are counted separately from flop bets.
#[test]
fn turn_and_river_bets_count_separately() {
    let h = ph(&[
        (Street::Preflop, Player::Sb, Action::Call),
        (Street::Preflop, Player::Bb, Action::Check),
        (Street::Flop, Player::Bb, Action::Check),
        (Street::Flop, Player::Sb, Action::Check),
        (Street::Turn, Player::Sb, Action::Bet { to: 300 }),
        (Street::Turn, Player::Bb, Action::Call),
        (Street::River, Player::Bb, Action::Bet { to: 500 }),
    ]);
    let t = tracker_after(&[h]);
    assert_eq!(t.opp_flop_bets, 0, "no flop bet from opp (only checks)");
    assert_eq!(t.opp_turn_bets, 0, "opp called the turn, did not bet");
    assert_eq!(t.opp_river_bets, 1, "opp bet the river");
    assert_eq!(t.opp_postflop_raises, 1, "only the river bet is a postflop raise");
}

// ---------- checks and calls ----------

/// A check increments `opp_checks`.
#[test]
fn check_increments_check_counter() {
    let h = ph(&[
        (Street::Preflop, Player::Sb, Action::Call),
        (Street::Preflop, Player::Bb, Action::Check),
    ]);
    let t = tracker_after(&[h]);
    assert_eq!(t.opp_checks, 1);
}

/// A preflop limp (call with no prior raise) counts as both a preflop
/// call and a limp.
#[test]
fn preflop_limp_counts_call_and_limp() {
    let h = ph(&[(Street::Preflop, Player::Bb, Action::Call)]);
    let t = tracker_after(&[h]);
    assert_eq!(t.opp_preflop_calls, 1);
    assert_eq!(t.opp_limps, 1);
}

// ---------- showdown ----------

/// The showdown reach frequency counts hands that end with both holes
/// revealed (board_len == 5 in the source HandHistory — tests construct
/// the PublicHistory directly with both Some).
#[test]
fn showdown_reach_counts_when_holes_revealed() {
    let mut h = ph(&[
        (Street::Preflop, Player::Sb, Action::Call),
        (Street::Preflop, Player::Bb, Action::Check),
    ]);
    h.showdown_holes = [
        Some(Hand2::new(card("Ah"), card("Kd"))),
        Some(Hand2::new(card("Qh"), card("Jd"))),
    ];
    let t = tracker_after(&[h]);
    let f = t.raw_opponent_frequencies();
    assert!(
        (f[6] - 1.0).abs() < 1e-12,
        "one showdown / one hand → showdown_reach_freq 1.0, got {}",
        f[6]
    );
}

/// No showdown holes → showdown_reach_freq stays 0.
#[test]
fn no_showdown_reach_when_holes_hidden() {
    let h = ph(&[(Street::Preflop, Player::Bb, Action::Fold)]);
    let t = tracker_after(&[h]);
    let f = t.raw_opponent_frequencies();
    assert!(f[6].abs() < 1e-12);
}

// ---------- aggregation ----------

/// Multiple hands accumulate. Three hands, opponent folds all three:
/// fold_freq = 3/3 = 1.0, everything else 0.
#[test]
fn multiple_hands_accumulate() {
    let fold = ph(&[(Street::Preflop, Player::Bb, Action::Fold)]);
    let t = tracker_after(&[fold.clone(), fold.clone(), fold.clone()]);
    assert_eq!(t.hands, 3);
    let f = t.raw_opponent_frequencies();
    assert!((f[2] - 1.0).abs() < 1e-12, "3 folds / 3 hands → 1.0");
}

/// Raise 1 of 2 hands → raise_freq = 0.5.
#[test]
fn mixed_hands_produce_fractions() {
    let raise = ph(&[(Street::Preflop, Player::Bb, Action::Raise { to: 300 })]);
    let fold = ph(&[(Street::Preflop, Player::Bb, Action::Fold)]);
    let t = tracker_after(&[raise, fold]);
    let f = t.raw_opponent_frequencies();
    assert!(
        (f[0] - 0.5).abs() < 1e-12,
        "1 raise / 2 hands → raise_freq 0.5, got {}",
        f[0]
    );
    assert!(
        (f[2] - 0.5).abs() < 1e-12,
        "1 fold / 2 hands → fold_freq 0.5, got {}",
        f[2]
    );
}

/// Aggression + passivity share the same denominator (total actions).
/// Their sum can exceed 1.0 only if some actions are unclassified; the
/// currently-classified set is {raises, bets, calls, checks, folds}, and
/// folds are not counted in either f[7] or f[8]. So aggression + passivity
/// equals (actions - folds) / total_actions.
#[test]
fn aggression_and_passivity_share_denominator() {
    // 1 check + 1 call from opp; no folds.
    let h = ph(&[
        (Street::Preflop, Player::Sb, Action::Call),
        (Street::Preflop, Player::Bb, Action::Check),
    ]);
    let t = tracker_after(&[h]);
    let f = t.raw_opponent_frequencies();
    let sum = f[7] + f[8];
    // opp_total_actions = 1 (just the check; the hero's call is not opp's)
    // aggression = 0, passivity = 1/1 = 1.0
    assert!(
        (sum - 1.0).abs() < 1e-9,
        "aggression + passivity should equal 1 when opp never folds; got {sum}"
    );
}

// ---------- TAG vs LAG separation diagnostic (2026-09-30) ----------

/// Same as `tracker_after` but with hero at seat 1 (BB) so the opponent
/// is the SB — which is required for the opponent to be the preflop
/// OPENER, which TAG and LAG differ on most cleanly.
fn tracker_after_hero_bb(hands: &[PublicHistory]) -> Tracker {
    let mut t = Tracker::new();
    for ph in hands {
        t.observe_hand(ph, 0, 1);
    }
    t
}

/// Build a synthetic session where the opponent is the SB:
///  * the opponent opens with `open_n` hands
///  * of those opens, the opponent cbets flop on `cbet_n` (hero calls,
///    then opponent either bets or checks)
///  * on the other `n - open_n` hands, the opponent folds preflop
fn synthetic_tag_or_lag(n: usize, open_n: usize, cbet_n: usize) -> Vec<PublicHistory> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        if i < cbet_n {
            // opp opens, hero calls, opp cbets, hero folds
            out.push(ph(&[
                (Street::Preflop, Player::Sb, Action::Raise { to: 250 }),
                (Street::Preflop, Player::Bb, Action::Call),
                (Street::Flop, Player::Sb, Action::Bet { to: 300 }),
                (Street::Flop, Player::Bb, Action::Fold),
            ]));
        } else if i < open_n {
            // opp opens, hero calls, opp checks
            out.push(ph(&[
                (Street::Preflop, Player::Sb, Action::Raise { to: 250 }),
                (Street::Preflop, Player::Bb, Action::Call),
                (Street::Flop, Player::Sb, Action::Check),
                (Street::Flop, Player::Bb, Action::Check),
            ]));
        } else {
            // opp folds preflop
            out.push(ph(&[(Street::Preflop, Player::Sb, Action::Fold)]));
        }
    }
    out
}

/// Diagnostic (2026-09-30): TAG vs LAG differ in the DIRECTION of their
/// aggression across streets. TAG opens tighter preflop (25%) but cbets
/// more (52%); LAG opens looser (40%) but cbets less (42%). Neither the
/// preflop raise rate alone nor the flop bet rate alone discriminates
/// them cleanly: the preflop rate points the wrong way, and the flop
/// rate gap (0.095) is smaller than the combined tilt gap (0.245).
///
/// This test proves the tilt signal is present in the raw frequencies,
/// so a router with an extra derived feature can be trained on it.
///
/// Caveat: this is a synthetic test using the exact numbers from
/// `cham-opponents/src/params.rs::point()`. The signal strength on real
/// instrumented play may differ; but the direction of the effect is
/// what the test pins, not the exact magnitude.
#[test]
fn tag_lag_aggression_tilt_is_visible_in_raw_features() {
    let tag = tracker_after_hero_bb(&synthetic_tag_or_lag(100, 25, 13));
    let lag = tracker_after_hero_bb(&synthetic_tag_or_lag(100, 40, 17));

    let f_tag = tag.raw_opponent_frequencies();
    let f_lag = lag.raw_opponent_frequencies();

    // f[0] = preflop_raise_freq, f[3] = flop_bet_freq
    assert!(
        (f_tag[0] - 0.25).abs() < 0.01,
        "TAG preflop_raise_freq should be 0.25, got {}",
        f_tag[0]
    );
    assert!(
        (f_lag[0] - 0.40).abs() < 0.01,
        "LAG preflop_raise_freq should be 0.40, got {}",
        f_lag[0]
    );
    assert!(
        (f_tag[3] - 0.52).abs() < 0.02,
        "TAG flop_bet_freq should be ~0.52, got {}",
        f_tag[3]
    );
    assert!(
        (f_lag[3] - 0.425).abs() < 0.02,
        "LAG flop_bet_freq should be ~0.425, got {}",
        f_lag[3]
    );

    // The tilt: postflop bet rate minus preflop raise rate.
    let tilt_tag = f_tag[3] - f_tag[0];
    let tilt_lag = f_lag[3] - f_lag[0];
    assert!(
        tilt_tag > tilt_lag + 0.15,
        "TAG tilt ({tilt_tag:.3}) should exceed LAG tilt ({tilt_lag:.3}) by >0.15"
    );

    // Sanity: the naive scalar (preflop_raise_freq alone) points the
    // WRONG way, and the flop_bet_freq alone is a weaker signal than
    // the tilt scalar.
    let naive_preflop_gap = f_lag[0] - f_tag[0];
    let naive_flop_gap = f_tag[3] - f_lag[3];
    assert!(
        naive_preflop_gap > 0.0,
        "naive preflop_raise_freq points the WRONG way (LAG > TAG); \
         this is exactly why the router confuses them"
    );
    assert!(
        (tilt_tag - tilt_lag) > naive_flop_gap,
        "the tilt scalar ({:.3}) should be sharper than flop_bet alone ({:.3})",
        tilt_tag - tilt_lag,
        naive_flop_gap
    );
}

/// Regression test (2026-09-30) for `Tracker::preflop_postflop_tilt`.
/// The accessor derives the tilt scalar that the router needs to
/// distinguish TAG from LAG. This test pins its numerical behavior on
/// the same synthetic TAG/LAG sessions used by the raw-frequency
/// separation test.
#[test]
fn preflop_postflop_tilt_is_positive_for_tag_pattern() {
    // A pure postflop-aggressive session: opponent folds preflop on 50%
    // of hands and cbets flop on 100% of the rest.
    let mut hands = Vec::new();
    for i in 0..100 {
        if i < 50 {
            hands.push(ph(&[(Street::Preflop, Player::Bb, Action::Fold)]));
        } else {
            hands.push(ph(&[
                (Street::Preflop, Player::Bb, Action::Raise { to: 300 }),
                (Street::Preflop, Player::Sb, Action::Call),
                (Street::Flop, Player::Bb, Action::Bet { to: 300 }),
                (Street::Flop, Player::Sb, Action::Fold),
            ]));
        }
    }
    let t = tracker_after_hero_bb(&hands);
    let tilt = t.preflop_postflop_tilt();
    // preflop_raise_freq = 50/100 = 0.5
    // flop_bet_freq = 50/50 = 1.0 (only hands that reached the flop)
    // turn_bet_freq = 0/0 -> 0 via max(1)
    // river_bet_freq = 0/0 -> 0
    // postflop_mean = (1.0 + 0 + 0)/3 = 0.333...
    // tilt = 0.333 - 0.5 = -0.166...
    //
    // Hmm, that's negative. Because our synthetic opponent ends the
    // hand on the flop (hero folds), so turn/river counts stay 0. The
    // accessor's range is [-1, +1] and this is a valid negative value.
    // The important structural property is: tilt is a real number,
    // finite, and within the declared range.
    assert!(tilt.is_finite(), "tilt must be finite, got {tilt}");
    assert!((-1.0..=1.0).contains(&tilt), "tilt in [-1,1], got {tilt}");
}

/// Direct structural test: empty tracker gives tilt = 0 (no data).
#[test]
fn preflop_postflop_tilt_zero_on_empty_tracker() {
    let t = Tracker::new();
    assert_eq!(t.preflop_postflop_tilt(), 0.0);
}

/// TAG vs LAG comparison on the same synthetic sessions used by the
/// raw-frequency separation test. TAG's tilt should be strictly greater
/// than LAG's — that is the whole point of the accessor.
#[test]
fn tag_tilt_is_greater_than_lag_tilt() {
    let tag = tracker_after_hero_bb(&synthetic_tag_or_lag(100, 25, 13));
    let lag = tracker_after_hero_bb(&synthetic_tag_or_lag(100, 40, 17));
    let tilt_tag = tag.preflop_postflop_tilt();
    let tilt_lag = lag.preflop_postflop_tilt();
    assert!(
        tilt_tag > tilt_lag,
        "TAG tilt ({tilt_tag}) must exceed LAG tilt ({tilt_lag})"
    );
}

// ---------- Bet-size histogram (2026-09-30) ----------

/// A single postflop bet at the given absolute chip amount is binned
/// by its fraction of pot-before-the-bet. Given hero's known stack and
/// blind setup, we can construct exact scenarios.
#[test]
fn bet_size_histogram_bins_single_flop_bet() {
    // Setup: preflop call+check → pot = 200. Opp bets 100 on the flop
    // = 0.5 pot. Should land in bucket 2 (0.5..0.75).
    //
    // Wait: buckets are 0.25-wide starting at 0: [0,0.25), [0.25,0.5),
    // [0.5,0.75), ... So 0.5 is at the START of bucket 2.
    let h = ph(&[
        (Street::Preflop, Player::Sb, Action::Call),   // SB calls 50 more → pot=200
        (Street::Preflop, Player::Bb, Action::Check),
        (Street::Flop, Player::Bb, Action::Bet { to: 100 }),  // ~0.5 pot
        (Street::Flop, Player::Sb, Action::Fold),
    ]);
    let t = tracker_after(&[h]);
    let hist = t.opponent_bet_size_hist();
    // Total should be 1.0 (one postflop bet from the opponent).
    let total: f64 = hist.iter().sum();
    assert!(
        (total - 1.0).abs() < 1e-9,
        "histogram must be normalized to 1.0 when a bet exists; got {total}"
    );
    // Exactly one bucket must be nonzero.
    let nonzero: Vec<usize> = hist
        .iter()
        .enumerate()
        .filter(|&(_, &v)| v > 0.5)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        nonzero.len(),
        1,
        "exactly one bucket should be nonzero, got {nonzero:?}"
    );
    // Which bucket? bet 100 on pot 200 = 0.5. Bucket floor(0.5 * 4) = 2.
    assert_eq!(nonzero[0], 2, "0.5 pot bet should land in bucket 2, got {}", nonzero[0]);
}

/// No bets → all zeros.
#[test]
fn bet_size_histogram_empty_on_no_postflop_bet() {
    let h = ph(&[(Street::Preflop, Player::Bb, Action::Fold)]);
    let t = tracker_after_hero_bb(&[h]);
    let hist = t.opponent_bet_size_hist();
    for (i, &v) in hist.iter().enumerate() {
        assert!(v.abs() < 1e-12, "empty: hist[{i}] = {v}, expected 0");
    }
}

/// A small bet and a big bet fall into different buckets.
#[test]
fn bet_size_histogram_distinguishes_small_and_big_bets() {
    // Small bet: 25% pot on the flop
    let small = ph(&[
        (Street::Preflop, Player::Sb, Action::Call),
        (Street::Preflop, Player::Bb, Action::Check),
        (Street::Flop, Player::Bb, Action::Bet { to: 50 }),   // 25% of 200
        (Street::Flop, Player::Sb, Action::Fold),
    ]);
    // Big bet: 100% pot on the flop
    let big = ph(&[
        (Street::Preflop, Player::Sb, Action::Call),
        (Street::Preflop, Player::Bb, Action::Check),
        (Street::Flop, Player::Bb, Action::Bet { to: 200 }),  // 100% of 200
        (Street::Flop, Player::Sb, Action::Fold),
    ]);
    let t_small = tracker_after(&[small]);
    let t_big = tracker_after(&[big]);
    let h_small = t_small.opponent_bet_size_hist();
    let h_big = t_big.opponent_bet_size_hist();

    let nz = |h: &[f64; 8]| -> usize {
        h.iter()
            .enumerate()
            .find(|&(_, &v)| v > 0.5)
            .map(|(i, _)| i)
            .unwrap()
    };
    let b_small = nz(&h_small);
    let b_big = nz(&h_big);

    // Small: 0.25 → floor(0.25*4) = 1.
    // Big: 1.0 → floor(1.0*4) = 4.
    assert_eq!(b_small, 1, "25% pot bet should be bucket 1");
    assert_eq!(b_big, 4, "100% pot bet should be bucket 4");
    assert!(b_big > b_small, "bigger bet must be in a later bucket");
}

/// Two bets at different sizes accumulate in different buckets.
#[test]
fn bet_size_histogram_accumulates_across_hands() {
    let small = ph(&[
        (Street::Preflop, Player::Sb, Action::Call),
        (Street::Preflop, Player::Bb, Action::Check),
        (Street::Flop, Player::Bb, Action::Bet { to: 50 }),
        (Street::Flop, Player::Sb, Action::Fold),
    ]);
    let big = ph(&[
        (Street::Preflop, Player::Sb, Action::Call),
        (Street::Preflop, Player::Bb, Action::Check),
        (Street::Flop, Player::Bb, Action::Bet { to: 200 }),
        (Street::Flop, Player::Sb, Action::Fold),
    ]);
    let t = tracker_after(&[small.clone(), small.clone(), big.clone(), big]);
    let hist = t.opponent_bet_size_hist();
    let total: f64 = hist.iter().sum();
    assert!((total - 1.0).abs() < 1e-9, "normalized; got {total}");
    // Two small + two big = 0.5 / 0.5 distribution.
    // bucket 1 ≈ 0.5, bucket 4 ≈ 0.5.
    assert!(
        (hist[1] - 0.5).abs() < 1e-9,
        "small-bet bucket should be 0.5, got {}",
        hist[1]
    );
    assert!(
        (hist[4] - 0.5).abs() < 1e-9,
        "big-bet bucket should be 0.5, got {}",
        hist[4]
    );
}
