//! Plan gate (Phase B): VBR at a hand-built Nash toy ≈ 0.
//!
//! Jam-or-fold toy, two opposite rank configurations. At each
//! configuration the equilibrium is analytically known; the walker's
//! exploitability (sum of both seats' best responses) must be ≈ 0.
//!
//! Hands are physical: the SB holds one hand, the BB holds another,
//! fixed across the two seat-perspective BR computations. The rank
//! passed to `br` is the RANK OF THE HERO'S HAND, not a constant:
//! swapping the hero seat swaps which rank is "hero".
//!
//! Prior bug (fixed here): a single `hero_rank` was passed to both
//! calls, so the hero always held the same (winning) hand regardless
//! of seat. Case A then reported 100.5 instead of 0.

use arrayvec::ArrayVec;
use cham_core::card::{Card, Deck};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::ActionSeq;
use cham_engine::ladder::ActionLadder;
use cham_search::fullgame::FullGameVbr;
use cham_search::pubtree::{PublicNode, PublicTree, TERMINAL};

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};
const BB: usize = 1;
const SB: usize = 0;

fn tree(jam_to: i64) -> PublicTree {
    let mut n = Vec::with_capacity(5);
    let mut a0: ArrayVec<Action, 12> = ArrayVec::new();
    a0.push(Action::Fold);
    a0.push(Action::Raise { to: jam_to });
    let mut c0: ArrayVec<u32, 12> = ArrayVec::new();
    c0.push(1);
    c0.push(2);
    n.push(PublicNode {
        player: SB as u8,
        actions: a0,
        children: c0,
        terminal: false,
    });
    n.push(PublicNode {
        player: TERMINAL,
        actions: ArrayVec::new(),
        children: ArrayVec::new(),
        terminal: true,
    });
    let mut a2: ArrayVec<Action, 12> = ArrayVec::new();
    a2.push(Action::Fold);
    a2.push(Action::Call);
    let mut c2: ArrayVec<u32, 12> = ArrayVec::new();
    c2.push(3);
    c2.push(4);
    n.push(PublicNode {
        player: BB as u8,
        actions: a2,
        children: c2,
        terminal: false,
    });
    n.push(PublicNode {
        player: TERMINAL,
        actions: ArrayVec::new(),
        children: ArrayVec::new(),
        terminal: true,
    });
    n.push(PublicNode {
        player: TERMINAL,
        actions: ArrayVec::new(),
        children: ArrayVec::new(),
        terminal: true,
    });
    PublicTree { nodes: n, root: 0 }
}

fn board() -> [Card; 5] {
    [Card(40), Card(41), Card(42), Card(43), Card(44)]
}

fn probe_state() -> State {
    let b = board();
    let prefix = [
        Card(2),
        Card(3),
        Card(4),
        Card(5),
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
    ];
    State::new(CFG, Deck::with_prefix(&prefix)).expect("state")
}

/// Hero's BR, holding `hero_rank` against a villain holding `vill_rank`.
fn br(
    tree: &PublicTree,
    ladder: &ActionLadder,
    hero_seat: usize,
    hero_rank: u32,
    vill_rank: u32,
    sb_p: [f64; 2],
    bb_p: [f64; 2],
) -> f64 {
    let hero = [[0u8, 1]];
    let villain = [[2u8, 3]];
    let hw = vec![1.0];
    let vw = vec![1.0];
    let hr = vec![hero_rank];
    let vr = vec![vill_rank];
    let mut pol = |_st: &State, path: &[Action], _seq: &ActionSeq, na: usize, _c: usize| {
        // SB acts at the root only; BB acts at the non-root decision node.
        let p = if path.is_empty() { sb_p } else { bb_p };
        p[..na.min(2)].to_vec()
    };
    let mut v = FullGameVbr {
        tree,
        ladder,
        hero_range: &hero,
        hero_rank: &hr,
        hero_w: &hw,
        villain_range: &villain,
        villain_rank: &vr,
        villain_w: &vw,
        cfg: CFG,
        hero_seat,
        policy: &mut pol,
    };
    v.best_response(&board()).expect("BR")
}

/// Exploitability = BR(SB) + BR(BB), where `rank_sb` and `rank_bb` are
/// the PHYSICAL ranks of each seat's hand (fixed regardless of who is
/// the hero).
fn exploit(sb_p: [f64; 2], bb_p: [f64; 2], rank_sb: u32, rank_bb: u32) -> f64 {
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);
    let jam_to = probe_state().max_raise_to();
    let t = tree(jam_to);
    // Hero = BB: hero holds the BB's hand.
    let br_bb = br(&t, &ladder, BB, rank_bb, rank_sb, sb_p, bb_p);
    // Hero = SB: hero holds the SB's hand. THIS is the line the prior
    // version got wrong — it passed rank_bb for both.
    let br_sb = br(&t, &ladder, SB, rank_sb, rank_bb, sb_p, bb_p);
    br_bb + br_sb
}

#[test]
#[ignore = "plan gate; --ignored --nocapture"]
fn nash_toy_exploitability_zero() {
    // A: BB's hand wins the showdown. Equilibrium: SB folds, BB calls.
    let a = exploit([1.0, 0.0], [0.0, 1.0], 100, 200);
    eprintln!("A (BB hand wins): exploit = {a:.6} bb");

    // B: SB's hand wins the showdown. Equilibrium: SB jams, BB folds.
    let b = exploit([0.0, 1.0], [1.0, 0.0], 200, 100);
    eprintln!("B (SB hand wins): exploit = {b:.6} bb");

    // Tolerance: the walker's card-removal mass is f64; with one combo
    // per side the arithmetic is exact up to a few ulps. 1e-6 is safe;
    // anything above that is a real bug, not roundoff.
    assert!(a.abs() < 1e-6, "A not 0: {a}");
    assert!(b.abs() < 1e-6, "B not 0: {b}");
}
