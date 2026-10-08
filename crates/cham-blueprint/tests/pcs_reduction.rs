//! Small-tree correctness gate for the PCS walk.
//!
//! Design doc §Testing 1 asks for a "small-deck reduction": train on a
//! tiny game where the equilibrium is knowable, and verify the walk
//! finds it. The engine is fixed at 52 cards, so instead of a small
//! deck we use a HAND-BUILT `PublicTree` — a 5-node jam-or-fold toy
//! whose Nash equilibrium is analytically trivial. The walk code path
//! is the production one (`PcsIteration::run`); only the tree is small.
//!
//! Toy shape (seats: 0 = SB = villain, 1 = BB = hero):
//!
//!     node 0: SB to act, actions = [Fold, Jam]
//!       Fold -> node 1 (terminal, SB folded)
//!       Jam  -> node 2
//!     node 2: BB to act, actions = [Fold, Call]
//!       Fold -> node 3 (terminal, BB folded)
//!       Call -> node 4 (terminal, all-in showdown)
//!
//! With single-combo ranges and a fixed board, the showdown outcome is
//! deterministic. Two rank assignments give forced equilibria:
//!
//! A. hero_rank > villain_rank (hero wins showdown): SB's best response
//!    to "BB calls a jam" is to fold; the equilibrium is
//!    SB = 100% fold. Hero EV = +0.5 bb (SB's lost blind).
//! B. hero_rank < villain_rank (villain wins showdown): BB's best
//!    response to a jam is to fold, so SB's best response is to jam.
//!    Equilibrium = SB 100% jam, BB 100% fold. Hero EV = -1.0 bb.
//!
//! If the walk converges to those strategies, the regret/reach
//! machinery is at least locally correct on a case we can verify.

use arrayvec::ArrayVec;
use cham_blueprint::pcs::table::RegretTable;
use cham_blueprint::pcs::walk::PcsIteration;
use cham_core::card::Card;
use cham_core::engine::Action;
use cham_core::engine::config::EngineConfig;
use cham_core::obs::{Observables, Player};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};
use cham_engine::ladder::ActionLadder;
use cham_search::pubtree::{PublicNode, PublicTree, TERMINAL};

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};
const HERO_SEAT: usize = 1;
const VILL_SEAT: usize = 0;

/// Hand-built 5-node jam-or-fold tree. `jam_to` is the SB's all-in bet
/// level (read from a fresh state's `max_raise_to`).
fn jam_or_fold_tree(jam_to: i64) -> PublicTree {
    let mut nodes: Vec<PublicNode> = Vec::with_capacity(5);

    // node 0: SB {Fold, Jam}
    // Preflop: the SB's all-in is a RAISE (a `Bet` is postflop-only and
    // would be rejected by `State::apply`, silently turning the Jam
    // branch into a zero-EV stub — the bug that made both toys
    // converge to jamming).
    let mut a0: ArrayVec<Action, 12> = ArrayVec::new();
    a0.push(Action::Fold);
    a0.push(Action::Raise { to: jam_to });
    let mut c0: ArrayVec<u32, 12> = ArrayVec::new();
    c0.push(1);
    c0.push(2);
    nodes.push(PublicNode {
        player: VILL_SEAT as u8,
        actions: a0,
        children: c0,
        terminal: false,
    });

    // node 1: terminal (SB folded)
    nodes.push(PublicNode {
        player: TERMINAL,
        actions: ArrayVec::new(),
        children: ArrayVec::new(),
        terminal: true,
    });

    // node 2: BB {Fold, Call}
    let mut a2: ArrayVec<Action, 12> = ArrayVec::new();
    a2.push(Action::Fold);
    a2.push(Action::Call);
    let mut c2: ArrayVec<u32, 12> = ArrayVec::new();
    c2.push(3);
    c2.push(4);
    nodes.push(PublicNode {
        player: HERO_SEAT as u8,
        actions: a2,
        children: c2,
        terminal: false,
    });

    // node 3: terminal (BB folded)
    nodes.push(PublicNode {
        player: TERMINAL,
        actions: ArrayVec::new(),
        children: ArrayVec::new(),
        terminal: true,
    });

    // node 4: terminal (all-in showdown)
    nodes.push(PublicNode {
        player: TERMINAL,
        actions: ArrayVec::new(),
        children: ArrayVec::new(),
        terminal: true,
    });

    PublicTree { nodes, root: 0 }
}

fn board() -> [Card; 5] {
    // Five cards disjoint from the four hole cards used below.
    [Card(40), Card(41), Card(42), Card(43), Card(44)]
}

fn read_strategy_at_root(
    table: &RegretTable,
    encoder: &mut Encoder,
    ladder: &ActionLadder,
) -> Vec<f64> {
    // Reconstruct the root's `Observables` — the walk seeds the state
    // with free (dummy) holes disjoint from the board; the KEY depends
    // only on (hole, board, street), and for the root the walk's own
    // dummy hole is the lowest unused card. Use the same construction.
    let b = board();
    let mut used = [false; 52];
    for c in &b {
        used[c.idx() as usize] = true;
    }
    let mut free = [0u8; 4];
    let mut k = 0usize;
    for c in 0..52u8 {
        if !used[c as usize] {
            free[k] = c;
            k += 1;
            if k == 4 {
                break;
            }
        }
    }
    let prefix = [
        Card(free[0]),
        Card(free[1]),
        Card(free[2]),
        Card(free[3]),
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
    ];
    let st = cham_core::engine::State::new(CFG, cham_core::card::Deck::with_prefix(&prefix))
        .expect("state");
    let obs = Observables::view(&st, Player::from_usize(VILL_SEAT));
    let seq = ActionSeq::default();
    let slots = ladder.slots(&obs, &seq);
    let key = encoder.key_for(&obs, &seq, &slots).0;
    table
        .row(key)
        .map(|r| r.current_strategy())
        .unwrap_or_default()
}

fn run_toy(iters: u64, hero_rank: u32, villain_rank: u32) -> (Vec<f64>, RegretTable) {
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);

    // Read max_raise_to from a fresh state so the jam action is legal.
    let b = board();
    let mut used = [false; 52];
    for c in &b {
        used[c.idx() as usize] = true;
    }
    let mut free = [0u8; 4];
    let mut k = 0usize;
    for c in 0..52u8 {
        if !used[c as usize] {
            free[k] = c;
            k += 1;
            if k == 4 {
                break;
            }
        }
    }
    let prefix = [
        Card(free[0]),
        Card(free[1]),
        Card(free[2]),
        Card(free[3]),
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
    ];
    let probe = cham_core::engine::State::new(CFG, cham_core::card::Deck::with_prefix(&prefix))
        .expect("probe state");
    let jam_to = probe.max_raise_to();

    let tree = jam_or_fold_tree(jam_to);

    let hero: Vec<[u8; 2]> = vec![[0, 1]];
    let villain: Vec<[u8; 2]> = vec![[2, 3]];
    let hero_rank_v: Vec<u32> = vec![hero_rank];
    let villain_rank_v: Vec<u32> = vec![villain_rank];

    let mut encoder = Encoder::cfg_only(cfg.clone()).expect("enc");
    let mut table = RegretTable::new();

    let iter = PcsIteration {
        tree: &tree,
        ladder: &ladder,
        hero_range: &hero,
        hero_rank: &hero_rank_v,
        villain_range: &villain,
        villain_rank: &villain_rank_v,
        cfg: CFG,
        hero_seat: HERO_SEAT,
    };

    for t in 1..=iters {
        iter.run(&mut encoder, &mut table, b, t, 1.5, 0.0, 2.0);
    }

    let sigma = read_strategy_at_root(&table, &mut encoder, &ladder);
    (sigma, table)
}

#[test]
fn tree_actions_are_engine_legal_at_each_node() {
    // Guard against the Bet-vs-Raise class of bug: every action in the
    // toy tree, applied at the state that reaches its node, must be
    // accepted by the engine. If this fails, the toy is malformed and
    // any equilibrium it appears to produce is meaningless.
    let b = board();
    let mut used = [false; 52];
    for c in &b {
        used[c.idx() as usize] = true;
    }
    let mut free = [0u8; 4];
    let mut k = 0usize;
    for c in 0..52u8 {
        if !used[c as usize] {
            free[k] = c;
            k += 1;
            if k == 4 {
                break;
            }
        }
    }
    let prefix = [
        Card(free[0]),
        Card(free[1]),
        Card(free[2]),
        Card(free[3]),
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
    ];
    let probe = cham_core::engine::State::new(CFG, cham_core::card::Deck::with_prefix(&prefix))
        .expect("probe state");
    let jam_to = probe.max_raise_to();
    let tree = jam_or_fold_tree(jam_to);

    // Walk the tree, replaying actions, asserting every one applies.
    fn check(tree: &PublicTree, node: u32, st: cham_core::engine::State, path: &mut Vec<Action>) {
        let n = &tree.nodes[node as usize];
        if n.terminal {
            return;
        }
        for (i, &a) in n.actions.iter().enumerate() {
            let mut st2 = st;
            let res = st2.apply(a);
            path.push(a);
            assert!(
                res.is_ok(),
                "node {node} action {a:?} rejected by engine (path {:?}): {:?}",
                path,
                res.err()
            );
            check(tree, n.children[i], st2, path);
            path.pop();
        }
    }
    let mut path = Vec::new();
    check(&tree, tree.root, probe, &mut path);
}

#[test]
fn hero_wins_showdowns_sb_folds() {
    // SB's Fold gives hero +0.5 bb; SB's Jam invites BB to call (+100 bb).
    // Folding is dominant. The root strategy converges to [1, 0].
    let (sigma, table) = run_toy(2_000, 200, 100);
    eprintln!("toy A: root sigma = {:?}, rows = {}", sigma, table.len());
    assert!(!sigma.is_empty(), "no strategy at root");
    assert!(
        sigma[0] > 0.99,
        "expected SB to fold ~100%, got sigma[0] = {}",
        sigma[0]
    );
}

#[test]
fn villain_wins_showdowns_sb_jams() {
    // Hero loses showdowns, so BB folds to a jam. SB then prefers to jam
    // (hero's EV is -1.0 bb) over folding (hero's EV is +0.5 bb). Root
    // strategy converges to [0, 1].
    let (sigma, table) = run_toy(2_000, 100, 200);
    eprintln!("toy B: root sigma = {:?}, rows = {}", sigma, table.len());
    assert!(!sigma.is_empty(), "no strategy at root");
    assert!(
        sigma[1] > 0.99,
        "expected SB to jam ~100%, got sigma[1] = {}",
        sigma[1]
    );
}

#[test]
fn strategies_are_valid_distributions() {
    let (sigma, _) = run_toy(500, 200, 100);
    let s: f64 = sigma.iter().sum();
    assert!(
        (s - 1.0).abs() < 1e-9,
        "root strategy does not sum to 1: sum={s}, sigma={sigma:?}"
    );
    for (i, &p) in sigma.iter().enumerate() {
        assert!((0.0..=1.0).contains(&p), "sigma[{i}] = {p} out of [0,1]");
    }
}
