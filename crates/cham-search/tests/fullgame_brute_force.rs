//! Brute-force validation of the full-game VBR walker (same tree,
//! scalar per-pair EV, fold winner from last action).

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
    for c in board { used[c.idx() as usize] = true; }
    for v in villain_range {
        used[v[0] as usize] = true;
        used[v[1] as usize] = true;
    }
    let mut dummy = [0u8; 2];
    let mut k = 0usize;
    for c in 0..52u8 {
        if !used[c as usize] {
            dummy[k] = c; k += 1; if k == 2 { break; }
        }
    }
    if k < 2 { return None; }
    let prefix = [
        Card(villain_range[0][0]), Card(dummy[0]),
        Card(villain_range[0][1]), Card(dummy[1]),
        board[0], board[1], board[2], board[3], board[4],
    ];
    State::new(CFG, Deck::with_prefix(&prefix)).ok()
}

fn disjoint(a: &[u8; 2], b: &[u8; 2]) -> bool {
    a[0] != b[0] && a[0] != b[1] && a[1] != b[0] && a[1] != b[1]
}

#[allow(clippy::too_many_arguments)]
fn brute_ev(
    tree: &PublicTree,
    hero: &[[u8; 2]], hero_rank: &[u32],
    villain: &[[u8; 2]], villain_rank: &[u32],
    policy: &mut impl FnMut(&[Action], usize) -> Vec<f64>,
    node: u32, st: State,
    reach: &[f64],
    last_action: Option<Action>, last_actor: Option<usize>,
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
                    if !disjoint(&hero[i], &villain[j]) { continue; }
                    let ev = if hero_rank[i] > villain_rank[j] { vill_inv }
                        else if hero_rank[i] < villain_rank[j] { -hero_inv }
                        else { (vill_inv - hero_inv) / 2.0 };
                    out[i] += reach[j] * ev;
                }
            }
        } else {
            let folder = match (last_action, last_actor) {
                (Some(Action::Fold), Some(a)) => a,
                _ => {
                    let ta = st.to_act();
                    if ta == HERO_SEAT { 1 - HERO_SEAT } else { HERO_SEAT }
                }
            };
            let sign = if folder == HERO_SEAT { -hero_inv } else { vill_inv };
            for i in 0..nh {
                for j in 0..nv {
                    if disjoint(&hero[i], &villain[j]) { out[i] += reach[j] * sign; }
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
            if st2.apply(a).is_err() { continue; }
            let ev = brute_ev(
                tree, hero, hero_rank, villain, villain_rank, policy,
                n.children[i], st2, reach, Some(a), Some(n.player as usize),
            );
            for h in 0..nh { if ev[h] > best[h] { best[h] = ev[h]; } }
        }
        best
    } else {
        let mut probs = vec![vec![0.0f64; na]; nv];
        for v in 0..nv {
            let p = policy(&[], v);
            for k in 0..na { probs[v][k] = p.get(k).copied().unwrap_or(0.0); }
        }
        let mut total = vec![0.0f64; nh];
        for (i, &a) in n.actions.iter().enumerate() {
            let mut st2 = st;
            if st2.apply(a).is_err() { continue; }
            let new_reach: Vec<f64> = (0..nv).map(|v| reach[v] * probs[v][i]).collect();
            let ev = brute_ev(
                tree, hero, hero_rank, villain, villain_rank, policy,
                n.children[i], st2, &new_reach, Some(a), Some(n.player as usize),
            );
            for h in 0..nh { total[h] += ev[h]; }
        }
        total
    }
}

#[test]
#[ignore = "brute-force validation; --ignored --nocapture"]
fn fullgame_matches_brute_force() {
    let board = board_from_seed(0xB0);
    let mut avail: Vec<u8> = Vec::new();
    for c in 0..52u8 {
        if !board.iter().any(|b| b.idx() as usize == c as usize) { avail.push(c); }
    }
    let half = avail.len() / 2;
    let hero: Vec<[u8; 2]> = (0..5).map(|k| [avail[2 * k], avail[2 * k + 1]]).collect();
    let vill: Vec<[u8; 2]> = (0..5).map(|k| [avail[half + 2 * k], avail[half + 2 * k + 1]]).collect();
    let rank = |c: &[u8; 2]| -> u32 {
        (cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &board) * 1e6) as u32
    };
    let hero_rank: Vec<u32> = hero.iter().map(rank).collect();
    let vill_rank: Vec<u32> = vill.iter().map(rank).collect();

    let ladder = ActionLadder::new(&AbstractionConfig::tiny());
    let tree = PublicTree::build(CFG, &ladder, 100_000);
    eprintln!("tree nodes: {}", tree.len());

    let hw = vec![1.0 / hero.len() as f64; hero.len()];
    let vw = vec![1.0 / vill.len() as f64; vill.len()];

    let mut policy_walker = |_st: &State, _seq: &ActionSeq, na: usize, _j: usize| -> Vec<f64> {
        if na == 0 { Vec::new() } else { let mut v = vec![0.0; na]; v[0] = 1.0; v }
    };
    let mut vbr = FullGameVbr {
        tree: &tree, ladder: &ladder,
        hero_range: &hero, hero_rank: &hero_rank, hero_w: &hw,
        villain_range: &vill, villain_rank: &vill_rank, villain_w: &vw,
        cfg: CFG, hero_seat: HERO_SEAT, policy: &mut policy_walker,
    };
    let walker_bb = vbr.best_response(&board).expect("None");

    let st0 = state_for_board(&vill, &board).expect("state");
    let mut policy_brute = |_hist: &[Action], _j: usize| -> Vec<f64> {
        let mut v = vec![0.0; 4]; v[0] = 1.0; v
    };
    let ev_chips = brute_ev(
        &tree, &hero, &hero_rank, &vill, &vill_rank, &mut policy_brute,
        tree.root, st0, &vw, None, None,
    );
    let brute_bb: f64 = ev_chips.iter().zip(hw.iter()).map(|(e, w)| e * w).sum::<f64>() / CFG.bb as f64;

    eprintln!("walker: {:.12} bb", walker_bb);
    eprintln!("brute : {:.12} bb", brute_bb);
    eprintln!("diff  : {:.3e}", (walker_bb - brute_bb).abs());
    assert!((walker_bb - brute_bb).abs() < 1e-9);
}
