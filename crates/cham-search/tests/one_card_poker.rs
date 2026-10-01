//! One-card-poker acceptance test for the F2 class-conditioned solver
//! (2026-10-01).
//!
//! ## What this proves
//!
//! The pre-F2 solver computed a strategy σ(path) shared across hero's
//! own strength classes. On a genuine poker river — where the nuts and a
//! bluff-catcher at the same public path want different actions — that
//! cannot be right. This test uses a small, fully-analytic spot with a
//! known structural Nash shape, and asserts:
//!
//!   1. The class-conditioned solver produces DIFFERENT strategies for
//!      the two hero classes at the same public decision node.
//!   2. The strong class aggresses (bets/jams) more than the weak class.
//!
//! The pre-F2 solver CANNOT pass this test because its output at a path
//! is one distribution, not one-per-class.
//!
//! ## The spot
//!
//! River, pot = 1 bb, both stacks 1 bb, one bet size (1 bb → pot-sized;
//! `Subgame::tree` also includes a `jam` action which lands on the same
//! 1-bb investment, since stack = 1). Hero range = {nuts (0.5), air
//! (0.5)}. Villain range = {bluff-catcher (0.5)} (a single class).

use cham_search::prior::PriorStrats;
use cham_search::solve::solve;
use cham_search::subgame::{Class, Subgame};
use cham_search::trigger::SolverChoice;

fn hero_strong() -> Class {
    Class {
        weight: 0.5,
        strength: 0.9,
    }
}
fn hero_weak() -> Class {
    Class {
        weight: 0.5,
        strength: 0.1,
    }
}
fn villain_mid() -> Class {
    Class {
        weight: 1.0,
        strength: 0.5,
    }
}

#[test]
fn class_conditioned_solver_plays_poker_not_a_shared_strategy() {
    let sg = Subgame::build(
        vec![hero_strong(), hero_weak()],
        vec![villain_mid()],
        1.0,    // pot_bb
        1.0,    // stack_bb
        &[1.0], // bet_fracs
    )
    .expect("subgame");

    let prior = PriorStrats::empty();
    let result = solve(&sg, &prior, &SolverChoice::Rnr { p: 0.9 }, 2000).expect("solve");

    let class_strats = result
        .our_class_strategy
        .as_ref()
        .expect("F2 solver must emit per-class hero strategy");

    let root_actions = root_hero_actions(&sg);
    let strong = class_strats
        .get(&(String::new(), 0u8, 0usize))
        .expect("strong hero class at root");
    let weak = class_strats
        .get(&(String::new(), 0u8, 1usize))
        .expect("weak hero class at root");

    // Aggregate aggression = probability of any non-"check" action.
    // With pot=stack=1 the tree contains both "bet1" and "jam" (same
    // 1-bb investment), so an aggregate metric avoids over-asserting on
    // which of the two actions CFR prefers.
    let check_idx = root_actions.iter().position(|a| a == "check").unwrap_or(0);
    let agg = |probs: &[f64]| -> f64 {
        probs
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != check_idx)
            .map(|(_, p)| *p)
            .sum()
    };
    let agg_strong = agg(strong);
    let agg_weak = agg(weak);

    eprintln!(
        "one-card: root actions {root_actions:?}\n\
         one-card: strong probs {strong:?}  (aggression {agg_strong:.3})\n\
         one-card: weak   probs {weak:?}  (aggression {agg_weak:.3})"
    );

    // (1) Class conditioning is real: the two classes bet differently.
    let max_diff = (0..root_actions.len())
        .map(|i| (strong[i] - weak[i]).abs())
        .fold(0.0f64, f64::max);
    assert!(
        max_diff > 0.05,
        "class-conditioned solver must distinguish the two classes: max |Δ| = {max_diff:.3}"
    );

    // (2) Strong class aggresses more than weak.
    assert!(
        agg_strong > agg_weak + 0.1,
        "strong must aggress more than weak: {agg_strong:.3} vs {agg_weak:.3}"
    );

    // (3) Strong class aggresses (value-betting the nuts).
    assert!(
        agg_strong > 0.9,
        "strong class must aggression-bet the nuts: aggression = {agg_strong:.3}"
    );

    // (4) Sanity: per-class strategies are proper distributions.
    for ((_path, _player, _c), probs) in class_strats.iter() {
        let s: f64 = probs.iter().sum();
        assert!(
            (s - 1.0).abs() < 1e-6,
            "per-class strategy must sum to 1: sum = {s}"
        );
        assert!(
            probs.iter().all(|&p| p >= -1e-9),
            "no negative probabilities in per-class strategy"
        );
    }
}

/// Root hero decision node's action labels, from the tree the solver built.
fn root_hero_actions(sg: &Subgame) -> Vec<String> {
    match sg.tree() {
        cham_search::subgame::Node::Decision { actions, .. } => actions,
        _ => panic!("root is a terminal — spot is degenerate"),
    }
}
