//! `ArchetypeAgent` — point + jittered scripted opponents with ANALYTIC
//! `action_probs` (SPECS/03 §3–4).
//!
//! Decision-procedure properties (mandated by review A4):
//! 1. All randomness is **fresh per decision**: `act` draws `u ~ U(0,1)` from
//!    `child(session_seed, "h{hand}.{street}.{dec}.d")` — no per-hand persistence.
//! 2. Mixing probabilities are **analytic functions of (params, obs, chart)** —
//!    `action_probs` returns exactly the distribution `act` samples from, and never
//!    consumes the decision stream (per-decision independence, tested).
//! 3. Postflop equity uses the deterministic strength proxy `strength_now`
//!    (decision D-003: exact off-river equity incl. runouts is ~10^4× too slow).
//! 4. Pinned sizing: preflop open 2.5 bb, 3-bet +3 bb over the raise, 4-bet ×2.2.

use std::cell::Cell;

use arrayvec::ArrayVec;

use cham_core::engine::{Action, Street};
use cham_core::obs::{is_legal, Observables, Player};
use cham_core::rng::{child, next_f64, Rng};

use crate::params::{ArchetypeId, ArchetypeParams};
use crate::percentile::PercentileChart;

/// Sizing fractions for `size_idx` (0 = 33% pot, 1 = 66%, 2 = 100%).
const SIZE_FRACS: [f64; 3] = [0.33, 0.66, 1.0];
/// Pinned preflop sizes (SPECS/03 §3): open 2.5bb; 3bet +3bb; 4bet ×2.2.
pub const OPEN_SIZE_BB: f64 = 2.5;
pub const THREE_BET_ADD_BB: f64 = 3.0;
pub const FOUR_BET_MULT: f64 = 2.2;
/// EHS proxy at which "top-pair-plus" begins (station never folds here).
const TP_EHS: f64 = 0.60;

pub struct ArchetypeAgent {
    arch: ArchetypeId,
    params: ArchetypeParams,
    chart: &'static PercentileChart,
    session_seed: u64,
    hand: Cell<u64>,
    street: Cell<u8>,
    decision: Cell<u32>,
}

impl ArchetypeAgent {
    /// Point (unjittered) archetype.
    pub fn point(arch: ArchetypeId, chart: &'static PercentileChart) -> ArchetypeAgent {
        ArchetypeAgent {
            arch,
            params: ArchetypeParams::point(arch),
            chart,
            session_seed: 0,
            hand: Cell::new(0),
            street: Cell::new(0),
            decision: Cell::new(0),
        }
    }

    /// Jittered archetype: parameters drawn once per session from
    /// `child(session_seed, "jitter")` (the jitter MANIFOLD, not per-hand noise).
    pub fn jittered(arch: ArchetypeId, session_seed: u64, chart: &'static PercentileChart) -> ArchetypeAgent {
        let mut rng = child(session_seed, "jitter");
        let params = crate::params::JitterSpec::standard().apply(&ArchetypeParams::point(arch), &mut rng);
        ArchetypeAgent {
            arch,
            params,
            chart,
            session_seed,
            hand: Cell::new(0),
            street: Cell::new(0),
            decision: Cell::new(0),
        }
    }

    pub fn arch(&self) -> ArchetypeId {
        self.arch
    }

    pub fn params(&self) -> &ArchetypeParams {
        &self.params
    }

    // ---------- sizing helpers (always legal) ----------

    fn size_to(&self, obs: &Observables<'_>, frac: f64, over_current: bool) -> i64 {
        let pot_after_call = obs.pot + obs.to_call;
        let target = if over_current {
            obs.current_bet as f64 + frac * pot_after_call as f64
        } else {
            frac * obs.pot as f64
        };
        let to = target.floor() as i64;
        let max = obs.max_raise_to;
        let min = if over_current { obs.min_raise_to } else { obs.current_bet + 1 };
        // guard: an actor facing nothing may still be nearly all-in (min > max)
        to.clamp(min.min(max), max)
    }

    fn aggressive(&self, obs: &Observables<'_>, frac: f64, over_current: bool) -> Action {
        let to = self.size_to(obs, frac, over_current);
        let a = if obs.to_call > 0 { Action::Raise { to } } else { Action::Bet { to } };
        if is_legal(obs, a) {
            return a;
        }
        // fall back through the legal aggressive slot, then call/check
        for l in &obs.legal {
            if matches!(l.action, Action::Bet { .. } | Action::Raise { .. }) {
                return l.action;
            }
        }
        if is_legal(obs, Action::Call) {
            return Action::Call;
        }
        Action::Check
    }

    fn call_or_fold(&self, obs: &Observables<'_>, ehs: f64) -> Action {
        let odds = if obs.to_call > 0 {
            obs.to_call as f64 / (obs.pot + obs.to_call) as f64
        } else {
            0.0
        };
        let need = (odds * self.params.call_factor).min(1.0);
        let station_tp = self.arch == ArchetypeId::Station && ehs >= TP_EHS;
        if (ehs >= need || station_tp || is_ilrelevant_call(obs)) && is_legal(obs, Action::Call) {
            return Action::Call;
        }
        if is_legal(obs, Action::Fold) {
            return Action::Fold;
        }
        Action::Check
    }

    /// EHS proxy for the current street.
    fn ehs(&self, obs: &Observables<'_>) -> f64 {
        let board: Vec<cham_core::card::Card> = obs.board[..obs.board_len as usize].to_vec();
        cham_core::eval::strength_now(obs.hole, &board)
    }

    fn bet_gate(&self, street: Street) -> f64 {
        match street {
            Street::Flop => self.params.cbet_flop,
            Street::Turn => self.params.barrel_turn,
            Street::River => self.params.barrel_river,
            Street::Preflop => unreachable!("preflop handled separately"),
        }
    }

    /// THE decision core: analytic distribution over legal actions.
    fn dist(&self, obs: &Observables<'_>) -> ArrayVec<(Action, f64), 12> {
        let mut out: ArrayVec<(Action, f64), 12> = ArrayVec::new();
        let pct = self.chart.percentile(obs.hole);
        if obs.street == Street::Preflop {
            let sb_unopened = obs.player == Player::Sb && obs.to_call == self.bb_minus_posted(obs);
            let bb_vs_limp = obs.player == Player::Bb && obs.to_call == 0 && obs.current_bet == 100;
            if sb_unopened {
                // pinned open size 2.5bb
                let to = 250;
                if pct < self.params.open_raise && is_legal(obs, Action::Raise { to }) {
                    out.push((Action::Raise { to }, 1.0));
                    return out;
                }
                if pct < self.params.complete && is_legal(obs, Action::Call) {
                    out.push((Action::Call, 1.0));
                    return out;
                }
                out.push((pick(obs, Action::Fold, Action::Check), 1.0));
                return out;
            }
            if bb_vs_limp {
                // iso-raise to 3bb total vs a limp, else check
                let to = 300;
                if pct < self.params.iso_check && is_legal(obs, Action::Raise { to }) {
                    out.push((Action::Raise { to }, 1.0));
                    return out;
                }
                out.push((Action::Check, 1.0));
                return out;
            }
            // facing a raise: single raise vs 3bet+
            let facing_3bet = obs.current_bet > 200;
            if !facing_3bet {
                let to = (obs.current_bet as f64 + THREE_BET_ADD_BB * 100.0) as i64;
                if pct < self.params.three_bet && is_legal(obs, Action::Raise { to }) {
                    out.push((Action::Raise { to }, 1.0));
                    return out;
                }
                if pct < self.params.call_open && is_legal(obs, Action::Call) {
                    out.push((Action::Call, 1.0));
                    return out;
                }
                out.push((pick(obs, Action::Fold, Action::Check), 1.0));
                return out;
            }
            // facing a 3bet (or 4bet+)
            let to = ((obs.current_bet as f64) * FOUR_BET_MULT) as i64;
            if obs.current_bet <= 600 && pct < self.params.four_bet && is_legal(obs, Action::Raise { to }) {
                out.push((Action::Raise { to }, 1.0));
                return out;
            }
            if pct < self.params.call_3bet && is_legal(obs, Action::Call) {
                out.push((Action::Call, 1.0));
                return out;
            }
            out.push((pick(obs, Action::Fold, Action::Check), 1.0));
            return out;
        }

        // ---- postflop ----
        let ehs = self.ehs(obs);
        if obs.to_call == 0 {
            // checked-to: value / gate / bluff lines
            if obs.street == Street::River {
                // bluff window: p(bluff) = clamp(bluff_window − ehs, 0, 1)
                let bluff_window = (1.0 - self.params.bluff_river + 0.05).max(0.0);
                if ehs < bluff_window {
                    let p = (bluff_window - ehs).min(1.0);
                    let bet = self.aggressive(obs, SIZE_FRACS[self.params.size_idx as usize], false);
                    out.push((bet, p));
                    out.push((Action::Check, 1.0 - p));
                    return out;
                }
            }
            if ehs >= self.params.value_bet && self.params.trap > 0.0 {
                // slow-play mixing (the analytic trap probability)
                let bet = self.aggressive(obs, SIZE_FRACS[self.params.size_idx as usize], false);
                out.push((Action::Check, self.params.trap));
                out.push((bet, 1.0 - self.params.trap));
                return out;
            }
            if ehs >= self.bet_gate(obs.street) {
                let bet = self.aggressive(obs, SIZE_FRACS[self.params.size_idx as usize], false);
                out.push((bet, 1.0));
                return out;
            }
            out.push((Action::Check, 1.0));
            return out;
        }

        // facing a bet: value-raise / call / fold
        if ehs >= self.params.check_raise && is_aggressive_legal(obs) {
            let to = self.size_to(obs, 0.66, true);
            let a = Action::Raise { to };
            if is_legal(obs, a) {
                out.push((a, 1.0));
                return out;
            }
        }
        let call = self.call_or_fold(obs, ehs);
        out.push((call, 1.0));
        out
    }

    fn bb_minus_posted(&self, _obs: &Observables<'_>) -> i64 {
        50 // SB faces exactly one bb minus its posted blind (50 chips at bb=100)
    }
}

fn is_ilrelevant_call(obs: &Observables<'_>) -> bool {
    // all-in for less than a third of pot: pot-odds auto-call
    obs.to_call > 0 && obs.to_call * 3 < obs.pot
}

fn is_aggressive_legal(obs: &Observables<'_>) -> bool {
    obs.legal.iter().any(|l| matches!(l.action, Action::Bet { .. } | Action::Raise { .. }))
}

fn pick(obs: &Observables<'_>, primary: Action, fallback: Action) -> Action {
    if is_legal(obs, primary) {
        return primary;
    }
    fallback
}

impl ArchetypeAgent {
    fn advance_decision(&self, obs: &Observables<'_>) {
        let s = obs.street.as_u8();
        if self.street.get() != s {
            self.street.set(s);
            self.decision.set(0);
        } else {
            self.decision.set(self.decision.get() + 1);
        }
    }
}

impl cham_core::obs::Agent for ArchetypeAgent {
    fn name(&self) -> &str {
        match self.arch {
            ArchetypeId::Nit => "arch:nit",
            ArchetypeId::Tag => "arch:tag",
            ArchetypeId::Lag => "arch:lag",
            ArchetypeId::Station => "arch:station",
        }
    }

    fn act(&mut self, obs: &Observables<'_>, _rng: &mut Rng) -> Action {
        self.advance_decision(obs);
        let dist = self.dist(obs);
        // fresh per-decision draw (SPECS/03 §3.2): independent of everything prior
        let mut u_rng = child(
            self.session_seed ^ 0xA5A5_5A5A,
            &format!("h{}.{}.{}.d", self.hand.get(), obs.street.as_u8(), self.decision.get()),
        );
        let u = next_f64(&mut u_rng);
        let mut acc = 0.0;
        for (a, p) in &dist {
            acc += p;
            if u <= acc {
                return *a;
            }
        }
        dist[dist.len() - 1].0
    }

    fn action_probs(&self, obs: &Observables<'_>) -> Result<ArrayVec<(Action, f64), 12>, cham_core::obs::AgentError> {
        Ok(self.dist(obs))
    }

    fn on_hand_end(&mut self, _ph: &cham_core::engine::PublicHistory, _hero_net: i64) {
        self.hand.set(self.hand.get() + 1);
        self.street.set(0);
        self.decision.set(0);
    }
}
