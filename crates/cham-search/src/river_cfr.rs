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
    pub hero_strat: Vec<Vec<Vec<f64>>>,
    pub villain_strat: Vec<Vec<Vec<f64>>>,
    /// With the gadget: villain's virtual-root strategy per combo,
    /// `[P(terminate), P(play)]`. `None` without a gadget.
    pub gadget_root_strat: Option<Vec<[f64; 2]>>,
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

        // Safe-resolving gadget. When `v_bp_hero` is Some, a virtual
        // root gives the villain [terminate | play]. Terminate pays the
        // hero `v_bp_hero[j]` (so villain gets `-v_bp_hero[j]`). Play
        // enters the tree.
        let gadget = self.v_bp_hero.clone();
        let mut gadget_root_regret: Vec<[f64; 2]> = vec![[0.0; 2]; nv];
        let mut gadget_root_strat: Vec<[f64; 2]> = vec![[0.0; 2]; nv];

        for t in 1..=iters {
            let root_sigmas: Vec<[f64; 2]> = if gadget.is_some() {
                gadget_root_regret
                    .iter()
                    .map(|r| {
                        let sum: f64 = r.iter().filter(|&&x| x > 0.0).sum();
                        if sum <= 0.0 {
                            [0.5, 0.5]
                        } else {
                            [r[0].max(0.0) / sum, r[1].max(0.0) / sum]
                        }
                    })
                    .collect()
            } else {
                vec![[0.5, 0.5]; nv]
            };

            let villain_reach: Vec<f64> = (0..nv)
                .map(|j| (1.0 / nv as f64) * root_sigmas[j][1])
                .collect();

            let (_hero_cfv, villain_cfv) = self.iterate(
                &mut rows,
                self.tree.root,
                self.root_state,
                &hero_reach,
                &villain_reach,
                t as f64,
            );

            if let Some(ref vbp) = gadget {
                let mut node_v = vec![0.0; nv];
                for j in 0..nv {
                    let tv = -vbp[j];
                    let pv = villain_cfv[j];
                    node_v[j] = root_sigmas[j][0] * tv + root_sigmas[j][1] * pv;
                }
                let init_reach = 1.0 / nv as f64;
                for j in 0..nv {
                    let tv = -vbp[j];
                    let pv = villain_cfv[j];
                    gadget_root_regret[j][0] = (gadget_root_regret[j][0] + tv - node_v[j]).max(0.0);
                    gadget_root_regret[j][1] = (gadget_root_regret[j][1] + pv - node_v[j]).max(0.0);
                    gadget_root_strat[j][0] += t as f64 * init_reach * root_sigmas[j][0];
                    gadget_root_strat[j][1] += t as f64 * init_reach * root_sigmas[j][1];
                }
            }
        }

        // Extract average strategies.
        let mut out = SolvedRiver {
            hero_strat: vec![Vec::new(); nnodes],
            villain_strat: vec![Vec::new(); nnodes],
            gadget_root_strat: if gadget.is_some() {
                Some(
                    gadget_root_strat
                        .iter()
                        .map(|s| {
                            let sum: f64 = s.iter().sum();
                            if sum > 0.0 {
                                [s[0] / sum, s[1] / sum]
                            } else {
                                [0.5, 0.5]
                            }
                        })
                        .collect(),
                )
            } else {
                None
            },
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

    /// Hero's best-response value against the villain's average
    /// strategy from `solved`. Positive = hero chips.
    ///
    /// At hero nodes: max over actions. At villain nodes: follow the
    /// villain strategy vector. Returns the value in chips, weighted by
    /// the hero's initial range weight (uniform, matching `solve`).
    pub fn br_hero(&self, solved: &SolvedRiver) -> f64 {
        let nh = self.hero_range.len();
        let nv = self.villain_range.len();
        let hero_reach = vec![1.0 / nh as f64; nh];
        let villain_reach = vec![1.0 / nv as f64; nv];
        let cfvs = self.br_walk(self.tree.root, self.root_state, &villain_reach, solved);
        cfvs.iter().zip(hero_reach.iter()).map(|(c, w)| c * w).sum()
    }

    /// Two-seat exploitability: `BR_hero(villain_strat) + BR_villain(hero_strat)`.
    /// At Nash this is 0; positive means the average strategy is exploitable.
    /// Returns chips.
    ///
    /// The villain's BR is computed by mirroring the seat roles: construct
    /// a swapped `RiverCfr` (hero<->villain) and call `br_hero` on the
    /// hero strategy it was given. The sign convention matches `br_hero`:
    /// the swapped call returns the villain's value in the original game.
    pub fn exploitability(&self, solved: &SolvedRiver) -> f64 {
        let br_hero = self.br_hero(solved);

        // Mirror: seat roles swapped. hero_seat = 1 - self.hero_seat,
        // ranges swapped, ranks swapped. The gadget (if any) is dropped —
        // the mirrored call only measures BR against the hero's strategy
        // inside the tree, it does not re-solve.
        let mirror = RiverCfr {
            tree: self.tree,
            hero_range: self.villain_range,
            hero_rank: self.villain_rank,
            villain_range: self.hero_range,
            villain_rank: self.hero_rank,
            root_state: self.root_state,
            hero_seat: 1 - self.hero_seat,
            v_bp_hero: None,
        };
        // Swap the solved strategy: villain's view becomes the mirror's
        // hero's view.
        let mirrored = SolvedRiver {
            hero_strat: solved.villain_strat.clone(),
            villain_strat: solved.hero_strat.clone(),
            gadget_root_strat: None,
            iters: solved.iters,
        };
        let br_villain = mirror.br_hero(&mirrored);
        br_hero + br_villain
    }

    fn br_walk(
        &self,
        node: u32,
        st: State,
        villain_reach: &[f64],
        solved: &SolvedRiver,
    ) -> Vec<f64> {
        let n = &self.tree.nodes[node as usize];
        if n.terminal {
            // Hero CFV only; the villain-side vector is computed but
            // ignored (BR is from hero's perspective).
            let zero_hero = vec![0.0; self.hero_range.len()];
            let (h, _v) = self.terminal(&st, &zero_hero, villain_reach);
            return h;
        }
        let is_hero = (n.player as usize) == self.hero_seat;
        let nh = self.hero_range.len();
        let nv = self.villain_range.len();
        if is_hero {
            let mut best: Option<Vec<f64>> = None;
            for (ai, &a) in n.actions.iter().enumerate() {
                let mut st2 = st;
                if st2.apply(a).is_err() {
                    continue;
                }
                let h = self.br_walk(n.children[ai], st2, villain_reach, solved);
                best = Some(match best {
                    None => h,
                    Some(b) => b.iter().zip(h.iter()).map(|(x, y)| x.max(*y)).collect(),
                });
            }
            best.unwrap_or_else(|| vec![f64::NEG_INFINITY; nh])
        } else {
            let strat = &solved.villain_strat[node as usize];
            let mut total = vec![0.0; nh];
            for (ai, &a) in n.actions.iter().enumerate() {
                let mut st2 = st;
                if st2.apply(a).is_err() {
                    continue;
                }
                let new_reach: Vec<f64> =
                    (0..nv).map(|j| villain_reach[j] * strat[j][ai]).collect();
                let h = self.br_walk(n.children[ai], st2, &new_reach, solved);
                for i in 0..nh {
                    total[i] += h[i];
                }
            }
            total
        }
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
