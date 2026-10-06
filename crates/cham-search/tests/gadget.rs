//! Safe-resolving gadget structure (2026-10-06).
//!
//! Burch/Brown-Sandholm: the opponent gets a root opt-out worth their
//! blueprint CFV. This test checks the TREE is built as specified:
//!   - without the gadget: root is hero's decision;
//!   - with the gadget: root is villain's [terminate | play], and the
//!     terminate terminal's value is -v_bp[villain_class] (hero-relative).

use cham_search::subgame::{Class, Subgame, TerminalKind};

fn classes() -> (Vec<Class>, Vec<Class>) {
    let hero = vec![
        Class {
            weight: 0.5,
            strength: 0.8,
        },
        Class {
            weight: 0.5,
            strength: 0.2,
        },
    ];
    let villain = vec![
        Class {
            weight: 0.6,
            strength: 0.7,
        },
        Class {
            weight: 0.4,
            strength: 0.3,
        },
    ];
    (hero, villain)
}

#[test]
fn no_gadget_root_is_hero() {
    let (h, v) = classes();
    let sg = Subgame::build(h, v, 20.0, 90.0, &[0.5, 1.0]).expect("build");
    match sg.tree() {
        cham_search::subgame::Node::Decision { player, .. } => {
            assert_eq!(player, 0, "no gadget => root is hero (player 0)")
        }
        _ => panic!("root should be a decision"),
    }
}

#[test]
fn gadget_root_is_villain_optout() {
    let (h, v) = classes();
    let v_bp = vec![1.5, -0.5]; // blueprint CFV per villain class (hero-relative)
    let sg = Subgame::build(h, v, 20.0, 90.0, &[0.5, 1.0])
        .expect("build")
        .with_opponent_optout(v_bp.clone());
    match sg.tree() {
        cham_search::subgame::Node::Decision {
            player,
            actions,
            children,
        } => {
            assert_eq!(player, 1, "gadget => root is villain (player 1)");
            assert_eq!(actions, vec!["terminate".to_string(), "play".to_string()]);
            match &children[0] {
                cham_search::subgame::Node::Terminal { kind, .. } => {
                    assert_eq!(*kind, TerminalKind::OpponentTerminates);
                }
                _ => panic!("terminate child must be a terminal"),
            }
            // the play child is the normal hero-root subtree
            match &children[1] {
                cham_search::subgame::Node::Decision { player, .. } => {
                    assert_eq!(*player, 0, "play branch is the hero root")
                }
                _ => panic!("play child must be a decision"),
            }
        }
        _ => panic!("root should be a decision"),
    }
}

#[test]
fn terminate_value_is_negated_blueprint() {
    let (h, v) = classes();
    let v_bp = vec![1.5, -0.5];
    let sg = Subgame::build(h, v, 20.0, 90.0, &[0.5, 1.0])
        .expect("build")
        .with_opponent_optout(v_bp);
    // hero-relative terminate value = -v_bp[villain_class]
    assert!((sg.terminate_value(0) - (-1.5)).abs() < 1e-12);
    assert!((sg.terminate_value(1) - (0.5)).abs() < 1e-12);
}

#[test]
fn no_gadget_terminate_value_is_zero() {
    let (h, v) = classes();
    let sg = Subgame::build(h, v, 20.0, 90.0, &[0.5, 1.0]).expect("build");
    assert_eq!(sg.terminate_value(0), 0.0);
}
