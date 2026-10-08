//! `Observables::with_hole` preserves every public field except `hole`.
//! Used by the PCS trainer to derive a per-combo encoder key from a
//! single engine state.

use cham_core::card::{Card, Deck, Hand2};
use cham_core::engine::State;
use cham_core::engine::config::EngineConfig;
use cham_core::obs::{Observables, Player};

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

#[test]
fn with_hole_preserves_all_but_hole() {
    let prefix = [
        Card(0),
        Card(2),
        Card(4),
        Card(6),
        Card(8),
        Card(10),
        Card(12),
        Card(14),
        Card(16),
    ];
    let st = State::new(CFG, Deck::with_prefix(&prefix)).expect("state");
    let obs = Observables::view(&st, Player::Sb);
    let original_hole = obs.hole;

    let new_hole = Hand2::new(Card(20), Card(22));
    let obs2 = obs.with_hole(new_hole);

    assert_eq!(obs2.hole, new_hole, "hole not replaced");
    assert_ne!(obs2.hole, original_hole, "test is degenerate: holes equal");
    assert_eq!(obs2.player, obs.player);
    assert_eq!(obs2.street, obs.street);
    assert_eq!(obs2.board, obs.board);
    assert_eq!(obs2.board_len, obs.board_len);
    assert_eq!(obs2.pot, obs.pot);
    assert_eq!(obs2.to_call, obs.to_call);
    assert_eq!(obs2.current_bet, obs.current_bet);
    assert_eq!(obs2.min_raise_to, obs.min_raise_to);
    assert_eq!(obs2.max_raise_to, obs.max_raise_to);
    assert_eq!(obs2.last_full_raise, obs.last_full_raise);
    assert_eq!(obs2.stack, obs.stack);
    assert_eq!(obs2.effective_stack, obs.effective_stack);
    assert_eq!(obs2.stacks, obs.stacks);
    assert_eq!(obs2.legal.len(), obs.legal.len());
}

#[test]
fn with_hole_two_combos_differ_in_bucket_driver() {
    // The only field a bucket computation reads is `hole` (plus board
    // and street, both preserved). Two different holes must therefore
    // produce different `obs2`, which is what with_hole guarantees.
    let prefix = [
        Card(0),
        Card(2),
        Card(4),
        Card(6),
        Card(8),
        Card(10),
        Card(12),
        Card(14),
        Card(16),
    ];
    let st = State::new(CFG, Deck::with_prefix(&prefix)).expect("state");
    let obs = Observables::view(&st, Player::Sb);

    let a = obs.with_hole(Hand2::new(Card(20), Card(22)));
    let b = obs.with_hole(Hand2::new(Card(24), Card(26)));
    assert_ne!(a.hole, b.hole);
    assert_eq!(a.board, b.board);
}
