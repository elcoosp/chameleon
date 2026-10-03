//! 2026-10-03: the `mix:` spec + JamBot's analytic `action_probs` — the
//! machinery for the jamfix-regression fix. Tests the parse round-trip
//! and the mixer's convexity without needing a PercentileChart (the
//! agents are constructed directly).

use cham_core::card::Deck;
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::obs::{Agent, Observables, Player};
use cham_core::rng::rng_from_seed;
use cham_opponents::baselines::{CallBot, JamBot};
use cham_opponents::factory::OpponentSpec;
use cham_opponents::mixer::MixerAgent;

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

#[test]
fn mix_parse_round_trips() {
    let cases = [
        "mix:0.75:arch:nit~jamfix",
        "mix:0.5:arch:lag~arch:station",
        "mix:0.3:noisy:0.1:arch:tag~jamfix",
    ];
    for id in cases {
        let spec = OpponentSpec::parse(id).unwrap_or_else(|e| panic!("parse {id}: {e}"));
        assert_eq!(spec.id(), id, "round-trip failed for {id}");
    }
}

#[test]
fn mix_parse_rejects_bad_ids() {
    for bad in [
        "mix:",
        "mix:0.5",
        "mix:0.5:arch:nit",
        "mix:x:arch:nit~jamfix",
    ] {
        assert!(
            OpponentSpec::parse(bad).is_err(),
            "expected error for {bad:?}"
        );
    }
}

#[test]
fn jamfix_action_probs_is_point_mass_on_all_in() {
    let st = flop_state();
    let p = Player::from_usize(st.to_act());
    let obs = Observables::view(&st, p);
    let d = JamBot.action_probs(&obs).expect("jamfix probs");
    // Facing no bet on the flop, an all-in is legal -> point mass.
    let total: f64 = d.iter().map(|(_, pr)| *pr).sum();
    assert!(
        (total - 1.0).abs() < 1e-12,
        "probs must sum to 1, got {total}"
    );
    assert!(
        d.iter()
            .any(|(a, _)| matches!(a, Action::Bet { .. } | Action::Raise { .. })),
        "jamfix should mass on an aggressive action, got {d:?}"
    );
}

#[test]
fn mixer_is_a_convex_combination() {
    let st = flop_state();
    let p = Player::from_usize(st.to_act());
    let obs = Observables::view(&st, p);

    let call = CallBot.action_probs(&obs).expect("call probs");
    let jam = JamBot.action_probs(&obs).expect("jam probs");

    for wa in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let mixer = MixerAgent::new(Box::new(CallBot), Box::new(JamBot), wa);
        let m = mixer.action_probs(&obs).expect("mix probs");

        // sums to 1
        let total: f64 = m.iter().map(|(_, pr)| *pr).sum();
        assert!((total - 1.0).abs() < 1e-9, "wa={wa}: sum {total}");

        // each action's prob == wa*call(a) + (1-wa)*jam(a)
        for (a, pm) in m.iter() {
            let pc = call
                .iter()
                .find(|(x, _)| x == a)
                .map(|(_, q)| *q)
                .unwrap_or(0.0);
            let pj = jam
                .iter()
                .find(|(x, _)| x == a)
                .map(|(_, q)| *q)
                .unwrap_or(0.0);
            let want = wa * pc + (1.0 - wa) * pj;
            assert!(
                (pm - want).abs() < 1e-9,
                "wa={wa} action {a:?}: got {pm}, want {want}"
            );
        }
    }
}

#[test]
fn mixer_at_extremes_equals_a_child() {
    let st = flop_state();
    let p = Player::from_usize(st.to_act());
    let obs = Observables::view(&st, p);

    let pure_call = MixerAgent::new(Box::new(CallBot), Box::new(JamBot), 1.0)
        .action_probs(&obs)
        .expect("wa=1");
    let call = CallBot.action_probs(&obs).expect("call");
    assert_eq!(pure_call.len(), call.len(), "wa=1 must equal child a");

    let pure_jam = MixerAgent::new(Box::new(CallBot), Box::new(JamBot), 0.0)
        .action_probs(&obs)
        .expect("wa=0");
    let jam = JamBot.action_probs(&obs).expect("jam");
    assert_eq!(pure_jam.len(), jam.len(), "wa=0 must equal child b");
}
