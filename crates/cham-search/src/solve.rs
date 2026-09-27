//! Solvers (SPECS/06 §4): FMBR / RNR(p) / ReachGadget over the subgame tree.
//! All solvers are deterministic under a fixed iteration count (Iterations budget).

use crate::budget::WallClockGuard;
use crate::subgame::{Node, Subgame};
use crate::trigger::SolverChoice;

use crate::SearchError;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

/// Optional solver warm-start (BROAD-PERF-PLAN B6): OFF by default. When
/// enabled, the previous solve's final average strategy per (board class, SPR
/// band) seeds the next solve's initial regrets at the same key. Deterministic
/// (same hands → same sequence → same warm starts); the flag-off path never
/// touches the table and is bit-identical to the historical solver.
///
/// VALIDATION OUTCOME (recorded — `warmstart_oracle_validation`): cross-spot
/// transfer at 400 iters measures mean |ΔEV| ≈ 14.8 mb / worst ≈ 88 mb, above
/// the plan's 0.5 mb bar, so the flag stays opt-in. Locked in instead: zero
/// root-argmax flips over 200 spots and no exploitability degradation
/// (mean |lbr_gap.0| warm ≈ cold). Re-measure before enabling anywhere live.
static WARM_ENABLED: AtomicBool = AtomicBool::new(false);
static WARM_HITS: AtomicU64 = AtomicU64::new(0);
static WARM_MISSES: AtomicU64 = AtomicU64::new(0);

/// Warm-start regret scale: initial regrets `(warm − uniform) × scale`, so the
/// first CFR+ iteration plays near the warmed strategy. Kept gentle (1.0): a
/// warm start is a nudge, not a constraint — fixed-iteration solves must stay
/// within the oracle validation gate (mean |ΔEV| < 0.5 mb).
const WARM_SCALE: f64 = 1.0;

fn warm_table() -> &'static Mutex<HashMap<(u64, u8), Strats>> {
    static TABLE: OnceLock<Mutex<HashMap<(u64, u8), Strats>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Enable/disable solver warm-start (default off; `--search-warmstart`).
pub fn set_warm_start(enabled: bool) {
    WARM_ENABLED.store(enabled, Ordering::SeqCst);
}

/// Current warm-start switch state.
pub fn warm_start_enabled() -> bool {
    WARM_ENABLED.load(Ordering::SeqCst)
}

/// (hits, misses) on the warm-start table — measurability for the B6 gate.
pub fn warm_stats() -> (u64, u64) {
    (
        WARM_HITS.load(Ordering::Relaxed),
        WARM_MISSES.load(Ordering::Relaxed),
    )
}

/// Test-only reset (keeps the validation harness hermetic).
/// Compiled unconditionally so integration tests can use it.
pub fn warm_reset_for_tests() {
    warm_table().lock().expect("warm").clear();
    WARM_HITS.store(0, Ordering::Relaxed);
    WARM_MISSES.store(0, Ordering::Relaxed);
    WARM_ENABLED.store(false, Ordering::SeqCst);
}

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

/// Multi-leaf continuation blend, wired into the LIVE solve path (v7 Item
/// 5.3 / B-7, DeepStack): a single fixed leaf continuation is itself
/// exploitable, so villain priors are blended across {base, call-heavy,
/// fold-heavy} with LEAF_BLEND_WEIGHTS before the RNR p-blend.
///
/// Action labels here are tree strings ("check"/"bet*"/"jam"/"fold"/
/// "call"/"raise*"), not `cham_core::Action` — semantics are by label:
/// passive = "call" else "check"; weak = "fold" else passive. Tilt shifts
/// LEAF_TILT of donor mass onto the target (donors: aggressive labels for
/// call-heavy, everything-but-target for fold-heavy), exactly renormalized.
/// Unknown/mismatched shapes fall back to the base prior (never a lie).
pub fn blended_villain_prior(base: &[f64], actions: &[String]) -> Vec<f64> {
    const TILT: f64 = 0.25;
    const W: [f64; 3] = [0.6, 0.2, 0.2];
    let n = base.len();
    if n != actions.len() || n == 0 {
        return base.to_vec();
    }
    let lower: Vec<String> = actions.iter().map(|a| a.to_lowercase()).collect();
    let passive = lower
        .iter()
        .position(|a| a.starts_with("call"))
        .or_else(|| lower.iter().position(|a| a.starts_with("check")));
    let weak = lower.iter().position(|a| a.starts_with("fold")).or(passive);
    let tilt = |target: Option<usize>, aggressive_only: bool| -> Vec<f64> {
        let mut out = base.to_vec();
        let Some(t) = target else { return out };
        let is_aggr = |a: &str| a.starts_with("bet") || a.starts_with("raise") || a == "jam";
        let mut movable = 0.0;
        for (i, p) in base.iter().enumerate() {
            if i == t {
                continue;
            }
            if !aggressive_only || is_aggr(&lower[i]) {
                movable += p;
            }
        }
        let shift = movable * TILT;
        if shift <= 0.0 {
            return out;
        }
        for (i, p) in base.iter().enumerate() {
            if i == t {
                continue;
            }
            if !aggressive_only || is_aggr(&lower[i]) {
                out[i] = p - shift * (p / movable);
            }
        }
        out[t] = base[t] + shift;
        let total: f64 = out.iter().sum();
        if total > 0.0 {
            for v in out.iter_mut() {
                *v /= total;
            }
        }
        out
    };
    let ch = tilt(passive, true);
    let fh = tilt(weak, false);
    let mut out = vec![0.0; n];
    for i in 0..n {
        out[i] = W[0] * base[i] + W[1] * ch[i] + W[2] * fh[i];
    }
    let total: f64 = out.iter().sum();
    if total > 1e-12 {
        for v in out.iter_mut() {
            *v /= total;
        }
    }
    out
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
    warm: Option<&Strats>,
) -> (Strats, Strats, u32) {
    // regrets per (path, seat)
    let mut regret: BTreeMap<(String, u8), Vec<f64>> = BTreeMap::new();
    // B6 warm-start: seed initial regrets so iteration 0 plays the warmed
    // strategy (same action count required, else fall back to zeros).
    if let Some(w) = warm {
        for (path, player, actions, _n) in nodes.iter() {
            if *player != 0 {
                continue; // only OUR strategy warms (villain blends the prior)
            }
            if let Some(ws) = w.get(path) {
                if ws.len() == actions.len() {
                    let n = actions.len() as f64;
                    let entry = regret
                        .entry((path.clone(), *player))
                        .or_insert_with(|| vec![0.0; actions.len()]);
                    for (i, r) in entry.iter_mut().enumerate() {
                        *r = (ws.get(i).copied().unwrap_or(0.0) - 1.0 / n) * WARM_SCALE;
                    }
                }
            }
        }
    }
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
        // CFR+ update: for each seat, per infoset per action, counterfactual value.
        // C-1 fix (2026-09-27): accumulate regrets IN PLACE. The previous form
        // declared a fresh `new_regret` map inside this loop and overwrote
        // `regret` at the end of each iteration, so `+=` never spanned
        // iterations and RM+ could not converge (matching last-iteration's
        // instantaneous positive regrets, not cumulative).
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
            let entry = regret.entry(key.clone()).or_insert_with(|| vec![0.0; n]);
            for i in 0..n {
                entry[i] = (entry[i] + (v[i] - node_v)).max(0.0);
            }
            let _ = opp;
        }
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
    solve_with_warmkey(sg, prior, choice, iters, None)
}

/// Solve with an optional warm-start key (board class, SPR band). `None`
/// disables warming for this call; the global switch must also be on.
pub fn solve_with_warmkey(
    sg: &Subgame,
    prior: &crate::prior::PriorStrats,
    choice: &SolverChoice,
    iters: u32,
    warm_key: Option<(u64, u8)>,
) -> Result<SolveResult, SearchError> {
    let guard = WallClockGuard::new(&crate::budget::SearchBudget::Iterations { iters });
    let tree = sg.tree();
    let mut nodes: Vec<(String, u8, Vec<String>, &Node)> = Vec::new();
    collect(&tree, "", &mut nodes);
    // v7 Item 5.3 / B-7: blend the villain leaf continuation prior across
    // {base, call-heavy, fold-heavy} BEFORE any solver branch sees it, so
    // the live path (not just prior.rs unit tests) solves robust against
    // the leaves. Hero paths keep the raw prior (our strategy is solved).
    let mut blended: Strats = prior.strat.clone();
    for (path, player, actions, _n) in nodes.iter() {
        if *player != 1 {
            continue;
        }
        if let Some(base) = prior.strat.get(path) {
            if base.len() == actions.len() {
                blended.insert(path.clone(), blended_villain_prior(base, actions));
            }
        }
    }
    // B6: fetch the warmed strategy (cheap clone of small tables; a miss
    // behaves exactly like the flag-off path).
    let warmed: Option<Strats> = match (warm_key, warm_start_enabled()) {
        (Some(k), true) => {
            let hit = warm_table().lock().expect("warm").get(&k).cloned();
            if hit.is_some() {
                WARM_HITS.fetch_add(1, Ordering::Relaxed);
            } else {
                WARM_MISSES.fetch_add(1, Ordering::Relaxed);
            }
            hit
        }
        _ => None,
    };

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
                        let v = avg_ev_child(child, 0, &next, &blended, sg);
                        if v > best_v {
                            best_v = v;
                            best = ai;
                        }
                    }
                    our.insert(path.clone(), one_hot(best, actions.len()));
                }
            }
            let their = blended.clone();
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
            let (our, their, done) = cfr_plus(
                sg,
                &tree,
                &nodes,
                iters,
                &guard,
                Some(&blended),
                *p,
                warmed.as_ref(),
            );
            store_warm(warm_key, &our);
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
                let prior_s = blended
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
            let (our, their, done) = cfr_plus(
                sg,
                &tree,
                &nodes,
                iters,
                &guard,
                Some(&clamped),
                1.0,
                warmed.as_ref(),
            );
            store_warm(warm_key, &our);
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

/// Record a solve's final OUR strategy under the warm key (only when warming
/// is globally enabled and the call carried a key).
fn store_warm(warm_key: Option<(u64, u8)>, our: &Strats) {
    if let Some(k) = warm_key {
        if warm_start_enabled() {
            warm_table().lock().expect("warm").insert(k, our.clone());
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
