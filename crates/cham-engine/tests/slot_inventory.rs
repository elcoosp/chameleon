use cham_core::card::Deck;
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::obs::{Observables, Player};
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::ActionSeq;
use cham_engine::ladder::ActionLadder;

const CFG: EngineConfig = EngineConfig { start_stack: 10_000, sb: 50, bb: 100 };

#[test]
fn slot_inventory() {
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);
    let mut counts = std::collections::BTreeMap::new();
    for seed in 0..300u64 {
        let mut st = State::new(CFG, Deck::shuffled(&mut rng_from_seed(seed))).expect("s");
        let mut steps = 0;
        while !st.is_terminal() && steps < 50 {
            steps += 1;
            let p = st.to_act();
            let obs = Observables::view(&st, Player::from_usize(p));
            let seq = ActionSeq::default();
            let slots = ladder.slots(&obs, &seq);
            let normals = slots.iter()
                .filter(|s| matches!(s.action, Action::Bet{..}|Action::Raise{..}) && !s.frac.is_infinite())
                .count();
            let jams = slots.iter()
                .filter(|s| s.frac.is_infinite())
                .count();
            let street = obs.street.as_u8();
            *counts.entry((street, normals, jams)).or_insert(0u64) += 1;
            // advance: call or check to keep the hand moving
            let a = if cham_core::obs::is_legal(&obs, Action::Call) { Action::Call } else { Action::Check };
            if st.apply(a).is_err() { break; }
        }
    }
    eprintln!("\n=== (street, n_normal_aggressive, n_jam) -> count ===");
    for ((s, n, j), c) in &counts {
        let name = ["pre","flop","turn","river"].get(*s as usize).unwrap_or(&"?");
        eprintln!("  {name:>5}: {n} normal, {j} jam  -> {c}");
    }
    assert!(!counts.is_empty());
}
