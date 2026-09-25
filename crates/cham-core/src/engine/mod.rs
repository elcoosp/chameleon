//! The HUNL rules engine (SPECS/01 §5): `State` is `Copy` (≤ 128 bytes, fixed
//! arrays only), hot paths allocate nothing. Actions are canonical
//! (`Fold/Check/Call/Bet{to}/Raise{to}` — all-in is `Bet/Raise{to: stack_cap}`,
//! never a distinct variant, SPECS/00 §4).
//!
//! Positional truth: HU preflop SB acts first; postflop BB acts first, every street.
//! Min-raise: full raise = `last_full_raise_size`; an all-in below min-raise neither
//! reopens action nor resets the increment. Uncalled bets are returned; split odd
//! chip goes to the BB. The deck is consumed front-to-back (holes then board, no
//! burns — statistically irrelevant under a uniform shuffle and documented).

pub mod config;
pub mod fuzz;
pub mod history;

use arrayvec::ArrayVec;

pub use config::EngineConfig;
pub use history::{HandHistory, PublicHistory};

use crate::card::{Card, Deck, Hand2};
use crate::obs::LegalAction;
use crate::CoreError;

/// Betting street.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum Street {
    #[default]
    Preflop = 0,
    Flop = 1,
    Turn = 2,
    River = 3,
}

impl Street {
    pub fn from_u8(v: u8) -> Street {
        match v {
            0 => Street::Preflop,
            1 => Street::Flop,
            2 => Street::Turn,
            _ => Street::River,
        }
    }
    pub fn as_u8(self) -> u8 {
        self as u8
    }
    pub fn next(self) -> Option<Street> {
        match self {
            Street::Preflop => Some(Street::Flop),
            Street::Flop => Some(Street::Turn),
            Street::Turn => Some(Street::River),
            Street::River => None,
        }
    }
}

/// Canonical action (SPECS/00 §4). `to` = this street's total bet level AFTER the
/// action. `AllIn` is NOT a variant: it is `Bet/Raise { to: stack_cap }` with
/// `LegalAction::is_all_in = true`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Action {
    Fold,
    Check,
    Call,
    Bet { to: i64 },
    Raise { to: i64 },
}

impl Action {
    /// Short stable encoding used in traces ("f", "k", "c", "b<to>", "r<to>").
    pub fn to_str(self) -> String {
        match self {
            Action::Fold => "f".into(),
            Action::Check => "k".into(),
            Action::Call => "c".into(),
            Action::Bet { to } => format!("b{to}"),
            Action::Raise { to } => format!("r{to}"),
        }
    }
    /// Parse the short encoding (inverse of [`Action::to_str`]).
    pub fn parse(s: &str) -> Option<Action> {
        match s {
            "f" => return Some(Action::Fold),
            "k" => return Some(Action::Check),
            "c" => return Some(Action::Call),
            _ => {}
        }
        let (kind, amt) = s.split_at(1);
        let to: i64 = amt.parse().ok()?;
        match kind {
            "b" => Some(Action::Bet { to }),
            "r" => Some(Action::Raise { to }),
            _ => None,
        }
    }
}

/// Result of one `apply`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApplyOutcome {
    pub street_dealt: bool,
    pub hand_over: bool,
    pub all_in_runout: bool,
}

/// Full engine state. `Copy`, ≤ 128 bytes, no heap anywhere in its methods.
#[derive(Clone, Copy, Debug)]
pub struct State {
    pub(crate) deck: [Card; 52],
    pub(crate) deck_pos: u8,
    pub(crate) holes: [Hand2; 2],
    pub(crate) board: [Card; 5],
    pub(crate) board_len: u8,
    pub(crate) street: u8,
    pub(crate) to_act: u8,
    pub(crate) acted: u8,
    pub(crate) committed: [i32; 2],
    pub(crate) street_bet: [i32; 2],
    pub(crate) current_bet: i32,
    pub(crate) min_raise_to: i32,
    pub(crate) last_full_raise: i32,
    pub(crate) stacks: [i32; 2],
    pub(crate) sb: i32,
    pub(crate) bb: i32,
    pub(crate) start_stack: i32,
    pub(crate) hand_over: bool,
    pub(crate) showdown: bool,
    pub(crate) ended_by_runout: bool,
    pub(crate) result: [i32; 2],
}

impl State {
    /// New hand under `cfg` with the given (shuffled) deck. Deals holes (p0, p1,
    /// p0, p1) and posts blinds; SB (player 0) acts first preflop.
    pub fn new(cfg: EngineConfig, mut deck: Deck) -> Result<State, CoreError> {
        cfg.validate()?;
        let h0a = deck.deal()?;
        let h1a = deck.deal()?;
        let h0b = deck.deal()?;
        let h1b = deck.deal()?;
        let start = cfg.start_stack as i32;
        let (sb, bb) = (cfg.sb as i32, cfg.bb as i32);
        // blinds capped by stack (defense in depth; cfg validation keeps 20bb+)
        let sb_post = sb.min(start);
        let bb_post = bb.min(start);
        let mut s = State {
            deck: {
                let mut d = [Card(0); 52];
                d.copy_from_slice(&deck.cards);
                d
            },
            deck_pos: 4,
            holes: [Hand2::new(h0a, h0b), Hand2::new(h1a, h1b)],
            board: [Card(0); 5],
            board_len: 0,
            street: 0,
            to_act: 0,
            acted: 0,
            committed: [sb_post, bb_post],
            street_bet: [sb_post, bb_post],
            current_bet: bb_post.max(sb_post),
            min_raise_to: bb,
            last_full_raise: bb,
            stacks: [start - sb_post, start - bb_post],
            sb,
            bb,
            start_stack: start,
            hand_over: false,
            showdown: false,
            ended_by_runout: false,
            result: [0; 2],
        };
        if s.stacks[0] == 0 && s.stacks[1] == 0 {
            s.all_in_runout_terminal();
        }
        Ok(s)
    }

    // ---------- accessors (i64 public API per SPECS/00 §4) ----------

    pub fn street(&self) -> Street {
        Street::from_u8(self.street)
    }
    pub fn to_act(&self) -> usize {
        self.to_act as usize
    }
    pub fn stacks(&self) -> [i64; 2] {
        [self.stacks[0] as i64, self.stacks[1] as i64]
    }
    pub fn pot(&self) -> i64 {
        (self.committed[0] + self.committed[1]) as i64
    }
    pub fn current_bet(&self) -> i64 {
        self.current_bet as i64
    }
    pub fn min_raise_to(&self) -> i64 {
        self.min_raise_to as i64
    }
    pub fn max_raise_to(&self) -> i64 {
        (self.street_bet[self.to_act as usize] + self.stacks[self.to_act as usize]) as i64
    }
    pub fn last_full_raise(&self) -> i64 {
        self.last_full_raise as i64
    }
    pub fn board(&self) -> &[Card; 5] {
        &self.board
    }
    pub fn board_len(&self) -> u8 {
        self.board_len
    }
    pub fn hole(&self, p: usize) -> Hand2 {
        self.holes[p]
    }
    pub fn is_terminal(&self) -> bool {
        self.hand_over
    }
    /// True when the hand ended because a call put a player all-in and the board ran
    /// out (used by the eval all-in-EV variance reduction; SPECS/08 §4).
    pub fn is_all_in_runout(&self) -> bool {
        self.ended_by_runout
    }
    pub fn payoffs(&self) -> [i64; 2] {
        if !self.hand_over {
            return [0, 0];
        }
        [self.result[0] as i64, self.result[1] as i64]
    }
    pub fn cfg(&self) -> EngineConfig {
        EngineConfig {
            start_stack: self.start_stack as i64,
            sb: self.sb as i64,
            bb: self.bb as i64,
        }
    }
    pub fn reached_showdown(&self) -> bool {
        self.hand_over && self.showdown
    }

    // ---------- legality ----------

    /// Legal actions into a caller-provided ArrayVec (NO allocation; cap 12).
    /// Pinned canonical order:
    /// - facing no bet: `[Check, Bet(min), Bet(all-in)]`
    /// - facing a bet:  `[Fold, Call, Raise(min-raise), Raise(all-in)]`
    ///
    /// When the opponent is all-in, betting/raising is moot: `[Check]` / `[Fold, Call]`.
    pub fn legal_actions(&self, out: &mut ArrayVec<LegalAction, 12>) {
        out.clear();
        if self.hand_over {
            return;
        }
        let p = self.to_act as usize;
        let o = 1 - p;
        let facing = self.current_bet - self.street_bet[p];
        let opp_all_in = self.stacks[o] == 0;
        if facing == 0 {
            out.push(LegalAction { action: Action::Check, is_all_in: false });
            if self.stacks[p] > 0 && !opp_all_in {
                let max_to = self.street_bet[p] + self.stacks[p];
                let min_to = (self.current_bet + self.last_full_raise).min(max_to); // preflop BB: 200; postflop: bb; stack < min ⇒ all-in for less
                out.push(LegalAction {
                    action: Action::Bet { to: min_to as i64 },
                    is_all_in: min_to == max_to,
                });
                if max_to > min_to {
                    out.push(LegalAction {
                        action: Action::Bet { to: max_to as i64 },
                        is_all_in: true,
                    });
                }
            }
        } else {
            out.push(LegalAction { action: Action::Fold, is_all_in: false });
            if self.stacks[p] > 0 {
                out.push(LegalAction {
                    action: Action::Call,
                    is_all_in: facing >= self.stacks[p],
                });
                if !opp_all_in {
                    let max_to = self.street_bet[p] + self.stacks[p];
                    if max_to > self.current_bet {
                        let full_to = self.current_bet + self.last_full_raise;
                        let min_to = full_to.min(max_to); // all-in below min-raise allowed
                        out.push(LegalAction {
                            action: Action::Raise { to: min_to as i64 },
                            is_all_in: min_to == max_to,
                        });
                        if max_to > min_to {
                            out.push(LegalAction {
                                action: Action::Raise { to: max_to as i64 },
                                is_all_in: true,
                            });
                        }
                    }
                }
            }
        }
    }

    /// Rule-based legality (any `to` between the min-raise level and the stack cap
    /// is legal — the canonical slot list above is only the LADDER's view).
    fn is_legal(&self, a: Action) -> bool {
        if self.hand_over {
            return false;
        }
        let p = self.to_act as usize;
        let o = 1 - p;
        let facing = self.current_bet - self.street_bet[p];
        let opp_all_in = self.stacks[o] == 0;
        let max_to = self.street_bet[p] + self.stacks[p];
        match a {
            Action::Fold => facing > 0,
            Action::Check => facing == 0,
            Action::Call => facing > 0 && self.stacks[p] > 0,
            Action::Bet { to } => {
                facing == 0
                    && self.stacks[p] > 0
                    && !opp_all_in
                    && to > self.street_bet[p] as i64
                    && to <= max_to as i64
                    && (to >= (self.current_bet + self.last_full_raise) as i64 || to == max_to as i64)
            }
            Action::Raise { to } => {
                facing > 0
                    && self.stacks[p] > 0
                    && !opp_all_in
                    && to > self.current_bet as i64
                    && to <= max_to as i64
                    && (to >= (self.current_bet + self.last_full_raise) as i64
                        || to == max_to as i64)
            }
        }
    }

    // ---------- apply ----------

    /// Apply one action. Validates against the legal set; allocation-free.
    pub fn apply(&mut self, a: Action) -> Result<ApplyOutcome, CoreError> {
        if self.hand_over {
            return Err(CoreError::IllegalAction { action: a, reason: "hand already over".into() });
        }
        if !self.is_legal(a) {
            return Err(CoreError::IllegalAction { action: a, reason: "not in legal set".into() });
        }
        let p = self.to_act as usize;
        let o = 1 - p;
        let mut out = ApplyOutcome { street_dealt: false, hand_over: false, all_in_runout: false };

        match a {
            Action::Fold => {
                // uncalled portion of the opponent's bet returns to the opponent
                let returned = (self.street_bet[o] - self.street_bet[p]).max(0);
                self.stacks[o] += returned;
                self.committed[o] -= returned;
                let pot = self.committed[0] + self.committed[1];
                self.result[o] = pot - self.committed[o];
                self.result[p] = -self.committed[p];
                self.hand_over = true;
                self.showdown = false;
            }
            Action::Check => {
                self.acted |= 1 << p;
            }
            Action::Call => {
                let to_call = (self.current_bet - self.street_bet[p]).min(self.stacks[p]);
                self.street_bet[p] += to_call;
                self.stacks[p] -= to_call;
                self.committed[p] += to_call;
                self.acted |= 1 << p;
            }
            Action::Bet { to } | Action::Raise { to } => {
                let to = to as i32;
                let add = to - self.street_bet[p];
                let prev_current = self.current_bet;
                self.street_bet[p] = to;
                self.stacks[p] -= add;
                self.committed[p] += add;
                self.current_bet = to;
                // full-raise tracking: all-in below min-raise does not reset increment
                let increment = to - prev_current;
                if increment >= self.last_full_raise {
                    self.last_full_raise = increment;
                }
                self.min_raise_to = to + self.last_full_raise;
                self.acted = 1 << p; // opponent must respond
            }
        }

        if !self.hand_over {
            self.to_act = o as u8;
        }

        // short all-in CALL: p matched as far as possible, levels still unequal.
        // (All-in BETS/RAISES must NOT take this path — the opponent still acts.)
        if matches!(a, Action::Call)
            && !self.hand_over
            && self.stacks[p] == 0
            && self.street_bet[p] != self.street_bet[o]
        {
            let returned = (self.street_bet[o] - self.street_bet[p]).max(0);
            self.stacks[o] += returned;
            self.committed[o] -= returned;
            if self.street == Street::River as u8 {
                self.showdown_terminal();
            } else {
                self.all_in_runout_terminal();
                out.all_in_runout = true;
            }
        }

        // street completion: both acted and levels matched
        if !self.hand_over
            && self.acted == 0b11
            && self.street_bet[0] == self.street_bet[1]
        {
            if self.street == Street::River as u8 {
                self.showdown_terminal();
                out.hand_over = true;
            } else if self.stacks[0] == 0 || self.stacks[1] == 0 {
                self.all_in_runout_terminal();
                out.all_in_runout = true;
            } else {
                self.advance_street();
                out.street_dealt = true;
            }
        }
        out.hand_over = self.hand_over;
        Ok(out)
    }

    fn advance_street(&mut self) {
        let next = Street::from_u8(self.street).next().expect("not river here");
        let n = match next {
            Street::Flop => 3,
            Street::Turn | Street::River => 1,
            Street::Preflop => unreachable!("cham-core: invariant I6"),
        };
        for _ in 0..n {
            let c = self.deck[self.deck_pos as usize];
            self.deck_pos += 1;
            self.board[self.board_len as usize] = c;
            self.board_len += 1;
        }
        self.street = next as u8;
        self.street_bet = [0, 0];
        self.current_bet = 0;
        self.min_raise_to = self.bb;
        self.last_full_raise = self.bb;
        self.acted = 0;
        self.to_act = 1; // postflop BB acts first, every street
    }

    fn showdown_terminal(&mut self) {
        let mut c7 = [Card(0); 7];
        c7[2..].copy_from_slice(&self.board[..self.board_len as usize]);
        let mut r = [0u16; 2];
        for p in 0..2 {
            let [a, b] = self.holes[p].cards();
            c7[0] = a;
            c7[1] = b;
            r[p] = crate::eval::evaluate7(&c7);
        }
        let pot = self.committed[0] + self.committed[1];
        if r[0] > r[1] {
            self.result = [pot - self.committed[0], -self.committed[1]];
        } else if r[1] > r[0] {
            self.result = [-self.committed[0], pot - self.committed[1]];
        } else {
            // split: odd chip to BB (player 1)
            let half0 = pot / 2;
            let half1 = pot - half0;
            self.result = [half0 - self.committed[0], half1 - self.committed[1]];
        }
        self.hand_over = true;
        self.showdown = true;
    }

    fn all_in_runout_terminal(&mut self) {
        while self.board_len < 5 {
            let c = self.deck[self.deck_pos as usize];
            self.deck_pos += 1;
            self.board[self.board_len as usize] = c;
            self.board_len += 1;
        }
        self.ended_by_runout = true;
        self.showdown_terminal();
    }
}
