//! Baseline agents (SPECS/03 §6): CallBot, RaiseBot, JamBot, RandomBot, FishBot.
//! Their policies are trivially analytic — CallBot/RandomBot implement
//! `action_probs`; the rest return NotProbabilistic except where noted.

use arrayvec::ArrayVec;

use cham_core::engine::Action;
use cham_core::obs::{Agent, AgentError, Observables, is_legal};
use cham_core::rng::Rng;

/// F6c / report-E6 diagnostic: bets FIXED pot fractions that the tiny
/// ladder never produces, to exercise the off-tree translation path.
///
/// The tiny abstraction's bet fractions are 0.5 (flop/turn) and 0.5/1.25
/// (river). This bot instead bets 0.25 / 0.6 / 1.5 of the pot, cycling by
/// decision index, whenever it faces no bet postflop. When facing a bet it
/// folds (so it never produces an on-tree raise), and preflop it calls or
/// checks. Every aggressive action it takes is therefore OFF the training
/// tree, which is exactly what `CHAM_OFFTREE_TRANSLATE` and the
/// slot-index `size_bucket` are meant to handle.
///
/// Used by `probe`/`ladder` runs named `offtree`; see
/// `docs/plans/F6C-TRANSLATE-2026-10-01.md`.
pub struct OffTreeBettor {
    /// Cycling index into `OFF_TREE_FRACS`; advanced on each postflop bet
    /// opportunity so consecutive bets use different off-tree sizes.
    cycle: std::cell::Cell<usize>,
}

/// Pot fractions the tiny ladder never produces (tiny: 0.5 / 1.25).
// 0.25 is omitted: on the tiny flop (pot 200, min bet 1 bb = 100)
// a 0.25-pot bet (50) is below the minimum and clamps to 0.5 =
// on-tree. 0.75 / 0.6 / 1.5 are all legal and all off-tree.
pub const OFF_TREE_FRACS: [f64; 3] = [0.75, 0.6, 1.5];

impl OffTreeBettor {
    pub fn new() -> OffTreeBettor {
        OffTreeBettor {
            cycle: std::cell::Cell::new(0),
        }
    }
}

impl Default for OffTreeBettor {
    fn default() -> Self {
        Self::new()
    }
}

impl Agent for OffTreeBettor {
    fn name(&self) -> &str {
        "offtree"
    }
    fn act(&mut self, obs: &Observables<'_>, _rng: &mut Rng) -> Action {
        use cham_core::engine::Street;
        // Preflop: no off-tree bet (the tiny preflop ladder is raise_fracs,
        // not bet fracs). Call or check.
        if obs.street == Street::Preflop {
            if is_legal(obs, Action::Call) {
                return Action::Call;
            }
            return Action::Check;
        }
        // Facing a bet: fold. Keeps every aggressive action OFF-tree
        // (a raise here would be an on-tree size).
        if obs.to_call > 0 {
            if is_legal(obs, Action::Fold) {
                return Action::Fold;
            }
            return Action::Call;
        }
        // Facing no bet postflop: bet an off-tree pot fraction, clamped to
        // the legal raise window.
        let i = self.cycle.get();
        self.cycle.set((i + 1) % OFF_TREE_FRACS.len());
        let frac = OFF_TREE_FRACS[i];
        let raw = (obs.pot as f64 * frac).round() as i64;
        let to = raw.clamp(obs.min_raise_to.min(obs.max_raise_to), obs.max_raise_to);
        let a = Action::Bet { to };
        if is_legal(obs, a) {
            return a;
        }
        if is_legal(obs, Action::Check) {
            return Action::Check;
        }
        Action::Call
    }
}

/// Always call (or check when facing nothing).
pub struct CallBot;
impl Agent for CallBot {
    fn name(&self) -> &str {
        "callbot"
    }
    fn act(&mut self, obs: &Observables<'_>, _rng: &mut Rng) -> Action {
        if is_legal(obs, Action::Call) {
            Action::Call
        } else {
            Action::Check
        }
    }
    fn action_probs(
        &self,
        obs: &Observables<'_>,
    ) -> Result<ArrayVec<(Action, f64), 12>, AgentError> {
        let mut out: ArrayVec<(Action, f64), 12> = ArrayVec::new();
        if is_legal(obs, Action::Call) {
            out.push((Action::Call, 1.0));
        } else {
            out.push((Action::Check, 1.0));
        }
        Ok(out)
    }
}

/// Always max raise when possible, else call/check.
pub struct RaiseBot;
impl Agent for RaiseBot {
    fn name(&self) -> &str {
        "raisebot"
    }
    fn act(&mut self, obs: &Observables<'_>, _rng: &mut Rng) -> Action {
        for l in &obs.legal {
            if matches!(l.action, Action::Bet { .. } | Action::Raise { .. }) && l.is_all_in {
                return l.action;
            }
        }
        for l in &obs.legal {
            if matches!(l.action, Action::Bet { .. } | Action::Raise { .. }) {
                return l.action;
            }
        }
        if is_legal(obs, Action::Call) {
            Action::Call
        } else {
            Action::Check
        }
    }
}

/// Jam or call all-in; folds never.
pub struct JamBot;
impl Agent for JamBot {
    fn name(&self) -> &str {
        "jamfix"
    }
    fn act(&mut self, obs: &Observables<'_>, _rng: &mut Rng) -> Action {
        for l in &obs.legal {
            if l.is_all_in {
                return l.action;
            }
        }
        if is_legal(obs, Action::Call) {
            Action::Call
        } else {
            Action::Check
        }
    }
    /// Analytic shove distribution (2026-10-03). Previously JamBot had no
    /// `action_probs`, so the exploit trainer (which consumes the opponent
    /// ONLY through `action_probs`, traversal.rs) fell back to uniform for
    /// every node — meaning `--opponent jamfix` trained against noise, not
    /// against a shove-bot. This makes the bot analytic so it can be a real
    /// training opponent (or a mixture component).
    ///
    /// Deterministic: point mass on the all-in when one is legal, else on
    /// call/check. Matches `act` exactly.
    fn action_probs(
        &self,
        obs: &Observables<'_>,
    ) -> Result<ArrayVec<(Action, f64), 12>, AgentError> {
        let mut out: ArrayVec<(Action, f64), 12> = ArrayVec::new();
        for l in &obs.legal {
            if l.is_all_in {
                out.push((l.action, 1.0));
                return Ok(out);
            }
        }
        if is_legal(obs, Action::Call) {
            out.push((Action::Call, 1.0));
        } else {
            out.push((Action::Check, 1.0));
        }
        Ok(out)
    }
}

/// Uniform over legal actions (probs analytic — used in coverage tests).
pub struct RandomBot;
impl Agent for RandomBot {
    fn name(&self) -> &str {
        "random"
    }
    fn act(&mut self, obs: &Observables<'_>, rng: &mut Rng) -> Action {
        let n = obs.legal.len();
        let i = cham_core::rng::pick(rng, n);
        obs.legal[i].action
    }
    fn action_probs(
        &self,
        obs: &Observables<'_>,
    ) -> Result<ArrayVec<(Action, f64), 12>, AgentError> {
        let n = obs.legal.len() as f64;
        let mut out: ArrayVec<(Action, f64), 12> = ArrayVec::new();
        for l in &obs.legal {
            out.push((l.action, 1.0 / n));
        }
        Ok(out)
    }
}

/// Passive caller that calls far too wide but folds occasionally on big bets.
pub struct FishBot;
impl Agent for FishBot {
    fn name(&self) -> &str {
        "fish"
    }
    fn act(&mut self, obs: &Observables<'_>, rng: &mut Rng) -> Action {
        // call anything under half pot; otherwise 80/20 call/fold
        if obs.to_call > 0 && obs.to_call * 2 >= obs.pot {
            let u = cham_core::rng::next_f64(rng);
            if u < 0.2 && is_legal(obs, Action::Fold) {
                return Action::Fold;
            }
        }
        if is_legal(obs, Action::Call) {
            Action::Call
        } else {
            Action::Check
        }
    }
    fn action_probs(
        &self,
        obs: &Observables<'_>,
    ) -> Result<ArrayVec<(Action, f64), 12>, AgentError> {
        let mut out: ArrayVec<(Action, f64), 12> = ArrayVec::new();
        if !(obs.to_call > 0 && obs.to_call * 2 >= obs.pot) {
            if is_legal(obs, Action::Call) {
                out.push((Action::Call, 1.0));
                return Ok(out);
            }
            out.push((Action::Check, 1.0));
            return Ok(out);
        }
        if is_legal(obs, Action::Fold) {
            out.push((Action::Fold, 0.2));
        }
        if is_legal(obs, Action::Call) {
            out.push((
                Action::Call,
                if is_legal(obs, Action::Fold) {
                    0.8
                } else {
                    1.0
                },
            ));
        }
        if out.is_empty() {
            out.push((Action::Check, 1.0));
        }
        Ok(out)
    }
}
