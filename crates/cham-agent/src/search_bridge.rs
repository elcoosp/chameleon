//! Bridge from the agent pipeline to the river solver (F1, 2026-10-01).
//!
//! The pipeline has always had an `AgentMode.search` field and a
//! `DecisionTrace.search` slot, but neither was ever populated — the
//! solver was dead code on the live path
//! (`COMPETITIVE-REVIEW-2026-10-01.md`, finding F1).
//!
//! This module assembles a river subgame from the current
//! `Observables` and the tracker-derived opponent model, invokes the
//! (now class-conditioned, F2) solver, and returns the class-conditioned
//! strategy for the acting hero.
//!
//! ## Scope of this first pass
//!
//! The subgame build is deliberately conservative. Hero is collapsed to
//! a single class (weight 1.0, strength = river equity of the actual
//! hole cards). Villain is collapsed to K classes spread uniformly over
//! [0, 1] with equal weight — a broad, agnostic model. Later work will
//! replace the villain spread with the tracker-derived range.
//!
//! The output of `try_solve` is the sampled action plus diagnostic
//! telemetry for `DecisionTrace.search`. When the solver does not fire,
//! or when the tree cannot be built, or when the solve is truncated,
//! the caller keeps its pre-search decision.

use cham_core::engine::Action;
use cham_core::obs::{Observables, is_legal};
use cham_engine::encoder::ActionSeq;

use crate::modes::AgentMode;
use crate::tracker::Tracker;
use cham_search::subgame::Class;

/// Number of villain strength classes used for the subgame build.
pub const DEFAULT_VILLAIN_CLASSES: usize = 3;

/// Villain range spread width. 1.0 = uniform over the whole equity range;
/// 0.3 = tight around the middle. Kept > 0 in this first pass so the
/// solver sees a non-degenerate villain distribution.
pub const VILLAIN_SPREAD: f64 = 1.0;

/// Outcome of a successful search. `action` is the class-conditioned
/// choice for hero's actual class; `distribution` is that class's
/// strategy over the root legal actions (indexed by the input slice).
#[derive(Clone, Debug)]
pub struct SearchOutcome {
    pub action: Action,
    pub distribution: Vec<f64>,
    pub solver: String,
    pub iters: u32,
    pub truncated: bool,
    pub lbr: f64,
}

/// Configuration captured at decision time from `AgentMode`. Kept
/// separate from `cham_search::trigger::SearchConfig` so the agent does
/// not have to link the full trigger module into the hot path type.
#[derive(Clone, Debug)]
pub struct SearchBridgeCfg {
    pub enabled: bool,
    pub solver: cham_search::trigger::SolverChoice,
    pub iters: u32,
    pub min_pot_bb: f64,
    pub river_only: bool,
}

impl SearchBridgeCfg {
    /// Read the search config from an `AgentMode`. Returns `None` when
    /// search is disabled or unrecognized.
    pub fn from_mode(mode: &AgentMode) -> Option<SearchBridgeCfg> {
        if !mode.search.enabled {
            return None;
        }
        let solver = match mode.search.solver.as_str() {
            "Fmbr" | "fmbr" => cham_search::trigger::SolverChoice::Fmbr,
            "ReachGadget" | "reach-gadget" | "gadget" => {
                cham_search::trigger::SolverChoice::ReachGadget
            }
            "Rnr" | "rnr" | "" => cham_search::trigger::SolverChoice::Rnr { p: 0.9 },
            _ => cham_search::trigger::SolverChoice::Rnr { p: 0.9 },
        };
        Some(SearchBridgeCfg {
            enabled: true,
            solver,
            iters: 400,
            min_pot_bb: 2.0,
            river_only: true,
        })
    }
}

/// Build a villain range from the tracker's observed opponent behaviour.
///
/// F1 upgrade (2026-10-01): replaces the previous uniform K-class spread.
/// The uniform spread put 1/3 of villain mass at strength 1.0, which is a
/// monster range, not a caller's range. Against a calling-heavy opponent
/// (callbot, arch:station) that made the solver refuse to value-bet and
/// cost ~−11 000 mb/seating vs search-OFF (`F1-SEARCH-CORRECTED-2026-10-01.md`).
///
/// Two signals drive the shape:
///   * `showdown_reach_freq` (f[6]) = how often the opponent goes to
///     showdown. High → wide range → shift the mean strength down.
///   * `river_bet_freq` (f[5]) = how often the opponent bets the river.
///     High → polar range (strong + bluffs) → increase the spread and
///     the extreme mass.
///
/// Falls back to a symmetric 3-class range around 0.5 when the tracker
/// has fewer than `MIN_HANDS_FOR_RANGE` hands observed (a fresh session
/// has no information to model).
pub const MIN_HANDS_FOR_RANGE: u64 = 30;

pub fn villain_range_from_tracker(tracker: &Tracker) -> Vec<Class> {
    let f = tracker.raw_opponent_frequencies();
    let showdown_reach = f[6].clamp(0.0, 1.0);
    let river_bet = f[5].clamp(0.0, 1.0);

    if tracker.hands < MIN_HANDS_FOR_RANGE {
        // No information yet: a symmetric spread around 0.5 is honest.
        return vec![
            Class {
                weight: 1.0 / 3.0,
                strength: 0.20,
            },
            Class {
                weight: 1.0 / 3.0,
                strength: 0.50,
            },
            Class {
                weight: 1.0 / 3.0,
                strength: 0.80,
            },
        ];
    }

    // "Wideness": high showdown reach = wide range = lower mean strength.
    let wideness = showdown_reach;
    let mean_strength = (0.70 - 0.55 * wideness).clamp(0.15, 0.70);

    // "Polarity": high river bet = polar range = wider spread + more
    // extreme mass.
    let polarity = (river_bet / 0.6).clamp(0.0, 1.0);
    let spread = (0.20 + 0.25 * polarity).clamp(0.20, 0.45);
    let extreme_mass = (0.30 + 0.30 * polarity).clamp(0.30, 0.60);

    let low = (mean_strength - spread).clamp(0.0, 1.0);
    let mid = mean_strength;
    let high = (mean_strength + spread).clamp(0.0, 1.0);
    let half_extreme = extreme_mass / 2.0;
    let mid_mass = 1.0 - extreme_mass;

    vec![
        Class {
            weight: half_extreme,
            strength: low,
        },
        Class {
            weight: mid_mass,
            strength: mid,
        },
        Class {
            weight: half_extreme,
            strength: high,
        },
    ]
}

/// Try to run a live solve at the current decision.
///
/// Returns `Some(outcome)` only when every prerequisite is satisfied:
/// search enabled, river street, pot meets the minimum, the subgame
/// builds, and the solve is not truncated. Otherwise returns `None` and
/// the caller keeps its pre-search decision.
pub fn try_solve(
    cfg: &SearchBridgeCfg,
    // F1 first pass: `tracker`, `encoder`, and `robust` are accepted for
    // forward compatibility but not yet consumed — the villain range is
    // an agnostic K-class uniform spread rather than tracker-derived.
    // A later pass will read tracker frequencies and the robust policy
    // reach to build a genuine villain range.
    tracker: &Tracker,
    _encoder: &cham_engine::encoder::Encoder,
    _robust: &cham_blueprint::policy::BlueprintPolicy,
    obs: &Observables<'_>,
    _seq: &ActionSeq,
) -> Option<SearchOutcome> {
    if !cfg.enabled {
        return None;
    }
    if cfg.river_only && obs.street != cham_core::engine::Street::River {
        return None;
    }
    if obs.pot_bb() < cfg.min_pot_bb {
        return None;
    }
    // F1-A/B guard: refuse any state the subgame tree does not model.
    if obs.to_call != 0 {
        return None;
    }

    // Hero classes: one class, weight 1.0, strength = hero's current
    // strength on the river board (0..1).
    let board: Vec<cham_core::card::Card> = obs.board[..obs.board_len as usize].to_vec();
    let hero_strength = cham_core::eval::strength_now(obs.hole, &board);
    let hero_classes = vec![cham_search::subgame::Class {
        weight: 1.0,
        strength: hero_strength,
    }];

    // F1 upgrade (2026-10-01): tracker-derived villain range. See
    // `villain_range_from_tracker` for the derivation and the fallback.
    let villain_classes: Vec<cham_search::subgame::Class> = villain_range_from_tracker(tracker);

    let pot_bb = obs.pot_bb().max(1.0);
    let stack_bb = obs.effective_stack_bb().max(0.5);
    let bet_fracs = [0.5, 1.0];

    let sg = cham_search::subgame::Subgame::build(
        hero_classes,
        villain_classes,
        pot_bb,
        stack_bb,
        &bet_fracs,
    )
    .ok()?;

    let prior = cham_search::prior::PriorStrats::empty();
    let result = cham_search::solve::solve(&sg, &prior, &cfg.solver, cfg.iters).ok()?;

    if result.truncated {
        return None;
    }

    // Pick the hero class-conditioned strategy at the root. Under F2 the
    // solver emits `our_class_strategy` keyed by (path, player, class).
    // Our single hero class has index 0 and the root path is "".
    let root = result
        .our_class_strategy
        .as_ref()
        .and_then(|cs| cs.get(&(String::new(), 0u8, 0usize)).cloned())
        .or_else(|| result.our_strategy.get("").cloned())?;

    // Map the solver's action ordering to the live legal actions. The
    // solver's tree uses labels ("check", "bet0.5", "bet1", "jam"); the
    // live node's legal actions are `obs.legal`. Match by best-effort
    // label comparison: "check" matches Check; "bet*" or "jam" match
    // Bet/Raise. Fall back to the highest-probability legal action.
    let (action, dist) = map_to_legal(obs, &root, &sg)?;

    Some(SearchOutcome {
        action,
        distribution: dist,
        solver: format!("{:?}", cfg.solver),
        iters: result.iters,
        truncated: false,
        lbr: result.lbr_gap.0,
    })
}

/// Map the solver's root distribution onto the live legal action list.
/// Returns the argmax legal action and the distribution reordered to
/// match the live slots.
fn map_to_legal(
    obs: &Observables<'_>,
    solver_dist: &[f64],
    sg: &cham_search::subgame::Subgame,
) -> Option<(Action, Vec<f64>)> {
    let legal: Vec<Action> = obs.legal.iter().map(|l| l.action).collect();
    if legal.is_empty() {
        return None;
    }
    let solver_actions = root_action_labels(sg);
    // Build a live-slot distribution by matching solver labels to legal
    // actions heuristically: "check" → Check; anything else → Bet/Raise
    // of the first legal aggressive action. If no aggressive action is
    // legal, collapse all mass onto the non-check legal action.
    let check_slot = legal
        .iter()
        .position(|a| matches!(a, Action::Check))
        .unwrap_or(0);
    let fold_slot = legal.iter().position(|a| matches!(a, Action::Fold));
    let call_slot = legal.iter().position(|a| matches!(a, Action::Call));
    let aggressive_slot = legal
        .iter()
        .position(|a| matches!(a, Action::Bet { .. } | Action::Raise { .. }));

    let mut live_dist = vec![0.0f64; legal.len()];
    for (i, label) in solver_actions.iter().enumerate() {
        let p = solver_dist.get(i).copied().unwrap_or(0.0);
        let target = if label == "check" {
            Some(check_slot)
        } else if label.starts_with("bet") || label == "jam" {
            aggressive_slot
        } else if label == "fold" {
            fold_slot
        } else if label == "call" {
            call_slot
        } else {
            None
        };
        if let Some(slot) = target {
            live_dist[slot] += p;
        }
    }
    let total: f64 = live_dist.iter().sum();
    if total <= 1e-12 {
        // Nothing mapped — refuse rather than produce a uniform.
        return None;
    }
    for v in live_dist.iter_mut() {
        *v /= total;
    }
    // Pick argmax over the live distribution, filtered by legal action.
    let mut best = 0usize;
    for (i, &p) in live_dist.iter().enumerate() {
        if p > live_dist[best] && is_legal(obs, legal[i]) {
            best = i;
        }
    }
    Some((legal[best], live_dist))
}

/// Extract the root decision node's action labels from a subgame's tree.
fn root_action_labels(sg: &cham_search::subgame::Subgame) -> Vec<String> {
    match sg.tree() {
        cham_search::subgame::Node::Decision { actions, .. } => actions,
        _ => Vec::new(),
    }
}

/// Convenience: does the current observation satisfy the coarse trigger
/// preconditions (used by the pipeline to avoid building a bridge when
/// it cannot fire)?
pub fn would_trigger(cfg: &SearchBridgeCfg, obs: &Observables<'_>) -> bool {
    cfg.enabled
        && (!cfg.river_only || obs.street == cham_core::engine::Street::River)
        && obs.pot_bb() >= cfg.min_pot_bb
        // 2026-10-01 (F1-A/B): the solver tree is rooted at hero-acts-
        // first with actions [check, bet0.5, bet1, jam]. If the hero
        // faces a bet (to_call > 0), the state has fold/call/raise legal
        // and the root distribution has no honest mapping onto it. The
        // 2026-10-01 A/B measured this as a −5 000 mb/seating regression
        // against every opponent. Restrict the trigger to the exact
        // state class the subgame models. See
        // `F1-SEARCH-NEGATIVE-2026-10-01.md`.
        && obs.to_call == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_mode_none_when_disabled() {
        let mut mode = crate::modes::AgentMode::argmax();
        mode.search.enabled = false;
        assert!(SearchBridgeCfg::from_mode(&mode).is_none());
    }

    #[test]
    fn from_mode_some_when_enabled() {
        let mut mode = crate::modes::AgentMode::argmax();
        mode.search.enabled = true;
        mode.search.solver = "Fmbr".into();
        let cfg = SearchBridgeCfg::from_mode(&mode).expect("cfg");
        assert!(cfg.enabled);
        assert!(matches!(
            cfg.solver,
            cham_search::trigger::SolverChoice::Fmbr
        ));
    }
}
