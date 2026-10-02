//! F6c / report-E6 (2026-10-02): the `OffTreeBettor` must produce bets at
//! pot fractions the tiny ladder never contains, and must never raise
//! (raises would be on-tree). Pins the properties the fallback-rate
//! measurement relies on.
//!
//! Actor selection uses `State::to_act()` throughout — postflop in HU the
//! BB acts first, so hardcoding a seat is wrong.

use cham_core::card::Deck;
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::obs::{Agent, Observables, Player};
use cham_core::rng::{Rng, rng_from_seed};
use cham_opponents::baselines::{OFF_TREE_FRACS, OffTreeBettor};

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

fn new_state() -> State {
    State::new(CFG, Deck::shuffled(&mut rng_from_seed(7))).expect("state")
}

fn actor(st: &State) -> Player {
    Player::from_usize(st.to_act())
}

/// Play the preflop out (call, check) to reach the flop.
fn to_flop() -> State {
    let mut st = new_state();
    while st.street() == cham_core::engine::Street::Preflop && !st.is_terminal() {
        let p = st.to_act();
        let obs = Observables::view(&st, Player::from_usize(p));
        let a = if obs.to_call > 0 {
            Action::Call
        } else {
            Action::Check
        };
        st.apply(a).expect("preflop call/check");
    }
    st
}

#[test]
fn preflop_calls_or_checks_never_raises() {
    let mut st = new_state();
    let mut bot = OffTreeBettor::new();
    let mut rng: Rng = rng_from_seed(1);
    let p = actor(&st);
    let obs = Observables::view(&st, p);
    let a = bot.act(&obs, &mut rng);
    assert!(
        matches!(a, Action::Call | Action::Check),
        "preflop should call/check, got {a:?}"
    );
}

#[test]
fn postflop_bet_is_legal_and_off_tree() {
    let mut st = to_flop();
    let mut bot = OffTreeBettor::new();
    let mut rng: Rng = rng_from_seed(2);
    let p = actor(&st);
    let obs = Observables::view(&st, p);
    // The bettor bets when facing no bet (to_call == 0).
    assert_eq!(obs.to_call, 0, "expected to face no bet on the flop");
    let a = bot.act(&obs, &mut rng);
    match a {
        Action::Bet { to } => {
            assert!(
                cham_core::obs::is_legal(&obs, a),
                "bettor produced an illegal bet {to}"
            );
            let frac = to as f64 / obs.pot as f64;
            // Not one of the tiny ladder's on-tree fracs.
            assert!(
                (frac - 0.5).abs() >= 1e-6 && (frac - 1.25).abs() >= 1e-6,
                "bet {to} (frac {frac}) is on-tree"
            );
            // Matches one of the declared off-tree fracs (or is the max
            // legal bet, if the stack is short enough to clamp).
            assert!(
                OFF_TREE_FRACS
                    .iter()
                    .any(|f| (frac - f).abs() < 0.05 || to == obs.max_raise_to),
                "bet {to} (frac {frac}) matches no off-tree frac"
            );
        }
        other => panic!("expected a bet, got {other:?}"),
    }
}

#[test]
fn facing_a_bet_folds_or_calls_never_raises() {
    let mut st = to_flop();
    // Whoever is to act puts in a legal bet.
    let p = actor(&st);
    let obs = Observables::view(&st, p);
    let bet = obs.min_raise_to.min(obs.max_raise_to).max(1);
    st.apply(Action::Bet { to: bet }).expect("opening bet");
    // The other player now faces it.
    let p2 = actor(&st);
    let obs2 = Observables::view(&st, p2);
    assert!(obs2.to_call > 0, "expected to face the bet");
    let mut bot = OffTreeBettor::new();
    let mut rng: Rng = rng_from_seed(3);
    let a = bot.act(&obs2, &mut rng);
    assert!(
        matches!(a, Action::Fold | Action::Call),
        "facing a bet should fold/call, got {a:?}"
    );
}

#[test]
fn cycle_advances_through_sizes() {
    let mut bot = OffTreeBettor::new();
    let mut seen = std::collections::BTreeSet::new();
    for seed in 0..6u64 {
        let st = to_flop();
        let p = actor(&st);
        let obs = Observables::view(&st, p);
        let mut rng: Rng = rng_from_seed(seed);
        if let Action::Bet { to } = bot.act(&obs, &mut rng) {
            seen.insert(to);
        }
    }
    assert!(seen.len() >= 2, "cycle should vary bet sizes, got {seen:?}");
}
