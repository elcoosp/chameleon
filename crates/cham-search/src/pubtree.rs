//! Public betting tree (2026-10-08, plan W1 T1.2/T1.6).
//!
//! The card-INDEPENDENT abstract game tree: nodes carry the acting player
//! and the abstract action set (ladder slots), never hole cards or board.
//! This is the shared skeleton for the full-game VBR and the PCS trainer;
//! per-combo strategies are attached at solve time.
//!
//! Built by driving a fresh engine `State` and branching on the ladder's
//! abstract slots at each node (the same slots training keys on). The
//! structure is card-independent by construction — verified by a test that
//! builds it under different decks and asserts equal (player, actions).

use arrayvec::ArrayVec;
use cham_core::card::Deck;
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::obs::{Observables, Player};
use cham_engine::encoder::ActionSeq;
use cham_engine::ladder::ActionLadder;

pub const TERMINAL: u8 = 255;

#[derive(Clone, Debug)]
pub struct PublicNode {
    /// Acting player (0/1), or `TERMINAL`.
    pub player: u8,
    /// Abstract actions at this node (empty at terminals).
    pub actions: ArrayVec<Action, 12>,
    /// Child node indices, parallel to `actions`.
    pub children: ArrayVec<u32, 12>,
    /// Terminal iff `player == TERMINAL`.
    pub terminal: bool,
}

pub struct PublicTree {
    pub nodes: Vec<PublicNode>,
    pub root: u32,
}

impl PublicTree {
    /// Build the abstract tree from the default ordered deck. `cap_nodes`
    /// bounds memory (the real tree is large; the caller chooses a cap).
    pub fn build(cfg: EngineConfig, ladder: &ActionLadder, cap_nodes: usize) -> PublicTree {
        Self::build_with_deck(cfg, ladder, cap_nodes, Deck::ordered())
    }

    /// Build the abstract tree using a caller-provided deck. The tree
    /// structure is card-independent by construction; this entry point
    /// exists so callers (and tests) can vary the deck and confirm that.
    pub fn build_with_deck(
        cfg: EngineConfig,
        ladder: &ActionLadder,
        cap_nodes: usize,
        deck: Deck,
    ) -> PublicTree {
        let mut tree = PublicTree { nodes: Vec::new(), root: 0 };
        let mut st = State::new(cfg, deck).expect("fresh state");
        let mut seq = ActionSeq::default();
        tree.root = tree.build_node(&mut st, ladder, &mut seq, cap_nodes);
        tree
    }

    fn build_node(
        &mut self,
        st: &mut State,
        ladder: &ActionLadder,
        seq: &mut ActionSeq,
        cap_nodes: usize,
    ) -> u32 {
        let id = self.nodes.len() as u32;
        if st.is_terminal() || self.nodes.len() >= cap_nodes {
            self.nodes.push(PublicNode {
                player: TERMINAL,
                actions: ArrayVec::new(),
                children: ArrayVec::new(),
                terminal: true,
            });
            return id;
        }
        self.nodes.push(PublicNode {
            player: TERMINAL,
            actions: ArrayVec::new(),
            children: ArrayVec::new(),
            terminal: true,
        });
        let p = st.to_act();
        let obs = Observables::view(st, Player::from_usize(p));
        let slots = ladder.slots(&obs, seq);
        let mut actions: ArrayVec<Action, 12> = ArrayVec::new();
        for s in slots.iter() {
            if cham_core::obs::is_legal(&obs, s.action) {
                actions.push(s.action);
            }
        }
        if actions.is_empty() {
            return id;
        }
        let mut children: ArrayVec<u32, 12> = ArrayVec::new();
        for a in actions.iter() {
            let mut st2 = *st;
            let mut seq2 = *seq;
            let obs_p = Observables::view(&st2, Player::from_usize(p));
            cham_engine::ladder::record_action(ladder, &obs_p, Player::from_usize(p), *a, &mut seq2);
            if st2.apply(*a).is_err() {
                continue;
            }
            let c = self.build_node(&mut st2, ladder, &mut seq2, cap_nodes);
            children.push(c);
        }
        self.nodes[id as usize] = PublicNode {
            player: p as u8,
            actions,
            children,
            terminal: false,
        };
        id
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
