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
