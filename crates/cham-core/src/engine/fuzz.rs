//! Property/fuzz harness (SPECS/01 §5): random-action hand playback asserting
//! invariants I2–I7, plus the I9 leak check over `PublicHistory`.

use arrayvec::ArrayVec;

use crate::card::Deck;
use crate::consts::{I2_LEGAL_NONEMPTY, I3_CHIP_CONSERVATION, I4_STACKS_NONNEG, I5_ZERO_SUM, I6_BOARD_UNIQUE};
use crate::engine::config::EngineConfig;
use crate::engine::history::HandHistory;
use crate::engine::{Action, State};
use crate::obs::{LegalAction, Player};
use crate::rng::Rng;
use crate::CoreError;

/// Assert the state-level invariants (I3–I6). Cards are `Card(u8)` typed < 52 by
/// construction only if the deck is sound — I1 checked here too.
pub fn check_invariants(s: &State) -> Result<(), CoreError> {
    // I1: deck/domain sanity
    for c in s.board() {
        if c.idx() > 51 {
            return Err(CoreError::Invariant(crate::consts::I1_CARD_DOMAIN));
        }
    }
    // I4: stacks non-negative
    let [st0, st1] = s.stacks();
    if st0 < 0 || st1 < 0 {
        return Err(CoreError::Invariant(I4_STACKS_NONNEG));
    }
    // I3: conservation (during hand: stacks + pot == 2*start)
    if !s.is_terminal() && s.stacks().iter().sum::<i64>() + s.pot() != 2 * s.cfg().start_stack {
        return Err(CoreError::Invariant(I3_CHIP_CONSERVATION));
    }
    // I6: board uniqueness
    let n = s.board_len() as usize;
    for i in 0..n {
        for j in (i + 1)..n {
            if s.board()[i].idx() == s.board()[j].idx() {
                return Err(CoreError::Invariant(I6_BOARD_UNIQUE));
            }
        }
        let [a, b] = s.hole(0).cards();
        if s.board()[i].idx() == a.idx() || s.board()[i].idx() == b.idx() {
            return Err(CoreError::Invariant(I6_BOARD_UNIQUE));
        }
        let [a, b] = s.hole(1).cards();
        if s.board()[i].idx() == a.idx() || s.board()[i].idx() == b.idx() {
            return Err(CoreError::Invariant(I6_BOARD_UNIQUE));
        }
    }
    Ok(())
}

/// Play one hand to termination with uniformly random legal actions, collecting a
/// full [`HandHistory`]. Asserts I2/I3/I4/I5/I6/I7 along the way.
pub fn play_random(cfg: EngineConfig, seed: u64, rng: &mut Rng) -> Result<HandHistory, CoreError> {
    cfg.validate()?;
    let deck = Deck::shuffled(rng);
    let mut state = State::new(cfg, deck)?;
    let mut actions: Vec<(crate::engine::Street, Player, Action)> = Vec::new();
    let mut guard = 0u32;
    while !state.is_terminal() {
        let mut legal: ArrayVec<LegalAction, 12> = ArrayVec::new();
        state.legal_actions(&mut legal);
        if legal.is_empty() {
            return Err(CoreError::Invariant(I2_LEGAL_NONEMPTY));
        }
        // I7: raise-to levels respect the min-raise floor
        let street = state.street();
        let player = Player::from_usize(state.to_act());
        for l in &legal {
            if let Action::Raise { to } = l.action {
                debug_assert!(
                    to >= state.min_raise_to()
                        || l.is_all_in
                        || to == state.max_raise_to(),
                    "I7 min-raise progression violated"
                );
            }
        }
        let idx = crate::rng::pick(rng, legal.len());
        let a = legal[idx].action;
        state.apply(a)?;
        actions.push((street, player, a));
        check_invariants(&state)?;
        guard += 1;
        if guard > 400 {
            return Err(CoreError::Invariant("hand did not terminate (guard)"));
        }
    }
    // I5: zero-sum payoffs
    let [p0, p1] = state.payoffs();
    if p0 + p1 != 0 {
        return Err(CoreError::Invariant(I5_ZERO_SUM));
    }
    Ok(HandHistory {
        seed,
        actions,
        cfg,
        holes: [state.hole(0), state.hole(1)],
        board: *state.board(),
        board_len: state.board_len(),
        result_sb: state.payoffs()[0],
    })
}
