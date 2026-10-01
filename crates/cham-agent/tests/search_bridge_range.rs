//! F1 villain-range test (2026-10-02): the range must respond to the
//! tracker's observed opponent behaviour. Same call shape as `try_solve`
//! but only the range builder, no solver.

use cham_agent::tracker::Tracker;
use cham_core::card::Card;
use cham_core::engine::history::PublicHistory;
use cham_core::engine::{Action, Player, Street};

fn card(s: &str) -> Card {
    Card::parse(s).expect("card")
}

fn ph(actions: &[(Street, Player, Action)]) -> PublicHistory {
    PublicHistory {
        actions: actions.to_vec(),
        board: [card("2c"); 5],
        showdown_holes: [None, None],
        nets: [0, 0],
    }
}

/// A caller: raises preflop rarely, calls a lot, reaches showdown often.
fn caller_tracker() -> Tracker {
    let mut t = Tracker::new();
    for _ in 0..100 {
        let h = ph(&[
            (Street::Preflop, Player::Bb, Action::Call),
            (Street::Preflop, Player::Sb, Action::Check),
            (Street::Flop, Player::Bb, Action::Call),
            (Street::Flop, Player::Sb, Action::Bet { to: 200 }),
            (Street::Flop, Player::Bb, Action::Call),
            (Street::Turn, Player::Sb, Action::Bet { to: 400 }),
            (Street::Turn, Player::Bb, Action::Call),
            (Street::River, Player::Sb, Action::Bet { to: 800 }),
            (Street::River, Player::Bb, Action::Call),
        ]);
        t.observe_hand(&h, 0, 0);
    }
    t
}

/// An aggressor: raises a lot, bets river often.
fn aggressor_tracker() -> Tracker {
    let mut t = Tracker::new();
    for _ in 0..100 {
        let h = ph(&[
            (Street::Preflop, Player::Bb, Action::Raise { to: 300 }),
            (Street::Preflop, Player::Sb, Action::Call),
            (Street::Flop, Player::Bb, Action::Bet { to: 400 }),
            (Street::Flop, Player::Sb, Action::Call),
            (Street::Turn, Player::Bb, Action::Bet { to: 800 }),
            (Street::Turn, Player::Sb, Action::Fold),
        ]);
        t.observe_hand(&h, 0, 0);
    }
    t
}

#[test]
fn villain_range_responds_to_tracker_behaviour() {
    let caller = caller_tracker();
    let aggressor = aggressor_tracker();

    let c_range = cham_agent::search_bridge::villain_range_from_tracker(&caller);
    let a_range = cham_agent::search_bridge::villain_range_from_tracker(&aggressor);

    eprintln!("caller range:");
    for c in &c_range {
        eprintln!("  w={:.3} strength={:.3}", c.weight, c.strength);
    }
    eprintln!("aggressor range:");
    for c in &a_range {
        eprintln!("  w={:.3} strength={:.3}", c.weight, c.strength);
    }

    // Both ranges are proper distributions.
    for r in [&c_range, &a_range] {
        let s: f64 = r.iter().map(|c| c.weight).sum();
        assert!((s - 1.0).abs() < 1e-9, "weights must sum to 1, got {s}");
    }

    // The caller's mean strength must be lower (weaker range).
    let mean = |r: &[cham_search::subgame::Class]| -> f64 {
        r.iter().map(|c| c.weight * c.strength).sum()
    };
    assert!(
        mean(&c_range) < mean(&a_range),
        "caller range should be weaker than aggressor range: {:.3} vs {:.3}",
        mean(&c_range),
        mean(&a_range)
    );

    // Fresh tracker → symmetric fallback (weights 1/3 each, mean 0.5).
    let fresh = Tracker::new();
    let f_range = cham_agent::search_bridge::villain_range_from_tracker(&fresh);
    let f_mean = mean(&f_range);
    assert!(
        (f_mean - 0.5).abs() < 1e-9,
        "fresh tracker should give mean 0.5 fallback, got {f_mean}"
    );
}
