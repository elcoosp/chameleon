//! Observables and the Agent trait (SPECS/01 §6): borrowed views with NO leak
//! surface. `Observables::view(state, p)` is the only constructor; it copies every
//! field an agent may see and drops everything else (villain holes are not merely
//! private — they are absent from the struct).

use arrayvec::ArrayVec;

use crate::CoreError;
use crate::card::{Card, Hand2};
use crate::engine::{Action, State, Street};
use crate::rng::Rng;

/// Seat in a heads-up hand: SB (player 0) or BB (player 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum Player {
    Sb = 0,
    Bb = 1,
}

impl Player {
    pub fn from_usize(p: usize) -> Player {
        if p == 0 { Player::Sb } else { Player::Bb }
    }
    pub fn as_usize(self) -> usize {
        self as usize
    }
    pub fn other(self) -> Player {
        match self {
            Player::Sb => Player::Bb,
            Player::Bb => Player::Sb,
        }
    }
}

/// One legal action as offered by the engine. All-in is flagged, never a variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegalAction {
    pub action: Action,
    pub is_all_in: bool,
}

/// Errors from the default `action_probs` and agent plumbing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AgentError {
    /// The agent does not implement a probability oracle (baselines, humans).
    #[error("NotProbabilistic")]
    NotProbabilistic,
}

/// Borrowed view over a State for one seat — no ownership, no hidden info, no Vec.
/// The lifetime pins the view to its source state; the villain's hole cards are
/// structurally absent (there is no back-reference to the State at all).
#[derive(Clone, Debug)]
pub struct Observables<'a> {
    pub player: Player,
    pub street: Street,
    pub hole: Hand2,
    /// Board cards dealt so far (the first `board_len` are meaningful).
    pub board: [Card; 5],
    pub board_len: u8,
    pub pot: i64,
    pub to_call: i64,
    pub current_bet: i64,
    pub min_raise_to: i64,
    pub max_raise_to: i64,
    /// Size of the last full raise this street (for rule-based legality).
    pub last_full_raise: i64,
    /// This player's remaining stack.
    pub stack: i64,
    /// Effective stack vs the opponent's remaining chips.
    pub effective_stack: i64,
    pub stacks: [i64; 2],
    /// Engine-legal actions in the pinned canonical order (cap 12).
    pub legal: ArrayVec<LegalAction, 12>,
    _anchor: std::marker::PhantomData<&'a State>,
}

impl<'a> Observables<'a> {
    /// The only constructor: project a state down to one seat's view.
    /// Clone these observables with a different hole-card pair. Board,
    /// street, player, pot, stacks, and legal actions are preserved. Used
    /// by the PCS trainer to derive a per-combo encoder key at a single
    /// node without rebuilding the engine `State` per combo: the bucket
    /// depends only on `(hole, board, street)` and every other key field
    /// is public, so swapping the hole is sufficient.
    pub fn with_hole(&self, hole: Hand2) -> Observables<'a> {
        Observables {
            player: self.player,
            street: self.street,
            hole,
            board: self.board,
            board_len: self.board_len,
            pot: self.pot,
            to_call: self.to_call,
            current_bet: self.current_bet,
            min_raise_to: self.min_raise_to,
            max_raise_to: self.max_raise_to,
            last_full_raise: self.last_full_raise,
            stack: self.stack,
            effective_stack: self.effective_stack,
            stacks: self.stacks,
            legal: self.legal.clone(),
            _anchor: std::marker::PhantomData,
        }
    }

    pub fn view(state: &'a State, p: Player) -> Observables<'a> {
        let i = p.as_usize();
        let mut legal: ArrayVec<LegalAction, 12> = ArrayVec::new();
        state.legal_actions(&mut legal);
        let to_call = (state.current_bet as i64 - state.street_bet[i] as i64).max(0);
        let o = 1 - i;
        Observables {
            player: p,
            street: state.street(),
            hole: state.hole(i),
            board: *state.board(),
            board_len: state.board_len(),
            pot: state.pot(),
            to_call,
            current_bet: state.current_bet(),
            min_raise_to: state.min_raise_to(),
            max_raise_to: state.max_raise_to(),
            last_full_raise: state.last_full_raise(),
            stack: state.stacks()[i],
            effective_stack: state.stacks()[i].min(state.stacks()[o]),
            stacks: state.stacks(),
            legal,
            _anchor: std::marker::PhantomData,
        }
    }

    /// Pot in bb.
    pub fn pot_bb(&self) -> f64 {
        self.pot as f64 / 100.0
    }
    /// Stack-to-pot ratio (this seat).
    pub fn spr(&self) -> f64 {
        if self.pot == 0 {
            0.0
        } else {
            self.stack as f64 / self.pot as f64
        }
    }
    /// Pot odds offered by the current `to_call`: call / (pot + call).
    pub fn pot_odds(&self) -> f64 {
        if self.to_call == 0 {
            0.0
        } else {
            self.to_call as f64 / (self.pot + self.to_call) as f64
        }
    }
    /// Effective stack in bb.
    pub fn effective_stack_bb(&self) -> f64 {
        self.effective_stack as f64 / 100.0
    }
}

/// The agent trait every player policy implements (SPECS/01 §6).
pub trait Agent: Send {
    fn name(&self) -> &str;
    /// Choose one legal action. Must return a member of `obs.legal`.
    fn act(&mut self, obs: &Observables<'_>, rng: &mut Rng) -> Action;

    /// State-aware variant (2026-10-06): some policies (the live river
    /// search, via the safe-resolve gadget) need the live `State` to
    /// forward-simulate the subgame and build the blueprint prior. Default:
    /// ignore the state and call `act`, so every existing agent is unchanged.
    fn act_with_state(
        &mut self,
        obs: &Observables<'_>,
        rng: &mut Rng,
        _state: Option<&crate::engine::State>,
    ) -> Action {
        self.act(obs, rng)
    }
    /// Probability oracle for TRAINING (opponent reach). Analytic by construction
    /// (SPECS/03 §4): returns (action, p) pairs covering the agent's full intended
    /// distribution at this decision. Default: Err(NotProbabilistic) — only
    /// archetype scripts (and CallBot) implement it.
    fn action_probs(
        &self,
        _obs: &Observables<'_>,
    ) -> Result<ArrayVec<(Action, f64), 12>, AgentError> {
        Err(AgentError::NotProbabilistic)
    }
    /// Public information only (v2 leak fix, invariant I9). Default no-op.
    fn on_hand_end(&mut self, _ph: &crate::engine::PublicHistory, _hero_net: i64) {}
    /// Public action feed: the match driver calls this for EVERY action (with the
    /// PRE-action observables) so agents that need the canonical action sequence
    /// (infoset keys) can maintain it. Default no-op.
    fn on_public_action(&mut self, _obs: &Observables<'_>, _player: Player, _action: Action) {}
    /// Downcast hook for stateful opponents (v3 §6, M6): the trainer syncs the
    /// current-path action history into sequence-aware opponents before each
    /// `action_probs` query. Default `None` (stateless agents ignore it).
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }
}

/// Rule-based legality over the observable geometry (mirrors the engine's
/// `is_legal`): any `to` between the min-raise level and the stack cap is legal —
/// the canonical legal list only shows the extremes.
pub fn is_legal(obs: &Observables<'_>, a: Action) -> bool {
    let facing = obs.to_call;
    let opp = obs.stacks[1 - obs.player.as_usize()];
    let opp_all_in = opp == 0;
    let max_to = obs.max_raise_to;
    let min_level = obs.current_bet + obs.last_full_raise;
    match a {
        Action::Fold => facing > 0,
        Action::Check => facing == 0,
        Action::Call => facing > 0 && obs.stack > 0,
        Action::Bet { to } => {
            facing == 0
                && obs.stack > 0
                && !opp_all_in
                && to > obs.current_bet
                && to <= max_to
                && (to >= min_level || to == max_to)
        }
        Action::Raise { to } => {
            facing > 0
                && obs.stack > 0
                && !opp_all_in
                && to > obs.current_bet
                && to <= max_to
                && (to >= min_level || to == max_to)
        }
    }
}

/// Defensive helper used by wrappers (NoisyAgent, RandomBot): clamp an action to a
/// legal one. Returns a legal action unconditionally.
pub fn coerce_legal(obs: &Observables<'_>, a: Action) -> Result<Action, CoreError> {
    if is_legal(obs, a) {
        return Ok(a);
    }
    obs.legal
        .first()
        .map(|l| l.action)
        .ok_or(CoreError::Invariant(crate::consts::I2_LEGAL_NONEMPTY))
}
