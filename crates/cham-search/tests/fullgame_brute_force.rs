//! Brute-force validation of `FullGameVbr`.
//!
//! Walks the SAME `PublicTree` as the walker but computes EV(i, j) per
//! (hero combo, villain combo) pair with scalar loops and an explicit
//! shared-card check, instead of the O(n) kernels. Agreement to 1e-9
//! validates that the walker's vectorised reach-splitting and
//! kernel-based terminal EVs match a naive O(n * m) implementation.
//!
//! Uses the SAME reach convention as the walker and `RiverVbr`: the
//! terminal EV is `sum_j reach[j] * disjoint(i, j) * ev(i, j)` with no
//! per-hero-combo normalisation by the disjoint mass. This is a
//! CONSISTENCY check between two implementations of one convention,
//! not a validation of the convention itself. Whether `best_response`
//! should normalise by `Z_i = sum_j reach[j] * disjoint(i, j)` is a
//! separate audit (raised in the module comment of `fullgame.rs`).
//!
//! Action-set note: the tiny ladder's river slots include a non-jam
//! raise, which `RiverVbr` does not model, so the design doc's
//! river-reduction variant cannot be written directly. This test
//! replaces it as the correctness gate: it does not need any
//! correspondence between `RiverVbr`'s action set and the ladder's.

use cham_core::card::{Card, Deck, Hand2};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::ActionSeq;
use cham_engine::ladder::ActionLadder;
use cham_search::fullgame::FullGameVbr;
use cham_search::pubtree::PublicTree;

const CFG: EngineConfig = EngineConfig { start_stack: 10_000, sb: 50, bb: 100 };
const HERO_SEAT: usize = 1;

fn board_from_seed(seed: u64) -> [Card; 5] {
    let mut rng = rng_from_seed(seed);
    let mut deck: Vec<u8> = (0..52).collect();
    for i in (1..52).rev() {
        let j = (cham_core::rng::next_f64(&mut rng) * (i + 1) as f64) as usize;
        deck.swap(i, j);
    }
    [Card(deck[0]), Card(deck[1]), Card(deck[2]), Card(deck[3]), Card(deck[4])]
}

fn state_for_board(villain_range: &[[u8; 2]], board: &[Card; 5]) -> Option<State> {
    let mut used = [false; 52];
    for c in board {
        used[c.idx() as usize] = true;
    }
    for v in villain_range {
        used[v[0] as usize] = true;
        used[v[1] as usize] = true;
    }
    let mut dummy = [0u8; 2];
    let mut k = 0usize;
    for c in 0..52u8 {
        if !used[c as usize] {
            dummy[k] = c;
            k += 1;
            if k == 2 {
                break;
            }
        }
    }
    if k < 2 {
        return None;
    }
    let prefix = [
        Card(villain_range[0][0]),
        Card(dummy[0]),
        Card(villain_range[0][1]),
        Card(dummy[1]),
        board[0],
        board[1],
        board[2],
        board[3],
        board[4],
    ];
    State::new(CFG, Deck::with_prefix(&prefix)).ok()
}

fn disjoint(a: &[u8; 2], b: &[u8; 2]) -> bool {
    a[0] != b[0] && a[0] != b[1] && a[1] != b[0] && a[1] != b[1]
}

/// Scalar brute-force EV per hero combo, in chips. Mirrors `fullgame::walk`
/// structurally, but each terminal node evaluates `sum_j reach[j] *
/// disjoint(i, j) * ev(i, j)` with explicit loops.
#[allow(clippy::too_many_arguments)]
fn brute_ev(
    tree: &PublicTree,
    hero: &[[u8; 2]],
    hero_rank: &[u32],
    villain: &[[u8; 2]],
    villain_rank: &[u32],
    policy: &mut impl FnMut(&[Action], usize) -> Vec<f64>,
    node: u32,
    st: State,
    history: &mut Vec<Action>,
    reach: &[f64],
) -> Vec<f64> {
    let n = &tree.nodes[node as usize];
    let nh = hero.len();
    let nv = villain.len();

    if n.terminal {
        let stacks = st.stacks();
        let hero_inv = (CFG.start_stack - stacks[HERO_SEAT]) as f64;
        let vill_inv = (CFG.start_stack - stacks[1 - HERO_SEAT]) as f64;
        let mut out = vec![0.0f64; nh];
        if st.reached_showdown() {
            for i in 0..nh {
                for j in 0..nv {
                    if !disjoint(&hero[i], &villain[j]) {
                        continue;
                    }
                    let ev = if hero_rank[i] > villain_rank[j] {
                        vill_inv
                    } else if hero_rank[i] < villain_rank[j] {
                        -hero_inv
                    } else {
                        (vill_inv - hero_inv) / 2.0
                    };
                    out[i] += reach[j] * ev;
                }
            }
        } else {
            let hero_net = stacks[HERO_SEAT] - CFG.start_stack;
            let sign = if hero_net > 0 { vill_inv } else { -hero_inv };
            for i in 0..nh {
                for j in 0..nv {
                    if disjoint(&hero[i], &villain[j]) {
                        out[i] += reach[j] * sign;
                    }
                }
            }
        }
        return out;
    }

    let is_hero = (n.player as usize) == HERO_SEAT;
    let na = n.actions.len();

    if is_hero {
        let mut best = vec![f64::NEG_INFINITY; nh];
        for (i, &a) in n.actions.iter().enumerate() {
            let mut st2 = st;
            if st2.apply(a).is_err() {
                continue;
            }
            history.push(a);
            let ev = brute_ev(
                tree, hero, hero_rank, villain, villain_rank, policy,
                n.children[i], st2, history, reach,
            );
            history.pop();
            for h in 0..nh {
                if ev[h] > best[h] {
                    best[h] = ev[h];
                }
            }
        }
        best
    } else {
        let mut probs = vec![vec![0.0f64; na]; nv];
        for v in 0..nv {
            let p = policy(history, v);
            for k in 0..na {
                probs[v][k] = p.get(k).copied().unwrap_or(0.0);
            }
        }
        let mut total = vec![0.0f64; nh];
        for (i, &a) in n.actions.iter().enumerate() {
            let mut st2 = st;
            if st2.apply(a).is_err() {
                continue;
            }
            let new_reach: Vec<f64> = (0..nv).map(|v| reach[v] * probs[v][i]).collect();
            history.push(a);
            let ev = brute_ev(
                tree, hero, hero_rank, villain, villain_rank, policy,
                n.children[i], st2, history, &new_reach,
            );
            history.pop();
            for h in 0..nh {
                total[h] += ev[h];
            }
        }
        total
    }
}

#[test]
#[ignore = "brute-force validation; run with --ignored --nocapture"]
fn fullgame_matches_brute_force() {
    let board = board_from_seed(0xB0);

    // Card-disjoint hero/villain pools.
    let mut avail: Vec<u8> = Vec::new();
    for c in 0..52u8 {
        if !board.iter().any(|b| b.idx() as usize == c as usize) {
            avail.push(c);
        }
    }
    let half = avail.len() / 2;
    let hero_pool = &avail[..half];
    let vill_pool = &avail[half..];

    let hero: Vec<[u8; 2]> = (0..5).map(|k| [hero_pool[2 * k], hero_pool[2 * k + 1]]).collect();
    let vill: Vec<[u8; 2]> = (0..5).map(|k| [vill_pool[2 * k], vill_pool[2 * k + 1]]).collect();

    let rank = |c: &[u8; 2]| -> u32 {
        (cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &board) * 1e6) as u32
    };
    let hero_rank: Vec<u32> = hero.iter().map(rank).collect();
    let vill_rank: Vec<u32> = vill.iter().map(rank).collect();

    let ladder = ActionLadder::new(&AbstractionConfig::tiny());
    let tree = PublicTree::build(CFG, &ladder, 100_000);
    assert!(tree.len() > 1, "tree collapsed (len = {})", tree.len());
    eprintln!("tree nodes: {}", tree.len());

    let hw = vec![1.0 / hero.len() as f64; hero.len()];
    let vw = vec![1.0 / vill.len() as f64; vill.len()];

    // Deterministic policy: villain always takes action 0 (check when
    // checked to, fold when facing a bet). Same behaviour in both
    // walkers. Length-4 vector is safe: the walker truncates via
    // `probs.get(i).unwrap_or(0.0)`, and any trailing zeros are
    // ignored at 2- and 3-action nodes.
    let mut policy_walker = |_hist: &[Action], _seq: &ActionSeq, _j: usize| -> Vec<f64> {
        vec![1.0, 0.0, 0.0, 0.0]
    };

    let mut vbr = FullGameVbr {
        tree: &tree,
        hero_range: &hero,
        hero_rank: &hero_rank,
        hero_w: &hw,
        villain_range: &vill,
        villain_rank: &vill_rank,
        villain_w: &vw,
        cfg: CFG,
        hero_seat: HERO_SEAT,
        policy: &mut policy_walker,
    };
    let walker_bb = vbr.best_response(&board).expect("walker returned None");

    let st0 = state_for_board(&vill, &board).expect("state");
    let mut policy_brute = |_hist: &[Action], _j: usize| -> Vec<f64> {
        vec![1.0, 0.0, 0.0, 0.0]
    };
    let mut history: Vec<Action> = Vec::new();
    let ev_chips = brute_ev(
        &tree, &hero, &hero_rank, &vill, &vill_rank, &mut policy_brute,
        tree.root, st0, &mut history, &vw,
    );
    let brute_chips: f64 = ev_chips.iter().zip(hw.iter()).map(|(e, w)| e * w).sum();
    let brute_bb = brute_chips / CFG.bb as f64;

    eprintln!("walker: {:.12} bb", walker_bb);
    eprintln!("brute : {:.12} bb", brute_bb);
    eprintln!("diff  : {:.3e} bb", (walker_bb - brute_bb).abs());

    assert!(
        (walker_bb - brute_bb).abs() < 1e-9,
        "walker and brute force disagree: walker={} brute={}",
        walker_bb, brute_bb
    );
}
