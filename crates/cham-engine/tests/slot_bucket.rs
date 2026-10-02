//! F6c (2026-10-02): the `CHAM_SLOT_BUCKET` gate and the class-aware
//! `nearest_slot`. Both are keying-relevant; the shipped bundle was
//! built without them, so the gate default must be off and the
//! class-awareness must not change on-tree behavior.

use cham_core::card::Deck;
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::obs::{Observables, Player};
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::ActionSeq;
use cham_engine::ladder::ActionLadder;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

fn flop_state() -> State {
    let mut st = State::new(CFG, Deck::shuffled(&mut rng_from_seed(7))).expect("state");
    st.apply(Action::Call).expect("call");
    st.apply(Action::Check).expect("check");
    st
}

/// On-tree action: `nearest_slot` returns the exact position, unchanged
/// from the pre-class-aware behavior.
#[test]
fn nearest_slot_exact_for_on_tree_action() {
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);
    let st = flop_state();
    let obs = Observables::view(&st, Player::Sb);
    let seq = ActionSeq::default();
    let slots = ladder.slots(&obs, &seq);
    for (i, s) in slots.iter().enumerate() {
        let got = ladder.nearest_slot(&obs, &seq, s.action);
        assert_eq!(got, i, "on-tree action {i} not exact");
    }
}

/// Off-tree Raise must not match a Bet slot (class-blind regression
/// guard).
#[test]
fn nearest_slot_is_class_aware_for_off_tree_raise() {
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);
    let st = flop_state();
    let obs = Observables::view(&st, Player::Sb);
    let seq = ActionSeq::default();
    let slots = ladder.slots(&obs, &seq);

    let raise = Action::Raise {
        to: obs.current_bet + 75,
    };
    let idx = ladder.nearest_slot(&obs, &seq, raise);
    if let Some(s) = slots.get(idx) {
        if matches!(s.action, Action::Bet { .. }) {
            panic!("class-blind: a Raise request matched a Bet slot");
        }
    }
}

/// The gate must default off.
#[test]
fn gate_default_is_off() {
    let v = std::env::var("CHAM_SLOT_BUCKET").ok();
    assert!(
        v.as_deref() != Some("1"),
        "CHAM_SLOT_BUCKET=1 leaked into the test process"
    );
}
