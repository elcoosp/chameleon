//! Diagnostic: PublicTree size vs the `cap_nodes` argument to
//! `PublicTree::build`, run at `#[ignore]` so it does not fire in CI.
//!
//! Motivation. The smoke test at 619fba1 asserts only `|EV| < 1000`
//! and `EV.is_finite()`. It "passed" with EV = 0.0 — a value that
//! passes trivially and is the signature of a tree truncated at
//! `cap_nodes`: every leaf past the cap is a zero-payoff stub whose
//! `reached_showdown()` is false and whose hero_net is 0, so the
//! terminal branch returns 0 * mass = 0 on every path. Hero's max is
//! then 0.
//!
//! This test reports tree size at several caps. If the size equals the
//! cap for every cap up to 10M, the full tree exceeds 10M nodes and the
//! walker cannot be built on a fully materialised tree at all.

use cham_core::engine::config::EngineConfig;
use cham_engine::config::AbstractionConfig;
use cham_engine::ladder::ActionLadder;
use cham_search::pubtree::{PublicTree, TERMINAL};

const CFG: EngineConfig = EngineConfig { start_stack: 10_000, sb: 50, bb: 100 };

#[test]
#[ignore = "diagnostic; run manually with --ignored --nocapture"]
fn pubtree_size() {
    eprintln!();
    eprintln!("=== PublicTree size vs build cap (tiny ladder) ===");
    eprintln!("{:>10}  {:>12}  {:>10}", "cap", "len", "truncated");
    for cap in [1_000usize, 10_000, 100_000, 1_000_000, 10_000_000] {
        let ladder = ActionLadder::new(&AbstractionConfig::tiny());
        let t = PublicTree::build(CFG, &ladder, cap);
        eprintln!("{:>10}  {:>12}  {:>10}", cap, t.len(), t.len() == cap);
    }
    eprintln!();

    // Structural sanity: root must not be a terminal stub.
    let ladder = ActionLadder::new(&AbstractionConfig::tiny());
    let t = PublicTree::build(CFG, &ladder, 10_000_000);
    let root = &t.nodes[t.root as usize];
    assert!(
        !root.terminal,
        "PublicTree root is a terminal stub (player={} terminal={})",
        root.player, root.terminal
    );
    assert!(
        root.player != TERMINAL,
        "PublicTree root player is TERMINAL"
    );
    assert!(
        !root.actions.is_empty(),
        "PublicTree root has no actions"
    );
}
