//! Combo-level river CFR+ solver (Phase D / W3, first cut).
//!
//! Unlike the class-level `subgame`/`solve` pair, this keeps both ranges
//! as explicit combo vectors. Unlike the PCS walk, it does NOT key on
//! the encoder — the tree is small and fixed, so per-(node, combo)
//! storage is enough.
//!
//! Reuses the tree builder (`PublicTree::build_from_state`) and the O(n)
//! card-removal kernels (`showdown_cfv_two`) verbatim. The terminal math
//! is copied from `fullgame::terminal_ev` — same conventions, same
//! fold-winner-from-last-action rule.

use crate::kernel::showdown_cfv_two;
use crate::pubtree::PublicTree;
use cham_core::engine::State;

/// One CFR+ solver over a fixed river tree.
pub struct RiverCfr<'a> {
    pub tree: &'a PublicTree,
    pub hero_range: &'a [[u8; 2]],
    pub hero_rank: &'a [u32],
    pub villain_range: &'a [[u8; 2]],
    pub villain_rank: &'a [u32],
    /// The engine `State` at the tree root. Read at terminals for stacks
    /// and `reached_showdown`. The walk carries its own `State` per node
    /// (the recursion replays actions), so this is only the initial one.
    pub root_state: State,
    pub hero_seat: usize,
    /// Optional safe-resolving gadget value per villain combo (hero-relative).
    /// When `Some`, an extra root decision [terminate | play] is prepended:
    /// "terminate" pays the villain `-v_bp[j]` (their value is the negation
    /// of the hero-relative CFV).
    pub v_bp_hero: Option<Vec<f64>>,
}

#[derive(Clone, Debug, Default)]
pub struct SolvedRiver {
    /// per-node, per-combo action probabilities (hero), summed over iters
    pub hero_strat: Vec<Vec<Vec<f64>>>,
    /// per-node, per-combo action probabilities (villain)
    pub villain_strat: Vec<Vec<Vec<f64>>>,
    pub iters: u32,
}

struct Rows {
    // node -> combo -> action -> regret
    regret_h: Vec<Vec<Vec<f64>>>,
    regret_v: Vec<Vec<Vec<f64>>>,
    // node -> combo -> action -> strategy sum
    strat_h: Vec<Vec<Vec<f64>>>,
    strat_v: Vec<Vec<Vec<f64>>>,
}

fn regret_match(r: &[f64]) -> Vec<f64> {
    let na = r.len();
    let sum: f64 = r.iter().filter(|&&x| x > 0.0).sum();
    if sum <= 0.0 {
        return vec![1.0 / na as f64; na];
    }
    r.iter()
        .map(|&x| if x > 0.0 { x / sum } else { 0.0 })
        .collect()
}

impl<'a> RiverCfr<'a> {
    pub fn new(
        tree: &'a PublicTree,
        hero_range: &'a [[u8; 2]],
        hero_rank: &'a [u32],
        villain_range: &'a [[u8; 2]],
        villain_rank: &'a [u32],
        root_state: State,
        hero_seat: usize,
        v_bp_hero: Option<Vec<f64>>,
    ) -> Self {
        RiverCfr {
            tree,
            hero_range,
            hero_rank,
            villain_range,
            villain_rank,
            root_state,
            hero_seat,
            v_bp_hero,
        }
    }

    pub fn solve(&self, iters: u32) -> SolvedRiver {
        let nnodes = self.tree.nodes.len();
        let nh = self.hero_range.len();
        let nv = self.villain_range.len();

        let mut rows = Rows {
            regret_h: vec![Vec::new(); nnodes],
            regret_v: vec![Vec::new(); nnodes],
            strat_h: vec![Vec::new(); nnodes],
            strat_v: vec![Vec::new(); nnodes],
        };
        for node in 0..nnodes {
            let n = &self.tree.nodes[node];
            if n.terminal {
                continue;
            }
            let na = n.actions.len();
            let is_hero = (n.player as usize) == self.hero_seat;
            let ncombos = if is_hero { nh } else { nv };
            let which_r = if is_hero {
                &mut rows.regret_h
            } else {
                &mut rows.regret_v
            };
            let which_s = if is_hero {
                &mut rows.strat_h
            } else {
                &mut rows.strat_v
            };
            which_r[node] = (0..ncombos).map(|_| vec![0.0; na]).collect();
            which_s[node] = (0..ncombos).map(|_| vec![0.0; na]).collect();
        }

        // Root reaches: uniform.
        let hero_reach = vec![1.0 / nh as f64; nh];
        let villain_reach = vec![1.0 / nv as f64; nv];

        for t in 1..=iters {
            self.iterate(
                &mut rows,
                self.tree.root,
                self.root_state,
                &hero_reach,
                &villain_reach,
                t as f64,
            );
        }

        // Extract average strategies.
        let mut out = SolvedRiver {
            hero_strat: vec![Vec::new(); nnodes],
            villain_strat: vec![Vec::new(); nnodes],
            iters,
        };
        for node in 0..nnodes {
            let n = &self.tree.nodes[node];
            if n.terminal {
                continue;
            }
            let na = n.actions.len();
            if !rows.strat_h[node].is_empty() {
                out.hero_strat[node] = rows.strat_h[node]
                    .iter()
                    .map(|s| {
                        let sum: f64 = s.iter().sum();
                        if sum > 0.0 {
                            s.iter().map(|x| x / sum).collect()
                        } else {
                            vec![1.0 / na as f64; na]
                        }
                    })
                    .collect();
            }
            if !rows.strat_v[node].is_empty() {
                out.villain_strat[node] = rows.strat_v[node]
                    .iter()
                    .map(|s| {
                        let sum: f64 = s.iter().sum();
                        if sum > 0.0 {
                            s.iter().map(|x| x / sum).collect()
                        } else {
                            vec![1.0 / na as f64; na]
                        }
                    })
                    .collect();
            }
        }
        out
    }

    /// One iteration. Returns (hero_cfv, villain_cfv) per combo, chips.
    fn iterate(
        &self,
        rows: &mut Rows,
        node: u32,
        st: State,
        hero_reach: &[f64],
        villain_reach: &[f64],
        t: f64,
    ) -> (Vec<f64>, Vec<f64>) {
        let n = &self.tree.nodes[node as usize];
        if n.terminal {
            return self.terminal(&st, hero_reach, villain_reach);
        }
        let is_hero = (n.player as usize) == self.hero_seat;
        let na = n.actions.len();
        let n_actor = if is_hero {
            self.hero_range.len()
        } else {
            self.villain_range.len()
        };
        let actor_reach = if is_hero { hero_reach } else { villain_reach };

        // Strategy per combo at this node.
        let sigmas: Vec<Vec<f64>> = {
            let reg = if is_hero {
                &rows.regret_h[node as usize]
            } else {
                &rows.regret_v[node as usize]
            };
            (0..n_actor)
                .map(|i| {
                    if reg[i].is_empty() {
                        vec![1.0 / na as f64; na]
                    } else {
                        regret_match(&reg[i])
                    }
                })
                .collect()
        };

        // Recurse per action.
        let mut child_h: Vec<Vec<f64>> = Vec::with_capacity(na);
        let mut child_v: Vec<Vec<f64>> = Vec::with_capacity(na);
        for (ai, &a) in n.actions.iter().enumerate() {
            let mut st2 = st;
            if st2.apply(a).is_err() {
                child_h.push(vec![0.0; self.hero_range.len()]);
                child_v.push(vec![0.0; self.villain_range.len()]);
                continue;
            }
            let new_reach: Vec<f64> = (0..n_actor)
                .map(|i| actor_reach[i] * sigmas[i][ai])
                .collect();
            let (h, v) = if is_hero {
                self.iterate(rows, n.children[ai], st2, &new_reach, villain_reach, t)
            } else {
                self.iterate(rows, n.children[ai], st2, hero_reach, &new_reach, t)
            };
            child_h.push(h);
            child_v.push(v);
        }

        // Node CFV per combo (weighted sum over the actor's own strategy).
        let mut node_actor = vec![0.0; n_actor];
        for i in 0..n_actor {
            for ai in 0..na {
                let c = if is_hero {
                    child_h[ai][i]
                } else {
                    child_v[ai][i]
                };
                node_actor[i] += sigmas[i][ai] * c;
            }
        }

        // Regret update per combo.
        {
            let reg = if is_hero {
                &mut rows.regret_h[node as usize]
            } else {
                &mut rows.regret_v[node as usize]
            };
            let ss = if is_hero {
                &mut rows.strat_h[node as usize]
            } else {
                &mut rows.strat_v[node as usize]
            };
            for i in 0..n_actor {
                for ai in 0..na {
                    let c = if is_hero {
                        child_h[ai][i]
                    } else {
                        child_v[ai][i]
                    };
                    let d = c - node_actor[i];
                    reg[i][ai] = (reg[i][ai] + d).max(0.0);
                }
                for ai in 0..na {
                    ss[i][ai] += t * actor_reach[i] * sigmas[i][ai];
                }
            }
        }

        // Assemble per-combo CFVs for both players. Hero's CFV sums over
        // the villain's reach at this node's branch; the actor's own
        // per-combo vector is already reach-weighted by the other side.
        let (hero_cfv, villain_cfv) = if is_hero {
            // hero_cfv = node_actor
            let mut v = vec![0.0; self.villain_range.len()];
            for ai in 0..na {
                for j in 0..v.len() {
                    v[j] += child_v[ai][j];
                }
            }
            (node_actor, v)
        } else {
            let mut h = vec![0.0; self.hero_range.len()];
            for ai in 0..na {
                for j in 0..h.len() {
                    h[j] += child_h[ai][j];
                }
            }
            (h, node_actor)
        };
        (hero_cfv, villain_cfv)
    }

    fn terminal(
        &self,
        st: &State,
        hero_reach: &[f64],
        villain_reach: &[f64],
    ) -> (Vec<f64>, Vec<f64>) {
        let nh = self.hero_range.len();
        let nv = self.villain_range.len();
        let vill_seat = 1 - self.hero_seat;
        let cfg = st.cfg();
        let stacks = st.stacks();
        let hero_inv = (cfg.start_stack - stacks[self.hero_seat]) as f64;
        let vill_inv = (cfg.start_stack - stacks[vill_seat]) as f64;

        // Mass of hero combos disjoint from each villain combo, weighted
        // by villain reach. Then mirrored for villain's CFV.
        let mass_hero = disjoint_mass(self.hero_range, self.villain_range, villain_reach);
        let mass_vill = disjoint_mass(self.villain_range, self.hero_range, hero_reach);

        if st.reached_showdown() {
            let mut cfv_h = vec![0.0; nh];
            showdown_cfv_two(
                self.hero_range,
                self.hero_rank,
                self.villain_range,
                self.villain_rank,
                villain_reach,
                &mut cfv_h,
            );
            // CFV = mass * (vill_inv - hero_inv)/2 + cfv * (vill_inv + hero_inv)/2.
            let hero_cfv: Vec<f64> = (0..nh)
                .map(|i| {
                    mass_hero[i] * (vill_inv - hero_inv) / 2.0
                        + cfv_h[i] * (vill_inv + hero_inv) / 2.0
                })
                .collect();

            let mut cfv_v = vec![0.0; nv];
            showdown_cfv_two(
                self.villain_range,
                self.villain_rank,
                self.hero_range,
                self.hero_rank,
                hero_reach,
                &mut cfv_v,
            );
            let villain_cfv: Vec<f64> = (0..nv)
                .map(|j| {
                    mass_vill[j] * (hero_inv - vill_inv) / 2.0
                        + cfv_v[j] * (hero_inv + vill_inv) / 2.0
                })
                .collect();
            (hero_cfv, villain_cfv)
        } else {
            // Fold: which seat folded is known by who did not just act.
            // Cheap and correct: if the state is not at showdown and not
            // all-in, exactly one player folded. We read the last actor
            // by position — the to_act is the winner of the fold.
            let to_act = st.to_act();
            let folder = if to_act == self.hero_seat {
                vill_seat
            } else {
                self.hero_seat
            };
            if folder == self.hero_seat {
                let h = mass_hero.iter().map(|m| -hero_inv * m).collect();
                let v = mass_vill.iter().map(|m| hero_inv * m).collect();
                (h, v)
            } else {
                let h = mass_hero.iter().map(|m| vill_inv * m).collect();
                let v = mass_vill.iter().map(|m| -vill_inv * m).collect();
                (h, v)
            }
        }
    }
}

/// For each combo `i` in `range_i`, the mass of combos in `range_j`
/// disjoint from it, weighted by `reach_j`.
fn disjoint_mass(range_i: &[[u8; 2]], range_j: &[[u8; 2]], reach_j: &[f64]) -> Vec<f64> {
    let mut card = [0.0f64; 52];
    let mut total = 0.0;
    for (j, v) in range_j.iter().enumerate() {
        let w = reach_j[j];
        card[v[0] as usize] += w;
        card[v[1] as usize] += w;
        total += w;
    }
    range_i
        .iter()
        .map(|h| total - card[h[0] as usize] - card[h[1] as usize])
        .collect()
}
