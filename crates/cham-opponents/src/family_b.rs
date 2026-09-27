//! `FamilyBAgent` (SPECS/03 §5): the second, STRUCTURALLY DIFFERENT script family.
//! Preflop = direct chart-equity lookup table; postflop = a decision list over
//! (ehs bucket × pot-odds bucket). Written independently of the family-A
//! threshold procedure so that family-B evaluation tests learned styles, not
//! implementation tics (anti-circularity, review B1).

use arrayvec::ArrayVec;

use cham_core::engine::Action;
use cham_core::obs::{Observables, is_legal};
use cham_core::rng::Rng;

use crate::params::{ArchetypeId, ArchetypeParams};
use crate::percentile::PercentileChart;

/// Family-B style knob: tightness multiplier on chart-equity gates.
struct Style {
    open_eq: f64,        // min chart equity to open
    defend_eq: f64,      // min chart equity to defend a raise
    iso_eq: f64,         // min chart equity to iso-raise limps
    cbet_list: [f64; 4], // ehs buckets → bet? 1/0 style
    call_slack: f64,     // pot-odds multiplier
}

fn style_for(arch: ArchetypeId) -> Style {
    match arch {
        // calibrated to roughly MATCH family-A frequencies (then diverge in shape)
        ArchetypeId::Nit => Style {
            open_eq: 0.545,
            defend_eq: 0.50,
            iso_eq: 0.53,
            cbet_list: [0.0, 0.0, 1.0, 1.0],
            call_slack: 1.3,
        },
        ArchetypeId::Tag => Style {
            open_eq: 0.495,
            defend_eq: 0.45,
            iso_eq: 0.49,
            cbet_list: [0.0, 1.0, 1.0, 1.0],
            call_slack: 1.1,
        },
        ArchetypeId::Lag => Style {
            open_eq: 0.455,
            defend_eq: 0.40,
            iso_eq: 0.46,
            cbet_list: [1.0, 1.0, 1.0, 1.0],
            call_slack: 0.92,
        },
        ArchetypeId::Station => Style {
            open_eq: 0.470,
            defend_eq: 0.36,
            iso_eq: 0.48,
            cbet_list: [0.0, 1.0, 0.0, 1.0],
            call_slack: 0.62,
        },
    }
}

pub struct FamilyBAgent {
    arch: ArchetypeId,
    #[allow(dead_code)]
    params: ArchetypeParams,
    chart: &'static PercentileChart,
}

impl FamilyBAgent {
    pub fn new(arch: ArchetypeId, chart: &'static PercentileChart) -> FamilyBAgent {
        FamilyBAgent {
            arch,
            params: ArchetypeParams::point(arch),
            chart,
        }
    }

    fn ehs(&self, obs: &Observables<'_>) -> f64 {
        let board: Vec<cham_core::card::Card> = obs.board[..obs.board_len as usize].to_vec();
        cham_core::eval::strength_now(obs.hole, &board)
    }

    /// Decision list: (cond → action) evaluated top-down; first hit wins.
    fn decide(&self, obs: &Observables<'_>) -> Action {
        let st = style_for(self.arch);
        let eq = self.chart.equity(obs.hole);
        if obs.street == cham_core::engine::Street::Preflop {
            let open_to = 250i64;
            if obs.player == cham_core::obs::Player::Sb && obs.to_call == 50 {
                if eq >= st.open_eq && is_legal(obs, Action::Raise { to: open_to }) {
                    return Action::Raise { to: open_to };
                }
                if eq >= st.open_eq - 0.04 && is_legal(obs, Action::Call) {
                    return Action::Call;
                }
                return pick_fold(obs);
            }
            if obs.player == cham_core::obs::Player::Bb
                && obs.to_call == 0
                && obs.current_bet == 100
            {
                if eq >= st.iso_eq && is_legal(obs, Action::Raise { to: 300 }) {
                    return Action::Raise { to: 300 };
                }
                return Action::Check;
            }
            if obs.to_call > 0 {
                if eq >= st.defend_eq + 0.06
                    && is_legal(
                        obs,
                        Action::Raise {
                            to: obs.min_raise_to,
                        },
                    )
                {
                    return Action::Raise {
                        to: obs.min_raise_to,
                    };
                }
                if eq >= st.defend_eq && is_legal(obs, Action::Call) {
                    return Action::Call;
                }
                return pick_fold(obs);
            }
            return Action::Check;
        }
        // postflop decision list
        let ehs = self.ehs(obs);
        let bucket = if ehs < 0.35 {
            0
        } else if ehs < 0.55 {
            1
        } else if ehs < 0.75 {
            2
        } else {
            3
        };
        if obs.to_call == 0 {
            if st.cbet_list[bucket] > 0.0 {
                let frac = match bucket {
                    3 => 0.66,
                    _ => 0.5,
                };
                let to = (frac * obs.pot as f64).floor() as i64;
                // H-12 fix (2026-09-27): when an actor is checked to with
                // < min_raise_to behind, min > max and `i64::clamp` PANICS.
                // Guard exactly as the sibling `archetype.rs::size_to` does.
                let lo = obs.min_raise_to.max(1).min(obs.max_raise_to);
                let to = to.clamp(lo, obs.max_raise_to);
                if is_legal(obs, Action::Bet { to }) {
                    return Action::Bet { to };
                }
            }
            return Action::Check;
        }
        let odds = obs.to_call as f64 / (obs.pot + obs.to_call) as f64;
        if ehs >= odds * st.call_slack && is_legal(obs, Action::Call) {
            return Action::Call;
        }
        if ehs >= 0.85
            && is_legal(
                obs,
                Action::Raise {
                    to: obs.min_raise_to,
                },
            )
        {
            return Action::Raise {
                to: obs.min_raise_to,
            };
        }
        pick_fold(obs)
    }
}

fn pick_fold(obs: &Observables<'_>) -> Action {
    if obs.legal.iter().any(|l| l.action == Action::Fold) {
        Action::Fold
    } else {
        Action::Check
    }
}

impl cham_core::obs::Agent for FamilyBAgent {
    fn name(&self) -> &str {
        match self.arch {
            ArchetypeId::Nit => "famB:nit",
            ArchetypeId::Tag => "famB:tag",
            ArchetypeId::Lag => "famB:lag",
            ArchetypeId::Station => "famB:station",
        }
    }
    fn act(&mut self, obs: &Observables<'_>, _rng: &mut Rng) -> Action {
        self.decide(obs)
    }
    fn action_probs(
        &self,
        obs: &Observables<'_>,
    ) -> Result<ArrayVec<(Action, f64), 12>, cham_core::obs::AgentError> {
        let mut out: ArrayVec<(Action, f64), 12> = ArrayVec::new();
        out.push((self.decide(obs), 1.0));
        Ok(out)
    }
}
