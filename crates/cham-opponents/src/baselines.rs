//! Baseline agents (SPECS/03 §6): CallBot, RaiseBot, JamBot, RandomBot, FishBot.
//! Their policies are trivially analytic — CallBot/RandomBot implement
//! `action_probs`; the rest return NotProbabilistic except where noted.

use arrayvec::ArrayVec;

use cham_core::engine::Action;
use cham_core::obs::{is_legal, Agent, AgentError, Observables};
use cham_core::rng::Rng;

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
    fn action_probs(&self, obs: &Observables<'_>) -> Result<ArrayVec<(Action, f64), 12>, AgentError> {
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
    fn action_probs(&self, obs: &Observables<'_>) -> Result<ArrayVec<(Action, f64), 12>, AgentError> {
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
    fn action_probs(&self, obs: &Observables<'_>) -> Result<ArrayVec<(Action, f64), 12>, AgentError> {
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
            out.push((Action::Call, if is_legal(obs, Action::Fold) { 0.8 } else { 1.0 }));
        }
        if out.is_empty() {
            out.push((Action::Check, 1.0));
        }
        Ok(out)
    }
}
