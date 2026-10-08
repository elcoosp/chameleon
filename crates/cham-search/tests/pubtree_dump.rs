//! Diagnostic: dump the PublicTree structure + reconstruct the engine
//! state at each node by replaying actions from the root. Purpose: find
//! out (a) which streets the tree actually spans, (b) whether terminals
//! have non-zero chip investment (so the walker's payoff branch is
//! reachable), and (c) whether any terminal is at showdown.
//!
//! Motivation. `pubtree_size` (1805406) showed the tiny-ladder tree is
//! 4124 nodes at cap 1M — not truncated, but far smaller than a 4-street
//! tree with 3-way branching should be. The smoke test (619fba1)
//! returned `Some(0.0)`, which is consistent with a tree that never
//! advances streets and never invests chips.

use cham_core::card::Deck;
use cham_core::engine::config::EngineConfig;
use cham_core::engine::State;
use cham_engine::config::AbstractionConfig;
use cham_engine::ladder::ActionLadder;
use cham_search::pubtree::PublicTree;

const CFG: EngineConfig = EngineConfig { start_stack: 10_000, sb: 50, bb: 100 };

#[test]
#[ignore = "diagnostic; run manually with --ignored --nocapture"]
fn pubtree_dump() {
    let ladder = ActionLadder::new(&AbstractionConfig::tiny());
    let t = PublicTree::build(CFG, &ladder, 1_000_000);

    let n_total = t.nodes.len();
    let n_term = t.nodes.iter().filter(|n| n.terminal).count();
    eprintln!();
    eprintln!("=== PublicTree dump (tiny ladder, cap 1M) ===");
    eprintln!("nodes: {} (terminal: {}, non-terminal: {})", n_total, n_term, n_total - n_term);

    let mut hist = [0usize; 13];
    for n in &t.nodes {
        if !n.terminal {
            let k = n.actions.len().min(12);
            hist[k] += 1;
        }
    }
    eprintln!("non-terminal action-count histogram:");
    for (k, &c) in hist.iter().enumerate() {
        if c > 0 {
            eprintln!("  {:>3} actions: {}", k, c);
        }
    }

    eprintln!("--- first 20 nodes (flat) ---");
    for (i, n) in t.nodes.iter().take(20).enumerate() {
        eprintln!(
            "node[{:>4}]: player={:>3} terminal={:<5} actions={:>2} children={:>2}",
            i, n.player, n.terminal, n.actions.len(), n.children.len()
        );
    }

    eprintln!("--- DFS from root (depth limited to 8) ---");
    let mut st = State::new(CFG, Deck::ordered()).expect("fresh state");
    dfs(&t, &mut st, t.root, 0, 8);

    eprintln!();
    eprintln!("=== DFS summary ===");
    let mut counts = TermSummary::default();
    let mut st = State::new(CFG, Deck::ordered()).expect("fresh state");
    tally(&t, &mut st, t.root, 0, 200, &mut counts);
    eprintln!("terminals reached (depth < 200): {}", counts.total);
    eprintln!("  reached_showdown:          {}", counts.showdown);
    eprintln!("  fold (non-showdown):       {}", counts.fold);
    eprintln!("  hero_inv > 0:              {}", counts.hero_inv_pos);
    eprintln!("  vill_inv > 0:              {}", counts.vill_inv_pos);
    eprintln!("  any_inv > 0:               {}", counts.any_inv_pos);
    eprintln!("  by street: pre={} flop={} turn={} river={}",
        counts.preflop, counts.flop, counts.turn, counts.river);
}

#[derive(Default)]
struct TermSummary {
    total: usize,
    showdown: usize,
    fold: usize,
    hero_inv_pos: usize,
    vill_inv_pos: usize,
    any_inv_pos: usize,
    preflop: usize,
    flop: usize,
    turn: usize,
    river: usize,
}

fn tally(
    t: &PublicTree,
    st: &mut State,
    node: u32,
    depth: usize,
    max_depth: usize,
    out: &mut TermSummary,
) {
    if depth > max_depth {
        return;
    }
    let n = &t.nodes[node as usize];
    if n.terminal {
        out.total += 1;
        if st.reached_showdown() { out.showdown += 1; } else { out.fold += 1; }
        let stacks = st.stacks();
        let hero_inv = CFG.start_stack - stacks[1];
        let vill_inv = CFG.start_stack - stacks[0];
        if hero_inv > 0 { out.hero_inv_pos += 1; }
        if vill_inv > 0 { out.vill_inv_pos += 1; }
        if hero_inv > 0 || vill_inv > 0 { out.any_inv_pos += 1; }
        match st.street() {
            cham_core::engine::Street::Preflop => out.preflop += 1,
            cham_core::engine::Street::Flop => out.flop += 1,
            cham_core::engine::Street::Turn => out.turn += 1,
            cham_core::engine::Street::River => out.river += 1,
        }
        return;
    }
    for (i, &a) in n.actions.iter().enumerate() {
        let saved = *st;
        if st.apply(a).is_err() { continue; }
        tally(t, st, n.children[i], depth + 1, max_depth, out);
        *st = saved;
    }
}

fn dfs(t: &PublicTree, st: &mut State, node: u32, depth: usize, max_depth: usize) {
    if depth > max_depth { return; }
    let n = &t.nodes[node as usize];
    let indent = "  ".repeat(depth);
    if n.terminal {
        let stacks = st.stacks();
        let pot = st.pot();
        eprintln!(
            "{}TERM st={:?} stacks={:?} pot={} reached_sd={}",
            indent, st.street(), stacks, pot, st.reached_showdown()
        );
        return;
    }
    eprintln!(
        "{}node[{}] player={} st={:?} n_actions={} board_len={}",
        indent, node, n.player, st.street(), n.actions.len(), st.board_len()
    );
    for (i, &a) in n.actions.iter().enumerate() {
        let saved = *st;
        if st.apply(a).is_err() {
            eprintln!("{}  action[{:?}] -> ERR", indent, a);
            continue;
        }
        dfs(t, st, n.children[i], depth + 1, max_depth);
        *st = saved;
    }
}
