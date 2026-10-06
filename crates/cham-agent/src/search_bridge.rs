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
//! a single class (weight 1.0, strength = EXACT river equity of the
//! actual hole cards, §3.1). Villain uses the tracker-derived 3-class
//! range (`villain_range_from_tracker`), falling back to a symmetric
//! spread around 0.5 with < 30 hands observed. Later work will replace
//! the villain spread with blueprint-reach combo ranges (Half B).
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
/// strategy as (action, prob) pairs over REAL engine-legal actions.
#[derive(Clone, Debug)]
pub struct SearchOutcome {
    pub action: Action,
    pub distribution: Vec<(Action, f64)>,
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
    encoder: &cham_engine::encoder::Encoder,
    robust: &cham_blueprint::policy::BlueprintPolicy,
    obs: &Observables<'_>,
    seq: &ActionSeq,
    // 2026-10-06: the live State, for the safe-resolve gadget's blueprint
    // prior. `None` (tests / no-state callers) => gadget prior is empty.
    state: Option<&cham_core::engine::State>,
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

    // Hero classes: one class, weight 1.0, strength = EXACT river equity
    // of the actual hole cards (suit-aware). `strength_now` is a suit-blind
    // proxy (ignores flushes/draws); on the river the board is complete so
    // exact equity is O(1326) and affordable (§3.1/§3.2).
    let board: Vec<cham_core::card::Card> = obs.board[..obs.board_len as usize].to_vec();
    let hero_strength = if obs.board_len as usize >= 5 {
        let mut b5 = [cham_core::card::Card(0); 5];
        b5.copy_from_slice(&board[..5]);
        cham_engine::tables::river_equity(obs.hole, &b5)
    } else {
        cham_core::eval::strength_now(obs.hole, &board)
    };
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

    // Real blueprint prior (2026-10-06): with the live State, query the
    // blueprint at every solver-tree node so the prior IS the blueprint's
    // strategy. Without a State (tests / no-state callers), the prior is
    // empty and the gadget bounds against uniform.
    let prior = match state {
        Some(st) => build_blueprint_prior(&sg, st, robust, encoder, seq, obs.player),
        None => cham_search::prior::PriorStrats::empty(),
    };
    // Safe-resolving gadget (2026-10-06): give the opponent a root opt-out
    // worth their BLUEPRINT counterfactual value, which BOUNDS the
    // re-solved strategy's exploitability by the blueprint's. Without this,
    // search was +5.57 bb MORE exploitable (DEFINITIVE-RESULTS-2026-10-06.md).
    let v_bp = cham_search::solve::villain_cfv(&sg, &prior.strat);
    let sg = sg.with_opponent_optout(v_bp);
    let result = cham_search::solve::solve(&sg, &prior, &cfg.solver, cfg.iters).ok()?;

    if result.truncated {
        return None;
    }

    // Pick the hero class-conditioned strategy at the root. Under F2 the
    // solver emits `our_class_strategy` keyed by (path, player, class).
    // Our single hero class has index 0 and the root path is "".
    let hrp = hero_root_path(&sg).to_string();
    let root = result
        .our_class_strategy
        .as_ref()
        .and_then(|cs| cs.get(&(hrp.clone(), 0u8, 0usize)).cloned())
        .or_else(|| result.our_strategy.get(&hrp).cloned())?;

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

/// Convert one solver root label to a REAL engine-legal action.
///
/// Walk the solver tree from the LIVE state, querying the blueprint at each
/// decision node to build its strategy prior on the tree's action LABELS
/// (2026-10-06). This is the `v_bp` source for the safe-resolve gadget: with
/// a real prior, `villain_cfv` is the blueprint's counterfactual value, so
/// the gadget bounds the re-solved strategy by the BLUEPRINT (not uniform).
///
/// `player_of` maps a solver node's player id (0 hero, 1 villain) to a real
/// `Player`. Falls back to no entry when the blueprint has no row (unvisited)
/// or a label does not map to a legal real action.
fn build_blueprint_prior(
    sg: &cham_search::subgame::Subgame,
    state: &cham_core::engine::State,
    robust: &cham_blueprint::policy::BlueprintPolicy,
    enc0: &cham_engine::encoder::Encoder,
    seq0: &ActionSeq,
    hero: cham_core::obs::Player,
) -> cham_search::prior::PriorStrats {
    use cham_search::prior::PriorStrats;
    let mut prior = PriorStrats::empty();

    fn walk(
        node: &cham_search::subgame::Node,
        state: &cham_core::engine::State,
        robust: &cham_blueprint::policy::BlueprintPolicy,
        enc0: &cham_engine::encoder::Encoder,
        seq: &ActionSeq,
        hero: cham_core::obs::Player,
        path: &str,
        prior: &mut PriorStrats,
    ) {
        let (player_id, actions, children) = match node {
            cham_search::subgame::Node::Decision {
                player,
                actions,
                children,
            } => (*player, actions, children),
            _ => return,
        };
        // Map solver player 0/1 -> the real seat. Solver is hero-centric:
        // 0 = hero, 1 = villain (the other seat).
        let p = if player_id == 0 {
            hero
        } else {
            match hero {
                cham_core::obs::Player::Sb => cham_core::obs::Player::Bb,
                cham_core::obs::Player::Bb => cham_core::obs::Player::Sb,
            }
        };
        let obs = Observables::view(state, p);
        let mut enc = enc0.clone();
        if let Some(d) = robust.strategy(&obs, &mut enc, seq) {
            let slots = enc.slots(&obs, seq);
            let mut probs = vec![0.0f64; actions.len()];
            for (ai, label) in actions.iter().enumerate() {
                if let Some(real) = label_to_action(&obs, label) {
                    if let Some(si) = slots.iter().position(|s| s.action == real) {
                        probs[ai] = d.get(si).copied().unwrap_or(0.0);
                    }
                }
            }
            let tot: f64 = probs.iter().sum();
            if tot > 1e-9 {
                for x in probs.iter_mut() {
                    *x /= tot;
                }
                prior.set(path, probs);
            }
        }
        // Recurse, advancing the state along each child's real action.
        for (label, child) in actions.iter().zip(children.iter()) {
            if let Some(real) = label_to_action(&obs, label) {
                let mut next = *state;
                if next.apply(real).is_ok() {
                    let mut seq2 = *seq;
                    enc0.record(&obs, p, real, &mut seq2);
                    let cp = if path.is_empty() {
                        label.clone()
                    } else {
                        format!("{path}/{label}")
                    };
                    walk(child, &next, robust, enc0, &seq2, hero, &cp, prior);
                }
            }
        }
    }

    // The solver tree is rooted hero-first; if the gadget is on, the root is
    // the villain opt-out and we start the walk at the "play" child.
    let tree = sg.tree();
    let start = match &tree {
        cham_search::subgame::Node::Decision {
            actions, children, ..
        } if actions.first().map(|a| a.as_str()) == Some("terminate") => {
            children.get(1).unwrap_or(&tree)
        }
        other => other,
    };
    // Match the solver's `collect` path convention: hero's root is "" without
    // the gadget, "play" with it (the gadget's root is terminate|play).
    let start_path = hero_root_path(sg);
    walk(
        start, state, robust, enc0, seq0, hero, start_path, &mut prior,
    );
    prior
}

/// Solver labels: "check", "bet<frac>" (e.g. "bet0.5"), "jam".
/// Old behaviour collapsed every bet/jam onto the FIRST aggressive entry
/// of `obs.legal` (the min-bet) — a 1 bb "value bet" (§3.1). This maps
/// each label to its intended size: `bet<f>` → `f × pot` clamped to
/// `[min_to, max_to]`, `jam` → `max_raise_to`. Returns `None` for unknown
/// labels (refuse, never remap) — the caller skips unmapped mass.
pub fn label_to_action(obs: &Observables<'_>, label: &str) -> Option<Action> {
    let max_to = obs.max_raise_to;
    let min_to = obs.min_raise_to.min(max_to);
    match label {
        "check" => Some(Action::Check),
        "jam" => (max_to > obs.current_bet).then_some(Action::Bet { to: max_to }),
        l if l.starts_with("bet") => {
            let f: f64 = l[3..].parse().ok()?;
            if !f.is_finite() || f <= 0.0 {
                return None;
            }
            // to_call == 0 guard: solver tree is rooted at hero-acts-first.
            let to = ((f * obs.pot as f64).floor() as i64).clamp(min_to, max_to);
            Some(Action::Bet { to })
        }
        _ => None, // unknown label (fold/call at a to_call==0 root): refuse
    }
}

/// Map the solver's root distribution onto REAL actions.
///
/// Returns the sampled-or-argmax action plus the distribution over the
/// returned action set. Mass that maps to the same real action is merged;
/// mass with no legal mapping is dropped (renormalised below). Returns
/// `None` when nothing maps (caller keeps its pre-search decision).
fn map_to_legal(
    obs: &Observables<'_>,
    solver_dist: &[f64],
    sg: &cham_search::subgame::Subgame,
) -> Option<(Action, Vec<(Action, f64)>)> {
    let mut out: Vec<(Action, f64)> = Vec::new();
    for (i, label) in root_action_labels(sg).iter().enumerate() {
        let p = solver_dist.get(i).copied().unwrap_or(0.0);
        if p <= 0.0 {
            continue;
        }
        let a = label_to_action(obs, label)?;
        if !is_legal(obs, a) {
            continue;
        }
        match out.iter_mut().find(|(b, _)| *b == a) {
            Some(e) => e.1 += p,
            None => out.push((a, p)),
        }
    }
    let total: f64 = out.iter().map(|x| x.1).sum();
    if total <= 1e-12 {
        // Nothing mapped — refuse rather than produce a uniform.
        return None;
    }
    for x in out.iter_mut() {
        x.1 /= total;
    }
    // Argmax over the mapped distribution, filtered by legality (all
    // entries are legal by construction; the guard is belt-and-braces).
    let mut best = 0usize;
    for (i, (a, p)) in out.iter().enumerate() {
        if *p > out[best].1 && is_legal(obs, *a) {
            best = i;
        }
    }
    let action = out[best].0;
    Some((action, out))
}

/// Extract the root decision node's action labels from a subgame's tree.
fn root_action_labels(sg: &cham_search::subgame::Subgame) -> Vec<String> {
    match sg.tree() {
        // Gadget root is [terminate | play]; hero's decision is the play child.
        cham_search::subgame::Node::Decision {
            actions, children, ..
        } if actions.first().map(|a| a.as_str()) == Some("terminate") => match children.get(1) {
            Some(cham_search::subgame::Node::Decision { actions, .. }) => actions.clone(),
            _ => Vec::new(),
        },
        cham_search::subgame::Node::Decision { actions, .. } => actions,
        _ => Vec::new(),
    }
}

/// The solver path string of HERO's root decision. `""` without the gadget;
/// `"play"` when the gadget's root opt-out is present (mirrors `collect`).
fn hero_root_path(sg: &cham_search::subgame::Subgame) -> &'static str {
    match sg.tree() {
        cham_search::subgame::Node::Decision { actions, .. }
            if actions.first().map(|a| a.as_str()) == Some("terminate") =>
        {
            "play"
        }
        _ => "",
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

    /// §3.1 regression: solver labels must map to REAL sizes, never to the
    /// min-bet. River, pot 200, deep stacks, to_call == 0.
    #[test]
    fn label_to_action_maps_real_sizes() {
        use cham_core::card::Deck;
        use cham_core::engine::config::EngineConfig;
        use cham_core::engine::{Action as EAction, State};
        use cham_core::obs::Player;
        let cfg = EngineConfig {
            start_stack: 10_000,
            sb: 50,
            bb: 100,
        };
        let mut rng = cham_core::rng::rng_from_seed(42);
        let mut st = State::new(cfg, Deck::shuffled(&mut rng)).expect("state");
        // Preflop: SB calls, BB checks. Flop/turn/river: checks through.
        for a in [
            EAction::Call,
            EAction::Check, // preflop
            EAction::Check,
            EAction::Check, // flop
            EAction::Check,
            EAction::Check, // turn
        ] {
            st.apply(a).expect("legal by construction");
        }
        assert_eq!(st.street(), cham_core::engine::Street::River);
        let obs = Observables::view(&st, Player::from_usize(st.to_act()));
        assert_eq!(obs.to_call, 0);
        assert_eq!(obs.pot, 200);
        // bet1 → full pot (200), NOT the min-bet.
        let b1 = label_to_action(&obs, "bet1").expect("bet1 maps");
        assert_eq!(b1, EAction::Bet { to: 200 });
        assert_ne!(
            b1,
            EAction::Bet {
                to: obs.min_raise_to
            }
        );
        // bet0.5 → half pot (100).
        let b05 = label_to_action(&obs, "bet0.5").expect("bet0.5 maps");
        assert_eq!(b05, EAction::Bet { to: 100 });
        // jam → max_raise_to.
        let jam = label_to_action(&obs, "jam").expect("jam maps");
        assert_eq!(
            jam,
            EAction::Bet {
                to: obs.max_raise_to
            }
        );
        // unknown labels refuse.
        assert!(label_to_action(&obs, "fold").is_none());
        assert!(label_to_action(&obs, "raise-the-moon").is_none());
    }
}
