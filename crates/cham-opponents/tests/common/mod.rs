//! Shared mini match driver for opponent tests: plays hands between two agents.

use cham_core::card::Deck;
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::obs::{Agent, Observables, Player};
use cham_core::rng::child;

pub const CFG: EngineConfig = EngineConfig { start_stack: 10_000, sb: 50, bb: 100 };

#[derive(Default)]
pub struct DecisionLog {
    /// (street, to_call_bucket, ehs_bucket, legal_count(u8), action_kind)
    pub entries: Vec<(u8, u8, u8, u8, u8)>,
    pub actions_by_kind: [u64; 5],
    pub n: u64,
}

pub fn action_kind(a: Action) -> usize {
    match a {
        Action::Fold => 0,
        Action::Check => 1,
        Action::Call => 2,
        Action::Bet { .. } => 3,
        Action::Raise { .. } => 4,
    }
}

/// Play `n` hands; hero acts at seat 0, villain at seat 1. Records hero decisions.
pub fn play_hands(hero: &mut dyn Agent, villain: &mut dyn Agent, n: u64, seed: u64, mut log: Option<&mut DecisionLog>) {
    for h in 0..n {
        let rng = &mut child(seed, &format!("h{h}"));
        let deck = Deck::shuffled(rng);
        let mut s = State::new(CFG, deck).expect("state");
        let mut guard = 0;
        while !s.is_terminal() && guard < 400 {
            guard += 1;
            let p = Player::from_usize(s.to_act());
            let obs = Observables::view(&s, p);
            let (hero_turn, agent): (bool, &mut dyn Agent) = if s.to_act() == 0 {
                (true, hero)
            } else {
                (false, villain)
            };
            let a = agent.act(&obs, rng);
            if hero_turn {
                if let Some(l) = log.as_deref_mut() {
                    let board: Vec<cham_core::card::Card> = obs.board[..obs.board_len as usize].to_vec();
                    let ehs = cham_core::eval::strength_now(obs.hole, &board);
                    let ehb = (ehs * 10.0) as u8;
                    let tcb = (obs.to_call / 200) as u8;
                    let kind = action_kind(a);
                    let kind_u8 = kind as u8;
                    l.entries.push((obs.street.as_u8(), tcb, ehb, obs.legal.len() as u8, kind_u8));
                    l.actions_by_kind[kind] += 1;
                    l.n += 1;
                }
            }
            s.apply(a).expect("agents must play legally");
        }
        let ph = cham_core::engine::history::HandHistory {
            seed: h,
            actions: vec![],
            cfg: CFG,
            holes: [s.hole(0), s.hole(1)],
            board: *s.board(),
            board_len: s.board_len(),
            result_sb: s.payoffs()[0],
        };
        let ph = cham_core::engine::history::PublicHistory::from(&ph);
        hero.on_hand_end(&ph, s.payoffs()[0]);
        villain.on_hand_end(&ph, s.payoffs()[1]);
    }
}


