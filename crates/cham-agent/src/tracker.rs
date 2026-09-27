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
        Tracker {
            ewm: [0.5; 13],
            ..Tracker::default()
        }
    }

    /// Exponential-smoothing decay factor λ such that a value's weight halves
    /// every `HALF_LIFE` hands: λ = 0.5^(1/HALF_LIFE) ≈ 0.98851.
    ///
    /// H-1 fix (2026-09-27): the previous form was `(0.5f64).ln() / HALF_LIFE`
    /// = ln(0.5)/60 ≈ -0.0116 — the LOG of the intended λ, negative, so every
    /// EWM stat (VPIP, PFR, 3bet, cbet, WTSD, aggression, ...) went negative
    /// or overshot past 1. The 13-dimension feature vector fed into the
    /// router was therefore out of range on every observation. See
    /// docs/plans/chameleon-bug-report.md H-1.
    fn lam() -> f64 {
        (0.5f64).powf(1.0 / HALF_LIFE)
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
        // H-3 / H-4 / H-5 fixes (2026-09-27):
        //
        //  (a) A real showdown has NO fold action anywhere. The previous
        //      code inferred "showdown" from `showdown_holes` being fully
        //      populated, but PublicHistory::from currently reveals both
        //      holes whenever board_len == 5 — including a FOLD on the
        //      river (H-5, I9 leak). Detect the fold directly.
        //  (b) FOLD_VS_BET needs a PER-HAND `opp_called_bet_this_hand`
        //      flag, not the LIFETIME `n_bets_faced_called > 0` counter,
        //      which becomes permanently true after the first call ever
        //      and then never records a fold again (H-4).
        //  (c) Every postflop stat must be gated on its per-hand
        //      OPPORTUNITY, not written 0/1 every hand (H-5). Otherwise
        //      the stats are diluted and `n_faces_open` becomes its own
        //      denominator (invariant 1.0).
        //  (d) Aggression must count bets/raises on EVERY street including
        //      the river (previously omitted, H-5).
        let opp_is_pfa = pfr || opp_3bet;
        let opp_called_bet_this_hand = ph.actions.iter().any(|(s, p, a)| {
            s.as_u8() >= 1 && p.as_usize() == opp && matches!(a, cham_core::engine::Action::Call)
        });
        let opp_folded_preflop = ph.actions.iter().any(|(s, p, a)| {
            s.as_u8() == 0 && p.as_usize() == opp && matches!(a, cham_core::engine::Action::Fold)
        });
        let reached_flop = ph.actions.iter().any(|(s, _, _)| s.as_u8() >= 1);
        let reached_turn = ph.actions.iter().any(|(s, _, _)| s.as_u8() >= 2);
        let any_fold = ph
            .actions
            .iter()
            .any(|(_, _, a)| matches!(a, cham_core::engine::Action::Fold));
        let reached_showdown = !any_fold && ph.showdown_holes.iter().all(|h| h.is_some());
        let opp_aggressive = opp_3bet
            || opp_cbet
            || opp_barreled_turn
            || ph.actions.iter().any(|(s, p, a)| {
                p.as_usize() == opp
                    && s.as_u8() >= 1
                    && matches!(
                        a,
                        cham_core::engine::Action::Bet { .. }
                            | cham_core::engine::Action::Raise { .. }
                    )
            });

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
        // H-5: `opp_cbet_opportunities` must count OPPORTUNITIES (opp was
        // PFA and reached the flop), not c-bets made — the previous code
        // made it identical to `n_cbet` (invariant 1.0 ratio).
        if opp_is_pfa && reached_flop {
            self.opp_cbet_opportunities += 1;
            if opp_cbet {
                self.n_cbet += 1;
            }
        }
        if opp_bet_faced {
            self.opp_bets_faced += 1;
        }

        // --- EWM updates (raw, [0,1]) — now all OPPORTUNITY-GATED ---
        self.ewm_update(
            EWM_VPIP,
            if is_opp_vpip(&ph.actions, opp) {
                1.0
            } else {
                0.0
            },
        );
        self.ewm_update(EWM_PFR, if pfr { 1.0 } else { 0.0 });
        if facing_open {
            // opp had an open to face → opportunity to 3-bet
            self.ewm_update(EWM_THREE_BET, if opp_3bet { 1.0 } else { 0.0 });
        }
        if facing_3bet {
            // H-3 fix: opp faces OUR 3bet. Previous code fired when
            // `opp_faces_open > 0 && opp_3bet` — the WRONG side (fires
            // when opp 3bets us), on a LIFETIME counter, and always wrote
            // 0.0 (a fold was never recorded). Now: 1.0 if opp folds
            // preflop, 0.0 if opp calls/raises.
            self.ewm_update(EWM_FOLD_TO_3BET, if opp_folded_preflop { 1.0 } else { 0.0 });
            self.ewm_update(
                EWM_CALL_3BET,
                if opp_called_3bet(&ph.actions, opp) {
                    1.0
                } else {
                    0.0
                },
            );
        }
        if opp_is_pfa && reached_flop {
            self.ewm_update(EWM_CBET_FLOP, if opp_cbet { 1.0 } else { 0.0 });
        }
        if opp_bet_faced && street >= 1 {
            // H-4 fix: per-hand flag, not the lifetime counter.
            self.ewm_update(
                EWM_FOLD_VS_BET,
                if opp_called_bet_this_hand { 0.0 } else { 1.0 },
            );
        }
        if opp_cbet && reached_turn {
            self.ewm_update(EWM_BARREL_TURN, if opp_barreled_turn { 1.0 } else { 0.0 });
        }
        // WTSD + showdown (H-5: require a real showdown — no fold anywhere)
        self.hands_since_showdown += 1;
        if reached_showdown {
            opp_showdown = true;
            self.hands_since_showdown = 0;
        }
        self.ewm_update(EWM_WTSD, if opp_showdown { 1.0 } else { 0.0 });
        let opp_won = ph.nets[1 - hero_seat] > 0;
        self.ewm_update(
            EWM_SHOWDOWN_WON,
            if opp_showdown && opp_won { 1.0 } else { 0.0 },
        );
        self.ewm_update(EWM_AGGRESSION, if opp_aggressive { 1.0 } else { 0.0 });
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
        let var = self
            .net_window
            .iter()
            .map(|&x| (x as f64 - mean).powi(2))
            .sum::<f64>()
            / n as f64;
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

fn is_opp_vpip(
    actions: &[(
        cham_core::engine::Street,
        cham_core::obs::Player,
        cham_core::engine::Action,
    )],
    opp: usize,
) -> bool {
    actions
        .iter()
        .filter(|(s, _, _)| s.as_u8() == 0)
        .any(|(_, p, a)| {
            p.as_usize() == opp
                && matches!(
                    a,
                    cham_core::engine::Action::Call
                        | cham_core::engine::Action::Bet { .. }
                        | cham_core::engine::Action::Raise { .. }
                )
        })
}

fn is_opp_call_path(
    actions: &[(
        cham_core::engine::Street,
        cham_core::obs::Player,
        cham_core::engine::Action,
    )],
    opp: usize,
) -> bool {
    is_opp_vpip(actions, opp)
}

fn opp_called_3bet(
    actions: &[(
        cham_core::engine::Street,
        cham_core::obs::Player,
        cham_core::engine::Action,
    )],
    opp: usize,
) -> bool {
    actions
        .iter()
        .filter(|(s, _, _)| s.as_u8() == 0)
        .any(|(_, p, a)| p.as_usize() == opp && matches!(a, cham_core::engine::Action::Call))
}
