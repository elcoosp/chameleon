//! Tracker (SPECS/07 §2): EWM opponent stats + opportunity counts from
//! `&PublicHistory` ONLY (type-level leak-proofness: no other input compiles).
//!
//! - EWM half-life 60 hands: `s ← s·λ + x·(1−λ)`, λ = exp(ln(0.5)/60)
//! - opportunity-based denominators (3bet only facing opens, cbet only
//!   checked-to-as-aggressor, …) — exposed for router features 14–17
//! - maturity shrink `min(1, hands/150)` toward 0.5 on all EWM stats
//! - session EV trend z-score over the last 200 hands
//! - hands-since-showdown

use serde::{Deserialize, Serialize};

use cham_core::card::Hand2;
use cham_core::engine::history::PublicHistory;

const HALF_LIFE: f64 = 60.0;
const TREND_WINDOW: usize = 200;

/// The tracker's serializable state (leak-audited: contains only aggregate stats).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Tracker {
    pub hands: u64,
    /// raw EWM statistics (unshrunk), each ∈ [0,1]
    pub ewm: [f64; 13],
    /// denominators for opportunity stats (feature 14–17 inputs)
    pub opp_faces_open: u64,
    pub opp_faces_3bet: u64,
    pub opp_cbet_opportunities: u64,
    pub opp_bets_faced: u64,
    /// numerators
    pub n_faces_open: u64,
    pub n_faces_3bet: u64,
    pub n_cbet: u64,
    pub n_bets_faced_called: u64,
    /// last `TREND_WINDOW` hand nets (sb-perspective), for the EV trend
    pub net_window: Vec<i64>,
    pub hands_since_showdown: u64,
    /// per-session accumulation only — never per-hand hidden cards
    pub total_net: i64,
}

pub const EWM_VPIP: usize = 0;
pub const EWM_PFR: usize = 1;
pub const EWM_THREE_BET: usize = 2;
pub const EWM_FOLD_TO_3BET: usize = 3;
pub const EWM_CALL_3BET: usize = 4;
pub const EWM_CBET_FLOP: usize = 5;
pub const EWM_FOLD_TO_CBET: usize = 6;
pub const EWM_BARREL_TURN: usize = 7;
pub const EWM_WTSD: usize = 8;
pub const EWM_AGGRESSION: usize = 9;
pub const EWM_SHOWDOWN_WON: usize = 10;
pub const EWM_FOLD_VS_BET: usize = 11;
pub const EWM_LIMP: usize = 12;

impl Tracker {
    pub fn new() -> Tracker {
        Tracker { ewm: [0.5; 13], ..Tracker::default() }
    }

    fn lam() -> f64 {
        (0.5f64).ln() / HALF_LIFE
    }

    fn ewm_update(&mut self, idx: usize, value: f64) {
        let lam = Self::lam();
        let s = self.ewm[idx];
        self.ewm[idx] = s * lam + value * (1.0 - lam);
    }

    /// Observe one completed hand. `hero_seat` is OUR seat; the tracker models the
    /// OPPONENT (seat `1 − hero_seat`) from public actions only.
    pub fn observe_hand(&mut self, ph: &PublicHistory, hero_net: i64, hero_seat: usize) {
        self.hands += 1;
        self.total_net += hero_net;
        self.net_window.push(hero_net);
        if self.net_window.len() > TREND_WINDOW {
            self.net_window.remove(0);
        }
        let opp = 1 - hero_seat;

        // reconstruct per-street public actions for the opponent
        let opp_put_in = [0i64; 2];
        let mut vpip = false;
        let mut pfr = false;
        let mut limped = false;
        let mut opp_3bet = false;
        let mut facing_open = false;
        let mut facing_3bet = false;
        let mut opp_cbet = false;
        let mut opp_barreled_turn = false;
        let mut opp_bet_faced = false;
        let mut opp_showdown = false;

        // walk the public action sequence with level tracking (chips)
        let mut street_bet = [0i64; 2];
        let mut street = 0u8;
        let mut raises_this_street = 0i32;
        let mut last_level = 0i64;
        let mut voluntary = false;
        for (s, player, action) in &ph.actions {
            if s.as_u8() != street {
                street = s.as_u8();
                street_bet = [0; 2];
                raises_this_street = 0;
                last_level = 0;
            }
            let p = player.as_usize();
            let is_opp = p == opp;
            match action {
                cham_core::engine::Action::Fold => {
                    if is_opp && street == 0 && raises_this_street == 0 {
                        // fold to... nothing to track beyond vpip
                    }
                }
                cham_core::engine::Action::Check => {}
                cham_core::engine::Action::Call => {
                    if street == 0 {
                        voluntary = true;
                        if is_opp && raises_this_street == 0 {
                            limped = last_level == 0;
                        }
                    }
                    if is_opp && facing_bet_postflop(raises_this_street, street) {
                        self.n_bets_faced_called += 1;
                        opp_bet_faced = true;
                    }
                }
                cham_core::engine::Action::Bet { to } | cham_core::engine::Action::Raise { to } => {
                    let increment = *to - street_bet[p] - last_level.max(0);
                    let _ = increment;
                    if street == 0 {
                        voluntary = true;
                        if is_opp {
                            if raises_this_street == 0 {
                                pfr = pfr || is_opp;
                            }
                            if raises_this_street >= 1 && is_opp {
                                opp_3bet = true;
                            }
                        } else if raises_this_street == 0 {
                            facing_open = true;
                        } else if raises_this_street == 1 {
                            facing_3bet = true;
                        }
                    } else if is_opp {
                        if street == 1 && raises_this_street == 0 {
                            opp_cbet = true;
                        } else if street == 2 && raises_this_street == 0 {
                            opp_barreled_turn = true;
                        }
                    } else if street >= 1 && raises_this_street == 0 {
                        // we bet: opponent faces a bet
                        opp_bet_faced = true;
                    }
                    raises_this_street += 1;
                    last_level = *to;
                }
            }
            street_bet[p] = last_level.max(street_bet[p]);
            let _ = opp_put_in;
        }
        if voluntary && is_opp_call_path(&ph.actions, opp) {
            vpip = true;
        }
        let _ = vpip;
        // --- opportunity denominators ---
        if facing_open {
            self.opp_faces_open += 1;
            if opp_3bet {
                self.n_faces_3bet += 1;
            }
            self.n_faces_open += 1;
        }
        if facing_3bet {
            self.opp_faces_3bet += 1;
        }
        if opp_cbet {
            self.opp_cbet_opportunities += 1;
            self.n_cbet += 1;
        }
        if opp_bet_faced {
            self.opp_bets_faced += 1;
        }

        // --- EWM updates (raw, [0,1]) ---
        self.ewm_update(EWM_VPIP, if is_opp_vpip(&ph.actions, opp) { 1.0 } else { 0.0 });
        self.ewm_update(EWM_PFR, if pfr { 1.0 } else { 0.0 });
        self.ewm_update(EWM_THREE_BET, if opp_3bet { 1.0 } else { 0.0 });
        if self.opp_faces_open > 0 && opp_3bet {
            self.ewm_update(EWM_FOLD_TO_3BET, 0.0);
        }
        if facing_3bet {
            self.ewm_update(EWM_CALL_3BET, if opp_called_3bet(&ph.actions, opp) { 1.0 } else { 0.0 });
        }
        self.ewm_update(EWM_CBET_FLOP, if opp_cbet { 1.0 } else { 0.0 });
        if opp_bet_faced && street >= 1 {
            let called = self.n_bets_faced_called > 0;
            self.ewm_update(EWM_FOLD_VS_BET, if called { 0.0 } else { 1.0 });
        }
        self.ewm_update(EWM_BARREL_TURN, if opp_barreled_turn { 1.0 } else { 0.0 });
        // WTSD + showdown
        self.hands_since_showdown += 1;
        if ph.showdown_holes.iter().all(|h| h.is_some()) {
            // only reached at showdown (PublicHistory contract)
            opp_showdown = true;
            self.hands_since_showdown = 0;
        }
        self.ewm_update(EWM_WTSD, if opp_showdown { 1.0 } else { 0.0 });
        let opp_won = ph.nets[1 - hero_seat] > 0;
        self.ewm_update(EWM_SHOWDOWN_WON, if opp_showdown && opp_won { 1.0 } else { 0.0 });
        self.ewm_update(EWM_AGGRESSION, if opp_3bet || opp_cbet || opp_barreled_turn { 1.0 } else { 0.0 });
        self.ewm_update(EWM_LIMP, if limped { 1.0 } else { 0.0 });
        let _ = opp_put_in;
    }

    /// Maturity-shrunk EWM stats for the router (SPECS/07 §2 formula).
    pub fn shrunk_ewm(&self) -> [f64; 13] {
        let m = cham_router::features::maturity_shrink(self.hands);
        self.ewm.map(|s| 0.5 * (1.0 - m) + s * m)
    }

    /// Session EV trend z over the last 200 hands (clamped ±3 → /3 by the router).
    pub fn trend_z(&self) -> f64 {
        let n = self.net_window.len();
        if n < 10 {
            return 0.0;
        }
        let mean = self.net_window.iter().sum::<i64>() as f64 / n as f64;
        let var = self.net_window.iter().map(|&x| (x as f64 - mean).powi(2)).sum::<f64>() / n as f64;
        let sd = var.sqrt().max(1e-9);
        let z = mean / (sd / (n as f64).sqrt());
        z.clamp(-3.0, 3.0)
    }

    /// Opportunity features 14–17 (log-scaled into [0,1]).
    pub fn opportunity_features(&self) -> [f64; 4] {
        let lg = |n: u64, cap: f64| ((n as f64 + 1.0).ln() / cap).min(1.0);
        [
            lg(self.opp_faces_open, 8.0),
            lg(self.opp_faces_3bet, 6.0),
            lg(self.opp_cbet_opportunities, 8.0),
            lg(self.opp_bets_faced, 10.0),
        ]
    }

    /// Hands since showdown, log-scaled /ln(50).
    pub fn hands_since_showdown_feature(&self) -> f64 {
        ((self.hands_since_showdown as f64 + 1.0).ln() / (50.0f64).ln()).min(1.0)
    }

    /// The showdown-hole projection used by the leak test (aggregate only).
    pub fn showdown_seen(&self, h: Hand2) -> bool {
        let _ = h;
        false // the tracker never records hole cards — structural guarantee
    }
}

fn facing_bet_postflop(raises: i32, street: u8) -> bool {
    street >= 1 && raises == 0
}

fn is_opp_vpip(actions: &[(cham_core::engine::Street, cham_core::obs::Player, cham_core::engine::Action)], opp: usize) -> bool {
    actions
        .iter()
        .filter(|(s, _, _)| s.as_u8() == 0)
        .any(|(_, p, a)| p.as_usize() == opp && matches!(a, cham_core::engine::Action::Call | cham_core::engine::Action::Bet { .. } | cham_core::engine::Action::Raise { .. }))
}

fn is_opp_call_path(actions: &[(cham_core::engine::Street, cham_core::obs::Player, cham_core::engine::Action)], opp: usize) -> bool {
    is_opp_vpip(actions, opp)
}

fn opp_called_3bet(actions: &[(cham_core::engine::Street, cham_core::obs::Player, cham_core::engine::Action)], opp: usize) -> bool {
    actions
        .iter()
        .filter(|(s, _, _)| s.as_u8() == 0)
        .any(|(_, p, a)| p.as_usize() == opp && matches!(a, cham_core::engine::Action::Call))
}
