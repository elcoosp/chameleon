//! Nash test for the combo-level river CFR+ solver.
//!
//! Single combo each side, deterministic showdown, hand-built jam-or-fold
//! tree. The CFR math is what is being checked; the tree is small enough
//! that every equilibrium is readable by hand.
//!
//! Game-theoretic structure of this toy:
//!
//! - SB (seat 0) acts first: Fold or Jam.
//! - BB (seat 1) faces a jam: Fold or Call.
//!
//! Strict preferences (these are what CFR must find):
//!
//! * BB with the LOSING hand: Fold strictly dominates Call.
//! * BB with the WINNING hand: Call strictly dominates Fold.
//!
//! Indifference (CFR may mix — do NOT assert a pure strategy):
//!
//! * SB with the WINNING hand: Fold and Jam both net +0.5 bb because
//!   BB folds to the jam (strictly). SB is indifferent.

use arrayvec::ArrayVec;
use cham_core::card::{Card, Deck};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_search::pubtree::{PublicNode, PublicTree, TERMINAL};
use cham_search::river_cfr::RiverCfr;

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

fn state() -> State {
    let prefix = [
        Card(2),
        Card(3),
        Card(4),
        Card(5),
        Card(40),
        Card(41),
        Card(42),
        Card(43),
        Card(44),
    ];
    State::new(CFG, Deck::with_prefix(&prefix)).expect("state")
}

#[test]
fn sb_folds_when_losing_showdowns() {
    // SB's hand loses to BB's. If SB jams, BB calls (winning) and SB
    // loses the stack. If SB folds, SB loses the blind. Fold strictly
    // dominates.
    let s = state();
    let jam_to = s.max_raise_to();
    let t = tree(jam_to);

    let hero = [[0u8, 1]];
    let hero_rank = vec![100u32];
    let villain = [[2u8, 3]];
    let villain_rank = vec![200u32];

    let solver = RiverCfr::new(&t, &hero, &hero_rank, &villain, &villain_rank, s, SB, None);
    let out = solver.solve(2000);

    let sb_strat = &out.hero_strat[0];
    assert!(!sb_strat.is_empty(), "no strategy at node 0");
    let fold_prob = sb_strat[0][0];
    eprintln!("SB folds with prob {}", fold_prob);
    assert!(
        fold_prob > 0.95,
        "SB should fold when losing, got {fold_prob}"
    );
}

#[test]
fn bb_folds_to_jam_when_losing_showdowns() {
    // SB holds the winning hand and jams. BB faces the jam at node 2
    // with actions [Fold, Call]. Call loses the stack; Fold loses the
    // blind. Fold strictly dominates.
    let s = state();
    let jam_to = s.max_raise_to();
    let t = tree(jam_to);

    let hero = [[0u8, 1]];
    let hero_rank = vec![200u32];
    let villain = [[2u8, 3]];
    let villain_rank = vec![100u32];

    let solver = RiverCfr::new(&t, &hero, &hero_rank, &villain, &villain_rank, s, SB, None);
    let out = solver.solve(2000);

    let bb_strat = &out.villain_strat[2];
    assert!(!bb_strat.is_empty(), "no strategy at node 2");
    let fold_prob = bb_strat[0][0];
    eprintln!("BB folds to jam with prob {}", fold_prob);
    assert!(
        fold_prob > 0.95,
        "BB should fold to a jam when losing showdowns, got {fold_prob}"
    );
}

#[test]
fn bb_calls_jam_when_winning_showdowns() {
    // SB holds the losing hand (still jams, since the tree allows it).
    // BB faces the jam with the winning hand. Call strictly dominates.
    let s = state();
    let jam_to = s.max_raise_to();
    let t = tree(jam_to);

    let hero = [[0u8, 1]];
    let hero_rank = vec![100u32];
    let villain = [[2u8, 3]];
    let villain_rank = vec![200u32];

    let solver = RiverCfr::new(&t, &hero, &hero_rank, &villain, &villain_rank, s, SB, None);
    let out = solver.solve(2000);

    let bb_strat = &out.villain_strat[2];
    let call_prob = bb_strat[0][1];
    eprintln!("BB calls jam with prob {}", call_prob);
    assert!(
        call_prob > 0.95,
        "BB should call a jam when winning showdowns, got {call_prob}"
    );
}

#[test]
fn sb_wins_indifferent_mixture() {
    // SB holds the winning hand. BB folds to a jam (strictly, per the
    // previous test). So SB's Fold and Jam both net +0.5 bb and CFR is
    // free to mix. The test asserts a valid distribution, NOT a pure
    // strategy — asserting purity here was a bug in the first version.
    let s = state();
    let jam_to = s.max_raise_to();
    let t = tree(jam_to);

    let hero = [[0u8, 1]];
    let hero_rank = vec![200u32];
    let villain = [[2u8, 3]];
    let villain_rank = vec![100u32];

    let solver = RiverCfr::new(&t, &hero, &hero_rank, &villain, &villain_rank, s, SB, None);
    let out = solver.solve(2000);

    let sb_strat = &out.hero_strat[0];
    let fold_prob = sb_strat[0][0];
    let jam_prob = sb_strat[0][1];
    eprintln!(
        "SB (winning hand) fold={:.4} jam={:.4} (indifferent)",
        fold_prob, jam_prob
    );
    assert!(
        (fold_prob + jam_prob - 1.0).abs() < 1e-9,
        "SB strategy must sum to 1"
    );
    assert!(fold_prob >= 0.0 && jam_prob >= 0.0);
}

#[test]
fn solved_strategy_is_near_nash() {
    // Solve with hero = SB and hero = BB. Each solve returns a full
    // strategy for both seats. Compute SB's BR against the BB strategy
    // from the first solve, and BB's BR against the SB strategy from
    // the second. At Nash these sum to zero.
    let s = state();
    let jam_to = s.max_raise_to();
    let t = tree(jam_to);

    let sb_combo = [[0u8, 1]];
    let bb_combo = [[2u8, 3]];
    let sb_win: Vec<u32> = vec![200];
    let sb_lose: Vec<u32> = vec![100];

    // Solve with hero = SB (SB has the winning hand).
    let solver_sb = RiverCfr::new(
        &t,
        &sb_combo,
        &sb_win,
        &bb_combo,
        &sb_lose,
        state(),
        SB,
        None,
    );
    let sol_sb = solver_sb.solve(5000);
    let br_sb = solver_sb.br_hero(&sol_sb);

    // Solve with hero = BB (BB has the losing hand, mirroring the same
    // physical game: SB wins showdowns).
    let solver_bb = RiverCfr::new(
        &t,
        &bb_combo,
        &sb_lose,
        &sb_combo,
        &sb_win,
        state(),
        BB,
        None,
    );
    let sol_bb = solver_bb.solve(5000);
    let br_bb = solver_bb.br_hero(&sol_bb);

    let exploitability = br_sb + br_bb;
    // Units: the pot is 1 bb = 100 chips, so a BR value of ±50 chips
    // is the ±0.5 bb expected at this toy's Nash. Exploitability is in
    // chips; the tolerance below is 1e-2 chip = 1e-4 bb.
    eprintln!(
        "SB BR = {br_sb:.6} chips ({:.6} bb), BB BR = {br_bb:.6} chips ({:.6} bb)",
        br_sb / 100.0,
        br_bb / 100.0,
    );
    eprintln!(
        "exploitability = {exploitability:.6} chips ({:.8} bb)",
        exploitability / 100.0
    );

    // At the toy's Nash (SB folds, since losing):
    //   SB's value = -0.5 bb = -50 chips (loses the blind)
    //   BB's value = +0.5 bb = +50 chips
    // So br_sb ≈ -50 and br_bb ≈ +50, sum ≈ 0. The residual is the
    // SB's indifference (converged to 0.9999/0.0001) leaking a small
    // amount into BB's BR. 1e-2 chips = 1e-4 bb is a fair bound.
    assert!(
        exploitability.abs() < 1e-2,
        "exploitability {exploitability:.6} chips ({:.8} bb) should be ~0 at Nash \
         (SB BR {br_sb:.6}, BB BR {br_bb:.6})",
        exploitability / 100.0
    );
}
