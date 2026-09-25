//! `NoisyAgent` (SPECS/03 §5): human-like mistake wrapper — with probability ε the
//! wrapped agent's action is replaced by a random legal action biased toward
//! callable/passive lines. Realized mistake rate ≈ ε (tested).

use arrayvec::ArrayVec;
use cham_core::engine::Action;
use cham_core::obs::{is_legal, Observables, Agent, AgentError};
use cham_core::rng::Rng;

pub struct NoisyAgent {
    inner: Box<dyn Agent>,
    epsilon: f64,
    mistakes: usize,
    decisions: usize,
}

impl NoisyAgent {
    pub fn new(inner: Box<dyn Agent>, epsilon: f64) -> NoisyAgent {
        NoisyAgent { inner, epsilon: epsilon.clamp(0.0, 1.0), mistakes: 0, decisions: 0 }
    }

    pub fn realized_mistake_rate(&self) -> f64 {
        if self.decisions == 0 {
            0.0
        } else {
            self.mistakes as f64 / self.decisions as f64
        }
    }
}

impl Agent for NoisyAgent {
    fn name(&self) -> &str {
        "noisy"
    }
    fn act(&mut self, obs: &Observables<'_>, rng: &mut Rng) -> Action {
        self.decisions += 1;
        let u = cham_core::rng::next_f64(rng);
        if u < self.epsilon {
            // mistake: random legal action biased toward callable/passive
            let mut pool: Vec<Action> = Vec::new();
            for l in &obs.legal {
                let weight = match l.action {
                    Action::Call | Action::Check => 3,
                    Action::Fold => 1,
                    _ => 1,
                };
                for _ in 0..weight {
                    pool.push(l.action);
                }
            }
            if !pool.is_empty() {
                let i = cham_core::rng::pick(rng, pool.len());
                self.mistakes += 1;
                return pool[i];
            }
        }
        self.inner.act(obs, rng)
    }
    fn action_probs(&self, obs: &Observables<'_>) -> Result<ArrayVec<(Action, f64), 12>, AgentError> {
        // the wrapper's TRAINING view is the inner policy (mistakes are execution
        // noise, not intended strategy — matches the spec's read of human noise)
        self.inner.action_probs(obs)
    }
    fn on_hand_end(&mut self, ph: &cham_core::engine::PublicHistory, hero_net: i64) {
        self.inner.on_hand_end(ph, hero_net);
    }
}

/// Convenience: is `a` legal (re-export shim used by tests).
pub fn legal(obs: &Observables<'_>, a: Action) -> bool {
    is_legal(obs, a)
}
