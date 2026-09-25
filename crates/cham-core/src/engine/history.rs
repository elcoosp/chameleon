//! Hand histories (SPECS/01 §5) — THE LEAK FIX (review A5).
//!
//! - [`HandHistory`]: FULL information — engine internals, eval bookkeeping,
//!   duplicate matching, replay tooling. NEVER passed to an Agent.
//! - [`PublicHistory`]: what an agent may see after a hand — actions, showdown-
//!   revealed cards ONLY, net results. No seed, no replay, no folded holes.

use serde::{Deserialize, Serialize};

use crate::card::{Card, Deck, Hand2};
use crate::engine::config::EngineConfig;
use crate::engine::{Action, State, Street};
use crate::obs::Player;
use crate::CoreError;

/// FULL information. For engine internals, eval bookkeeping, duplicate matching,
/// replay tooling. NEVER passed to an Agent (type-level separation: agents consume
/// `&PublicHistory` only, SPECS/01 §6).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HandHistory {
    pub seed: u64,
    pub actions: Vec<(Street, Player, Action)>,
    pub cfg: EngineConfig,
    pub holes: [Hand2; 2],
    pub board: [Card; 5],
    /// Number of board cards actually dealt (0..=5). Additive deviation D-004:
    /// `Card(0)` is a legal card (2♠), so a bare `[Card; 5]` cannot self-report
    /// how far the board ran out on folded hands.
    pub board_len: u8,
    /// Net result for the SB seat (player 0), chips.
    pub result_sb: i64,
}

impl HandHistory {
    /// Reconstruct the terminal state from the record (deterministic).
    pub fn replay(&self) -> Result<State, CoreError> {
        let [h0a, h0b] = self.holes[0].cards();
        let [h1a, h1b] = self.holes[1].cards();
        let n = self.board_len as usize;
        let mut prefix = Vec::with_capacity(4 + n);
        prefix.extend_from_slice(&[h0a, h1a, h0b, h1b]);
        prefix.extend_from_slice(&self.board[..n]);
        let mut state = State::new(self.cfg, Deck::with_prefix(&prefix))?;
        for (street, player, action) in &self.actions {
            if state.is_terminal() {
                return Err(CoreError::Replay("actions continue past terminal".into()));
            }
            if state.street() != *street {
                return Err(CoreError::Replay(format!("street mismatch: record {street:?}")));
            }
            let expected = if player.as_usize() == state.to_act() {
                Ok(())
            } else {
                Err(CoreError::Replay(format!("actor mismatch: record {player:?}")))
            };
            expected?;
            state.apply(*action)?;
        }
        if !state.is_terminal() {
            return Err(CoreError::Replay("recorded hand is not terminal".into()));
        }
        let [r0, _] = state.payoffs();
        if r0 != self.result_sb {
            return Err(CoreError::Replay("result mismatch on replay".into()));
        }
        Ok(state)
    }
    /// FNV-1a over the record (dedupe/replay bookkeeping; NOT tamper evidence).
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |b: u8| {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        };
        mix(self.seed as u8);
        mix((self.seed >> 8) as u8);
        for (s, p, a) in &self.actions {
            mix(s.as_u8());
            mix(p.as_usize() as u8);
            match a {
                Action::Fold => mix(0),
                Action::Check => mix(1),
                Action::Call => mix(2),
                Action::Bet { to } | Action::Raise { to } => {
                    mix(3);
                    for b in to.to_le_bytes() {
                        mix(b);
                    }
                }
            }
        }
        mix(self.result_sb as u8);
        h
    }
}

/// What an agent may see after a hand: actions, showdown-revealed cards ONLY
/// (both holes iff showdown reached; folded holes remain hidden), net results.
/// No seed, no `replay()`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublicHistory {
    pub actions: Vec<(Street, Player, Action)>,
    pub board: [Card; 5],
    pub showdown_holes: [Option<Hand2>; 2],
    pub nets: [i64; 2],
}

impl PublicHistory {
    /// The ONLY constructor — projects the full record down to public information.
    pub fn from(hh: &HandHistory) -> PublicHistory {
        let showdown = hh.board_len == 5;
        PublicHistory {
            actions: hh.actions.clone(),
            board: hh.board,
            showdown_holes: if showdown {
                [Some(hh.holes[0]), Some(hh.holes[1])]
            } else {
                [None, None]
            },
            // zero-sum: player-1 net is the negative of player-0 net
            nets: [hh.result_sb, -hh.result_sb],
        }
    }
}
