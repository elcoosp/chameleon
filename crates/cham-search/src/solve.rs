//! Solvers (SPECS/06 §4): FMBR / RNR(p) / ReachGadget over the subgame tree.
//! All solvers are deterministic under a fixed iteration count (Iterations budget).

use crate::budget::WallClockGuard;
use crate::subgame::{Node, Subgame};
use crate::trigger::SolverChoice;

use crate::SearchError;
use std::collections::BTreeMap;

/// Solved strategies: per node-path distributions (path = action labels joined).
#[derive(Clone, Debug, Default)]
pub struct SolveResult {
    pub our_strategy: BTreeMap<String, Vec<f64>>,
    pub their_strategy: BTreeMap<String, Vec<f64>>,
    pub iters: u32,
    pub truncated: bool,
    /// (ours, theirs) exploitability in bb on the built tree
    pub lbr_gap: (f64, f64),
}

type Strats = BTreeMap<String, Vec<f64>>;

fn uniform(n: usize) -> Vec<f64> {
    vec![1.0 / n as f64; n]
}

/// Collect decision nodes with their paths.
fn collect<'a>(node: &'a Node, path: &str, out: &mut Vec<(String, u8, Vec<String>, &'a Node)>) {
    match node {
        Node::Terminal { .. } => {}
        Node::Decision {
            player,
            actions,
            children,
        } => {
            out.push((path.to_string(), *player, actions.clone(), node));
            for (a, c) in actions.iter().zip(children.iter()) {
                let p = if path.is_empty() {
                    a.clone()
                } else {
                    format!("{path}/{a}")
                };
                collect(c, &p, out);
            }
        }
    }
}

/// Expected value given both strategies (recursion over the tree).
#[allow(clippy::only_used_in_recursion)]
fn ev(
    node: &Node,
    seat: u8,
    path: &str,
    strats: &Strats,
    sg: &Subgame,
    hero_c: usize,
    vill_c: usize,
) -> f64 {
    match node {
        Node::Terminal {
            hero_invested,
            villain_invested,
        } => {
            let hero_v = sg.showdown_value(
                &sg.hero_classes[hero_c],
                &sg.villain_classes[vill_c],
                *hero_invested,
                *villain_invested,
            );
            if seat == 0 { hero_v } else { -hero_v }
        }
        Node::Decision {
            player: _,
            actions,
            children,
        } => {
            let key = path;
            let probs = strats
                .get(key)
                .cloned()
                .unwrap_or_else(|| uniform(actions.len()));
            let mut ev_sum = 0.0;
            let mut total_p = 0.0;
            for (a, child) in actions.iter().zip(children.iter()) {
                let p = probs
                    .get(actions.iter().position(|x| x == a).unwrap_or(0))
                    .copied()
                    .unwrap_or(0.0);
                if p <= 1e-12 {
                    continue;
                }
                let next = if path.is_empty() {
                    a.clone()
                } else {
                    format!("{key}/{a}")
                };
                ev_sum += p * ev(child, seat, &next, strats, sg, hero_c, vill_c);
                total_p += p;
            }
            if total_p <= 1e-12 {
                // all-zero: take first child
                let next = if path.is_empty() {
                    actions[0].clone()
                } else {
                    format!("{key}/{}", actions[0])
                };
                return ev(&children[0], seat, &next, strats, sg, hero_c, vill_c);
            }
            ev_sum / total_p
        }
    }
}

/// Average EV over class pairs weighted by ranges.
fn avg_ev(node: &Node, seat: u8, path: &str, strats: &Strats, sg: &Subgame) -> f64 {
    let mut total = 0.0;
    for (hi, hc) in sg.hero_classes.iter().enumerate() {
        for (vi, vc) in sg.villain_classes.iter().enumerate() {
            let pair_weight = hc.weight * vc.weight;
            total += pair_weight * ev(node, seat, path, strats, sg, hi, vi);
        }
    }
    total
}

/// Best response value for `seat` against the opponent's fixed strategy.
#[allow(clippy::only_used_in_recursion)]
fn br_value(
    node: &Node,
    seat: u8,
    opp_seat: u8,
    path: &str,
    opp_strats: &Strats,
    sg: &Subgame,
    hero_c: usize,
    vill_c: usize,
) -> f64 {
    match node {
        Node::Terminal {
            hero_invested,
            villain_invested,
        } => {
            let hero_v = sg.showdown_value(
                &sg.hero_classes[hero_c],
                &sg.villain_classes[vill_c],
                *hero_invested,
                *villain_invested,
            );
            if seat == 0 { hero_v } else { -hero_v }
        }
        Node::Decision {
            player,
            actions,
            children,
        } => {
            if *player == seat {
                // max over actions (BR)
                let mut best = f64::NEG_INFINITY;
                for (a, child) in actions.iter().zip(children.iter()) {
                    let next = if path.is_empty() {
                        a.clone()
                    } else {
                        format!("{path}/{a}")
                    };
                    let v = br_value(child, seat, opp_seat, &next, opp_strats, sg, hero_c, vill_c);
                    if v > best {
                        best = v;
                    }
                }
                best
            } else {
                // opponent fixed: expectation over their strategy
                let key = path;
                let probs = opp_strats
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| uniform(actions.len()));
                let mut ev_sum = 0.0;
                let mut total_p = 0.0;
                for (a, child) in actions.iter().zip(children.iter()) {
                    let p = probs
                        .get(actions.iter().position(|x| x == a).unwrap_or(0))
                        .copied()
                        .unwrap_or(0.0);
                    if p <= 1e-12 {
                        continue;
                    }
                    let next = if path.is_empty() {
                        a.clone()
                    } else {
                        format!("{path}/{a}")
                    };
                    ev_sum +=
                        p * br_value(child, seat, opp_seat, &next, opp_strats, sg, hero_c, vill_c);
                    total_p += p;
                }
                if total_p <= 1e-12 {
                    let next = if path.is_empty() {
                        actions[0].clone()
                    } else {
                        format!("{path}/{}", actions[0])
                    };
                    return br_value(
                        &children[0],
                        seat,
                        opp_seat,
                        &next,
                        opp_strats,
                        sg,
                        hero_c,
                        vill_c,
                    );
                }
                ev_sum / total_p
            }
        }
    }
}

fn avg_br(node: &Node, seat: u8, opp_seat: u8, opp_strats: &Strats, sg: &Subgame) -> f64 {
    let mut total = 0.0;
    for (hi, hc) in sg.hero_classes.iter().enumerate() {
        for (vi, vc) in sg.villain_classes.iter().enumerate() {
            total +=
                hc.weight * vc.weight * br_value(node, seat, opp_seat, "", opp_strats, sg, hi, vi);
        }
    }
    total
}

/// CFR+ iteration over the tree for both seats (used by RNR / ReachGadget).
#[allow(clippy::too_many_arguments, unused_variables)]
fn cfr_plus(
    sg: &Subgame,
    tree: &Node,
    nodes: &[(String, u8, Vec<String>, &Node)],
    iters: u32,
    guard: &WallClockGuard,
    villain_override: Option<&Strats>,
    villain_override_p: f64,
) -> (Strats, Strats, u32) {
    // regrets per (path, seat)
    let mut regret: BTreeMap<(String, u8), Vec<f64>> = BTreeMap::new();
    let mut strat_sum: BTreeMap<(String, u8), Vec<f64>> = BTreeMap::new();
    let mut iters_done = 0u32;
    for t in 0..iters {
        if guard.expired() {
            break;
        }
        // current strategies from regrets (RM+)
        let mut current: Strats = BTreeMap::new();
        for (path, player, actions, _n) in nodes.iter() {
            let key = (path.clone(), *player);
            let n = actions.len();
            let entry = regret.entry(key).or_insert_with(|| vec![0.0; n]);
            let pos: Vec<f64> = entry.iter().map(|&r| r.max(0.0)).collect();
            let sum: f64 = pos.iter().sum();
            let cur = if sum > 1e-12 {
                pos.iter().map(|&p| p / sum).collect()
            } else {
                uniform(n)
            };
            current.insert(path.clone(), cur);
        }
        // effective strategies: villain override blending for RNR
        let mut effective: Strats = current.clone();
        if let Some(prior) = villain_override {
            for (path, player, actions, _n) in nodes.iter() {
                if *player != 1 {
                    continue;
                }
                let learned = current
                    .get(path)
                    .cloned()
                    .unwrap_or_else(|| uniform(actions.len()));
                let prior_s = prior
                    .get(path)
                    .cloned()
                    .unwrap_or_else(|| uniform(actions.len()));
                let blended: Vec<f64> = (0..actions.len())
                    .map(|i| {
                        villain_override_p * prior_s.get(i).copied().unwrap_or(0.0)
                            + (1.0 - villain_override_p) * learned.get(i).copied().unwrap_or(0.0)
                    })
                    .collect();
                effective.insert(path.clone(), blended);
            }
        }
        // CFR+ update: for each seat, per infoset per action, counterfactual value
        let mut new_regret: BTreeMap<(String, u8), Vec<f64>> = BTreeMap::new();
        for (path, player, actions, node) in nodes.iter() {
            let key = (path.clone(), *player);
            let opp = 1 - player;
            // node value for this player under effective strategies
            let n = actions.len();
            let mut v = vec![0.0; n];
            for (ai, a) in actions.iter().enumerate() {
                let next = if path.is_empty() {
                    a.clone()
                } else {
                    format!("{path}/{a}")
                };
                let child = match node {
                    Node::Decision { children, .. } => &children[ai.min(children.len() - 1)],
                    _ => unreachable!("cham-search: invariant I2"),
                };
                v[ai] = avg_ev_child(child, *player, &next, &effective, sg);
            }
            let node_v: f64 = {
                let cur = current.get(path).cloned().unwrap_or_else(|| uniform(n));
                (0..n)
                    .map(|i| cur.get(i).copied().unwrap_or(0.0) * v[i])
                    .sum()
            };
            let entry = new_regret
                .entry(key.clone())
                .or_insert_with(|| vec![0.0; n]);
            for i in 0..n {
                entry[i] += (v[i] - node_v).max(0.0);
            }
            let _ = opp;
        }
        regret = new_regret;
        // accumulate average strategy
        for (path, player, actions, _n) in nodes.iter() {
            let key = (path.clone(), *player);
            let n = actions.len();
            let cur = current.get(path).cloned().unwrap_or_else(|| uniform(n));
            let e = strat_sum.entry(key).or_insert_with(|| vec![0.0; n]);
            for i in 0..n {
                e[i] += cur[i];
            }
        }
        iters_done = t + 1;
    }
    // normalize averages
    let mut our = Strats::new();
    let mut their = Strats::new();
    for ((path, player), sums) in &strat_sum {
        let total: f64 = sums.iter().sum();
        let norm = if total > 1e-12 {
            sums.iter().map(|&s| s / total).collect()
        } else {
            uniform(sums.len())
        };
        if *player == 0 {
            our.insert(path.clone(), norm);
        } else {
            their.insert(path.clone(), norm);
        }
    }
    (our, their, iters_done)
}

/// Average EV of a subtree for `seat` (helper for the CFR+ update).
fn avg_ev_child(node: &Node, seat: u8, path: &str, strats: &Strats, sg: &Subgame) -> f64 {
    let mut total = 0.0;
    for (hi, hc) in sg.hero_classes.iter().enumerate() {
        for (vi, vc) in sg.villain_classes.iter().enumerate() {
            total += hc.weight * vc.weight * ev(node, seat, path, strats, sg, hi, vi);
        }
    }
    total
}

/// Solve per the chosen solver (SPECS/06 §4).
pub fn solve(
    sg: &Subgame,
    prior: &crate::prior::PriorStrats,
    choice: &SolverChoice,
    iters: u32,
) -> Result<SolveResult, SearchError> {
    let guard = WallClockGuard::new(&crate::budget::SearchBudget::Iterations { iters });
    let tree = sg.tree();
    let mut nodes: Vec<(String, u8, Vec<String>, &Node)> = Vec::new();
    collect(&tree, "", &mut nodes);

    match choice {
        SolverChoice::Fmbr => {
            // hero best-responds to the prior villain; villain frozen at prior
            let mut our: Strats = BTreeMap::new();
            for (path, player, actions, _n) in &nodes {
                if *player == 0 {
                    let mut best = 0usize;
                    let mut best_v = f64::NEG_INFINITY;
                    for (ai, a) in actions.iter().enumerate() {
                        let next = if path.is_empty() {
                            a.clone()
                        } else {
                            format!("{path}/{a}")
                        };
                        let child = match _n {
                            Node::Decision { children, .. } => {
                                &children[ai.min(children.len() - 1)]
                            }
                            _ => unreachable!("cham-search: invariant I2"),
                        };
                        let v = avg_ev_child(child, 0, &next, &prior.strat, sg);
                        if v > best_v {
                            best_v = v;
                            best = ai;
                        }
                    }
                    our.insert(path.clone(), one_hot(best, actions.len()));
                }
            }
            let their = prior.strat.clone();
            let both = merge(our.clone(), their.clone());
            let our_gap = avg_br(&tree, 1, 0, &our, sg) - avg_ev(&tree, 1, "", &both, sg);
            let their_gap = avg_br(&tree, 0, 1, &their, sg) - avg_ev(&tree, 0, "", &both, sg);
            Ok(SolveResult {
                our_strategy: our,
                their_strategy: their,
                iters: 0,
                truncated: false,
                lbr_gap: (our_gap, their_gap),
            })
        }
        SolverChoice::Rnr { p } => {
            let (our, their, done) =
                cfr_plus(sg, &tree, &nodes, iters, &guard, Some(&prior.strat), *p);
            let both = merge(our.clone(), their.clone());
            let our_gap = avg_br(&tree, 1, 0, &our, sg) - avg_ev(&tree, 1, "", &both, sg);
            let their_gap = avg_br(&tree, 0, 1, &their, sg) - avg_ev(&tree, 0, "", &both, sg);
            Ok(SolveResult {
                our_strategy: our,
                their_strategy: their,
                iters: done,
                truncated: done < iters,
                lbr_gap: (our_gap, their_gap),
            })
        }
        SolverChoice::ReachGadget => {
            // conservative arm: villain clamped halfway toward uniform (the gadget's
            // off-tree compensation); hero solves vs that mixture
            let mut clamped: Strats = BTreeMap::new();
            for (path, player, actions, _n) in nodes.iter() {
                if *player != 1 {
                    continue;
                }
                let prior_s = prior
                    .strat
                    .get(path)
                    .cloned()
                    .unwrap_or_else(|| uniform(actions.len()));
                let u = uniform(actions.len());
                clamped.insert(
                    path.clone(),
                    prior_s
                        .iter()
                        .zip(u.iter())
                        .map(|(&a, &b)| 0.5 * a + 0.5 * b)
                        .collect(),
                );
            }
            let (our, their, done) =
                cfr_plus(sg, &tree, &nodes, iters, &guard, Some(&clamped), 1.0);
            let both = merge(our.clone(), their.clone());
            let our_gap = avg_br(&tree, 1, 0, &our, sg) - avg_ev(&tree, 1, "", &both, sg);
            let their_gap = avg_br(&tree, 0, 1, &their, sg) - avg_ev(&tree, 0, "", &both, sg);
            Ok(SolveResult {
                our_strategy: our,
                their_strategy: their,
                iters: done,
                truncated: done < iters,
                lbr_gap: (our_gap, their_gap),
            })
        }
    }
}

/// Average hero EV of a (our, their) strategy pair on the subgame tree.
pub fn evaluate(sg: &Subgame, our: &Strats, their: &Strats) -> f64 {
    let tree = sg.tree();
    let both = merge(our.clone(), their.clone());
    avg_ev(&tree, 0, "", &both, sg)
}

fn one_hot(i: usize, n: usize) -> Vec<f64> {
    (0..n).map(|k| if k == i { 1.0 } else { 0.0 }).collect()
}

fn merge(a: Strats, b: Strats) -> Strats {
    let mut m = a;
    for (k, v) in b {
        m.insert(k, v);
    }
    m
}
