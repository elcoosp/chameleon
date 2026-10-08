//! Card-independence of the PublicTree (plan W1 T1.2 / T1.6).
//!
//! The shared skeleton for the full-game VBR and the PCS trainer must
//! not depend on hole cards or board: it carries only (player, abstract
//! actions). This test builds the same tree under an ordered deck and a
//! shuffled deck and asserts the (player, actions) skeleton is identical.

use cham_core::card::Deck;
use cham_core::engine::config::EngineConfig;
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::ladder::ActionLadder;
use cham_search::pubtree::PublicTree;

const CFG: EngineConfig = EngineConfig { start_stack: 10_000, sb: 50, bb: 100 };
const CAP: usize = 100_000;

fn skeleton(t: &PublicTree) -> Vec<(u8, String)> {
    t.nodes
        .iter()
        .map(|n| (n.player, format!("{:?}", n.actions)))
        .collect()
}

#[test]
fn pubtree_card_independent() {
    let ladder = ActionLadder::new(&AbstractionConfig::tiny());

    let a = PublicTree::build_with_deck(CFG, &ladder, CAP, Deck::ordered());
    let b = PublicTree::build_with_deck(
        CFG,
        &ladder,
        CAP,
        Deck::shuffled(&mut rng_from_seed(0xDEAD_BEEF)),
    );

    assert!(
        a.len() > 1,
        "PublicTree collapsed to a single node (degenerate build)"
    );
    assert!(
        a.nodes.iter().any(|n| n.terminal),
        "PublicTree has no terminal node (degenerate build)"
    );

    assert_eq!(a.len(), b.len(), "tree sizes differ between decks");
    assert_eq!(
        skeleton(&a),
        skeleton(&b),
        "PublicTree (player, actions) skeleton depends on the deck"
    );
}
