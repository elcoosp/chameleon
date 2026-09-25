//! `SwitcherBot` (SPECS/03 §6): mid-session drift — plays spec A until `switch_at`
//! hands, then spec B (fresh agent instance; state resets at the switch).

use arrayvec::ArrayVec;
use cham_core::engine::Action;
use cham_core::obs::{Agent, Observables};
use cham_core::rng::Rng;

use crate::factory::{build, OpponentSpec};
use crate::percentile::PercentileChart;

pub struct SwitcherBot {
    a: Box<dyn Agent>,
    b: Box<dyn Agent>,
    switch_at: u64,
    hands: u64,
}

impl SwitcherBot {
    pub fn new(a: OpponentSpec, b: OpponentSpec, switch_at: u64, chart: &'static PercentileChart) -> SwitcherBot {
        SwitcherBot {
            a: build(&a, chart),
            b: build(&b, chart),
            switch_at,
            hands: 0,
        }
    }
}

impl Agent for SwitcherBot {
    fn name(&self) -> &str {
        "switcher"
    }
    fn act(&mut self, obs: &Observables<'_>, rng: &mut Rng) -> Action {
        if self.hands < self.switch_at {
            self.a.act(obs, rng)
        } else {
            self.b.act(obs, rng)
        }
    }
    fn action_probs(&self, obs: &Observables<'_>) -> Result<ArrayVec<(Action, f64), 12>, cham_core::obs::AgentError> {
        if self.hands < self.switch_at {
            self.a.action_probs(obs)
        } else {
            self.b.action_probs(obs)
        }
    }
    fn on_hand_end(&mut self, ph: &cham_core::engine::PublicHistory, hero_net: i64) {
        self.hands += 1;
        self.a.on_hand_end(ph, hero_net);
        self.b.on_hand_end(ph, hero_net);
    }
}
