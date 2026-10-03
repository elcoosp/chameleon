//! Weighted mixture of two analytic opponents (2026-10-03).
//!
//! The `mix:<wa>:<a>~<b>` opponent spec combines two `action_probs`
//! oracles by a convex combination: `p(a) = wa * p_a(a) + (1-wa) * p_b(a)`,
//! renormalized over the union of legal actions. Both children must be
//! analytic (implement `action_probs`); `MixerAgent` returns `NotProbabilistic`
//! if either child is.
//!
//! Motivation: the router caps at 4 classes, so a 5th "shove-specialist"
//! expert is impossible; the way to make the existing experts handle a
//! shove-bot is to add the shove-bot as a mixture component during their
//! training (e.g. `mix:0.75:arch:nit~jamfix`).

use arrayvec::ArrayVec;

use cham_core::engine::Action;
use cham_core::obs::{Agent, AgentError, Observables};
use cham_core::rng::{Rng, next_f64};

pub struct MixerAgent {
    a: Box<dyn Agent>,
    b: Box<dyn Agent>,
    wa: f64,
}

impl MixerAgent {
    pub fn new(a: Box<dyn Agent>, b: Box<dyn Agent>, wa: f64) -> MixerAgent {
        MixerAgent {
            a,
            b,
            wa: wa.clamp(0.0, 1.0),
        }
    }
}

impl Agent for MixerAgent {
    fn name(&self) -> &str {
        "mixer"
    }

    fn act(&mut self, obs: &Observables<'_>, rng: &mut Rng) -> Action {
        match self.action_probs(obs) {
            Ok(d) if !d.is_empty() => {
                let u = next_f64(rng);
                let mut acc = 0.0;
                for (a, p) in d.iter() {
                    acc += p;
                    if u <= acc {
                        return *a;
                    }
                }
                d[d.len() - 1].0
            }
            _ => {
                if cham_core::obs::is_legal(obs, Action::Call) {
                    Action::Call
                } else {
                    Action::Check
                }
            }
        }
    }

    fn action_probs(
        &self,
        obs: &Observables<'_>,
    ) -> Result<ArrayVec<(Action, f64), 12>, AgentError> {
        let da = self.a.action_probs(obs)?;
        let db = self.b.action_probs(obs)?;
        let wa = self.wa;
        let wb = 1.0 - wa;

        let mut out: ArrayVec<(Action, f64), 12> = ArrayVec::new();
        let mut total = 0.0f64;

        // Union: contributions from `a`, then `b`-only actions.
        for (a, pa) in da.iter() {
            let pb = db
                .iter()
                .find(|(x, _)| x == a)
                .map(|(_, p)| *p)
                .unwrap_or(0.0);
            let p = wa * pa + wb * pb;
            if p > 0.0 {
                out.push((*a, p));
                total += p;
            }
        }
        for (b, pb) in db.iter() {
            if !da.iter().any(|(x, _)| x == b) {
                let p = wb * pb;
                if p > 0.0 {
                    out.push((*b, p));
                    total += p;
                }
            }
        }
        if total > 0.0 {
            for (_, p) in out.iter_mut() {
                *p /= total;
            }
        }
        Ok(out)
    }
}
