//! `PerturbedNashAgent` (SPECS/03 §5): a robust-blueprint policy tilted toward a
//! documented leak (Ganzfried–Sandholm safety perturbations).
//!
//! The strategy source is injected as a boxed closure by cham-eval (DAG: this
//! crate depends on cham-core only — the blueprint artifact never appears here).

use arrayvec::ArrayVec;

use cham_core::engine::Action;
use cham_core::obs::Observables;

/// Tilt direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Tilt {
    OverFold,
    OverCall,
    OverRaise,
}

impl Tilt {
    pub fn parse(s: &str) -> Option<Tilt> {
        match s {
            "overfold" => Some(Tilt::OverFold),
            "overcall" => Some(Tilt::OverCall),
            "overraise" => Some(Tilt::OverRaise),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Tilt::OverFold => "overfold",
            Tilt::OverCall => "overcall",
            Tilt::OverRaise => "overraise",
        }
    }
}

/// A strategy oracle over the REAL action space (the blueprint mixture).
pub type StrategySource = Box<dyn Fn(&Observables<'_>) -> Vec<(Action, f64)> + Send>;

pub struct PerturbedNashAgent {
    tilt: Tilt,
    delta: f64,
    source: Option<StrategySource>,
}

impl PerturbedNashAgent {
    pub fn new(tilt: Tilt, delta: f64) -> PerturbedNashAgent {
        PerturbedNashAgent {
            tilt,
            delta: delta.clamp(0.0, 0.9),
            source: None,
        }
    }

    /// Inject the blueprint-backed strategy (cham-eval does this at load time).
    pub fn set_source(&mut self, src: StrategySource) {
        self.source = Some(src);
    }

    /// Tilted distribution: shift `delta` mass toward the leak direction and
    /// renormalize (Ganzfried–Sandholm-style safety perturbation, review B1).
    pub fn tilted(&self, obs: &Observables<'_>) -> ArrayVec<(Action, f64), 12> {
        let base: Vec<(Action, f64)> = match &self.source {
            Some(src) => src(obs),
            None => uniform_source(obs),
        };
        let mut out: ArrayVec<(Action, f64), 12> = ArrayVec::new();
        // The tilt moves EXACTLY min(δ, 1 − target_mass) of probability mass into
        // the leak direction (Δ = δ whenever feasible — the test pins this), then
        // the remaining mass rescales to keep the total at 1.
        let classify = |a: &Action| -> bool {
            match self.tilt {
                Tilt::OverFold => matches!(a, Action::Fold),
                Tilt::OverCall => matches!(a, Action::Call | Action::Check),
                Tilt::OverRaise => matches!(a, Action::Bet { .. } | Action::Raise { .. }),
            }
        };
        let target_mass: f64 = base
            .iter()
            .filter(|(a, _)| classify(a))
            .map(|(_, p)| *p)
            .sum();
        let taken = (self.delta).min(1.0 - target_mass);
        let new_mass = target_mass + taken;
        for (a, p) in &base {
            let q = if classify(a) {
                if target_mass > 1e-12 {
                    p * (new_mass / target_mass)
                } else {
                    new_mass / base.len() as f64
                }
            } else if target_mass < 1.0 - 1e-12 {
                p * ((1.0 - new_mass) / (1.0 - target_mass))
            } else {
                *p
            };
            out.push((*a, q));
        }
        out.retain(|(_, p)| *p > 1e-9);
        out
    }
}

fn uniform_source(obs: &Observables<'_>) -> Vec<(Action, f64)> {
    let n = obs.legal.len() as f64;
    obs.legal.iter().map(|l| (l.action, 1.0 / n)).collect()
}

impl cham_core::obs::Agent for PerturbedNashAgent {
    fn name(&self) -> &str {
        match self.tilt {
            Tilt::OverFold => "pnash:overfold",
            Tilt::OverCall => "pnash:overcall",
            Tilt::OverRaise => "pnash:overraise",
        }
    }
    fn act(&mut self, obs: &Observables<'_>, rng: &mut cham_core::rng::Rng) -> Action {
        let dist = self.tilted(obs);
        sample(&dist, rng)
    }
    fn action_probs(
        &self,
        obs: &Observables<'_>,
    ) -> Result<ArrayVec<(Action, f64), 12>, cham_core::obs::AgentError> {
        Ok(self.tilted(obs))
    }
}

/// Sample an action from a normalized distribution.
pub fn sample(dist: &[(Action, f64)], rng: &mut cham_core::rng::Rng) -> Action {
    let u = cham_core::rng::next_f64(rng);
    let mut acc = 0.0;
    for (a, p) in dist {
        acc += p;
        if u <= acc {
            return *a;
        }
    }
    dist.last().map(|(a, _)| *a).unwrap_or(Action::Check)
}
