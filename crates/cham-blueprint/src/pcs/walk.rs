//! One PCS training iteration: walk the PublicTree with a fixed board,
//! accumulating regret and strategy at every infoset for BOTH players.
//!
//! Design: docs/plans/PHASE-C-PCS-DESIGN-2026-10-08.md.
//!
//! Conventions:
//! - `hero_reach[i]` and `villain_reach[j]` are joint probabilities
//!   from the root: initial range weight times the player's own strategy
//!   product along the path to this node.
//! - A node's returned `hero_cfv[i]` is `sum_j villain_reach[j] *
//!   disjoint(i,j) * ev(i,j)`, i.e. weighted by the OPPONENT's reach.
//!   Matches the fullgame VBR convention.
//! - Info sets are keyed by `Encoder::key_for(&obs.with_hole(range[i]),
//!   &seq, &slots).0`. Two combos that share a key share a regret row.
//! - Regret update (per info set): `R[a] += sum_{i in I} (child_i[a] -
//!   node_i)` where `node_i = sum_a sigma_i[a] * child_i[a]`. DCFR
//!   discounting is applied per action sign.

use crate::pcs::dcfr;
use crate::pcs::table::RegretTable;
use cham_core::card::{Card, Deck, Hand2};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::obs::{Observables, Player};
use cham_engine::encoder::{ActionSeq, Encoder};
use cham_engine::ladder::ActionLadder;
use cham_search::kernel::showdown_cfv_two;
use cham_search::pubtree::PublicTree;
use std::collections::HashMap;

pub struct PcsIteration<'a> {
    pub tree: &'a PublicTree,
    pub ladder: &'a ActionLadder,
    pub hero_range: &'a [[u8; 2]],
    pub hero_rank: &'a [u32],
    pub villain_range: &'a [[u8; 2]],
    pub villain_rank: &'a [u32],
    pub cfg: EngineConfig,
    pub hero_seat: usize,
}

struct Ctx<'b> {
    encoder: &'b mut Encoder,
    table: &'b mut RegretTable,
    pos_disc: f64,
    neg_disc: f64,
    strat_w: f64,
}

impl<'a> PcsIteration<'a> {
    /// One PCS update. `t` is the 1-based iteration counter.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &self,
        encoder: &mut Encoder,
        table: &mut RegretTable,
        board: [Card; 5],
        t: u64,
        alpha: f64,
        beta: f64,
        gamma: f64,
    ) {
        let pos_disc = dcfr::positive_discount(t, alpha);
        let neg_disc = dcfr::negative_discount(t, beta);
        let strat_w = dcfr::strategy_weight(t, gamma);

        let hero_w = vec![1.0 / self.hero_range.len() as f64; self.hero_range.len()];
        let villain_w = vec![1.0 / self.villain_range.len() as f64; self.villain_range.len()];

        // The engine `State` is only a vehicle for the walk. Nothing in
        // `walk` reads `st.hole(_)` — the hero and villain ranges are
        // held directly and `Observables::with_hole` swaps the hole at
        // key time. So the prefix can use ANY four cards disjoint from
        // the board. Using `villain_range[0]` here was a bug: when the
        // sampled board overlaps the villain's first combo, the prefix
        // had a duplicate and `Deck::with_prefix` overran its buffer.
        let mut used = [false; 52];
        for c in &board {
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
        if k < 4 {
            return;
        }

        // Prefix layout (matches `State::new`'s deal order):
        // [seat0_a, seat1_a, seat0_b, seat1_b, board...].
        let prefix = [
            Card(free[0]),
            Card(free[1]),
            Card(free[2]),
            Card(free[3]),
            board[0],
            board[1],
            board[2],
            board[3],
            board[4],
        ];
        let st = State::new(self.cfg, Deck::with_prefix(&prefix)).expect("fresh state");
        let seq = ActionSeq::default();

        let mut ctx = Ctx {
            encoder,
            table,
            pos_disc,
            neg_disc,
            strat_w,
        };
        let _ = self.walk(
            &mut ctx,
            self.tree.root,
            st,
            seq,
            &hero_w,
            &villain_w,
            None,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        &self,
        ctx: &mut Ctx<'_>,
        node: u32,
        st: State,
        seq: ActionSeq,
        hero_reach: &[f64],
        villain_reach: &[f64],
        last_action: Option<Action>,
        last_actor: Option<usize>,
    ) -> (Vec<f64>, Vec<f64>) {
        let n = &self.tree.nodes[node as usize];
        if n.terminal {
            return self.terminal(&st, hero_reach, villain_reach, last_action, last_actor);
        }

        let p = n.player as usize;
        let na = n.actions.len();
        let is_hero = p == self.hero_seat;

        let actor_range: &[[u8; 2]] = if is_hero {
            self.hero_range
        } else {
            self.villain_range
        };
        let actor_reach: &[f64] = if is_hero { hero_reach } else { villain_reach };
        let n_actor = actor_range.len();

        let obs_base = Observables::view(&st, Player::from_usize(p));
        let slots = self.ladder.slots(&obs_base, &seq);

        let mut keys: Vec<u64> = Vec::with_capacity(n_actor);
        let mut strat: Vec<Vec<f64>> = Vec::with_capacity(n_actor);
        for i in 0..n_actor {
            let hole = Hand2::new(Card(actor_range[i][0]), Card(actor_range[i][1]));
            let obs_i = obs_base.with_hole(hole);
            let k = ctx.encoder.key_for(&obs_i, &seq, &slots).0;
            keys.push(k);
            let s = ctx.table.row_mut(k, na).current_strategy();
            strat.push(s);
        }

        let mut child_h: Vec<Vec<f64>> = Vec::with_capacity(na);
        let mut child_v: Vec<Vec<f64>> = Vec::with_capacity(na);
        for (ai, &a) in n.actions.iter().enumerate() {
            let mut st2 = st;
            let mut seq2 = seq;
            cham_engine::ladder::record_action(
                self.ladder,
                &obs_base,
                Player::from_usize(p),
                a,
                &mut seq2,
            );
            if st2.apply(a).is_err() {
                child_h.push(vec![0.0; self.hero_range.len()]);
                child_v.push(vec![0.0; self.villain_range.len()]);
                continue;
            }
            let new_actor_reach: Vec<f64> = (0..n_actor)
                .map(|i| actor_reach[i] * strat[i][ai])
                .collect();
            let (h, v) = if is_hero {
                self.walk(
                    ctx,
                    n.children[ai],
                    st2,
                    seq2,
                    &new_actor_reach,
                    villain_reach,
                    Some(a),
                    Some(p),
                )
            } else {
                self.walk(
                    ctx,
                    n.children[ai],
                    st2,
                    seq2,
                    hero_reach,
                    &new_actor_reach,
                    Some(a),
                    Some(p),
                )
            };
            child_h.push(h);
            child_v.push(v);
        }

        let mut node_actor_cfv: Vec<f64> = vec![0.0; n_actor];
        for i in 0..n_actor {
            for ai in 0..na {
                let child_actor = if is_hero {
                    child_h[ai][i]
                } else {
                    child_v[ai][i]
                };
                node_actor_cfv[i] += strat[i][ai] * child_actor;
            }
        }

        let mut regret_delta: HashMap<u64, Vec<f64>> = HashMap::new();
        let mut reach_sum: HashMap<u64, f64> = HashMap::new();
        let mut sigma_agg: HashMap<u64, Vec<f64>> = HashMap::new();
        for i in 0..n_actor {
            let k = keys[i];
            let entry = regret_delta.entry(k).or_insert_with(|| vec![0.0; na]);
            for ai in 0..na {
                let child_actor = if is_hero {
                    child_h[ai][i]
                } else {
                    child_v[ai][i]
                };
                entry[ai] += child_actor - node_actor_cfv[i];
            }
            *reach_sum.entry(k).or_insert(0.0) += actor_reach[i];
            sigma_agg.entry(k).or_insert_with(|| strat[i].clone());
        }

        for (k, deltas) in &regret_delta {
            let row = ctx.table.row_mut(*k, na);
            for ai in 0..na {
                let d = deltas[ai];
                let w = if d >= 0.0 { ctx.pos_disc } else { ctx.neg_disc };
                row.regret[ai] = row.regret[ai] * w + d;
            }
            row.visits += 1;
        }
        for (k, &rsum) in &reach_sum {
            let sig = sigma_agg[k].clone();
            let row = ctx.table.row_mut(*k, na);
            for ai in 0..na {
                row.strategy_sum[ai] += ctx.strat_w * rsum * sig[ai];
            }
        }

        if is_hero {
            let mut villain_cfv = vec![0.0; self.villain_range.len()];
            for ai in 0..na {
                for j in 0..villain_cfv.len() {
                    villain_cfv[j] += child_v[ai][j];
                }
            }
            (node_actor_cfv, villain_cfv)
        } else {
            let mut hero_cfv = vec![0.0; self.hero_range.len()];
            for ai in 0..na {
                for j in 0..hero_cfv.len() {
                    hero_cfv[j] += child_h[ai][j];
                }
            }
            (hero_cfv, node_actor_cfv)
        }
    }

    fn terminal(
        &self,
        st: &State,
        hero_reach: &[f64],
        villain_reach: &[f64],
        last_action: Option<Action>,
        last_actor: Option<usize>,
    ) -> (Vec<f64>, Vec<f64>) {
        let nh = self.hero_range.len();
        let nv = self.villain_range.len();
        let vill_seat = 1 - self.hero_seat;
        let stacks = st.stacks();
        let hero_inv = (self.cfg.start_stack - stacks[self.hero_seat]) as f64;
        let vill_inv = (self.cfg.start_stack - stacks[vill_seat]) as f64;

        let mut card_h = [0.0f64; 52];
        let mut tot_hero = 0.0f64;
        for i in 0..nh {
            let w = hero_reach[i];
            card_h[self.hero_range[i][0] as usize] += w;
            card_h[self.hero_range[i][1] as usize] += w;
            tot_hero += w;
        }
        let mut mass_hero = vec![0.0f64; nh];
        for i in 0..nh {
            let a = self.hero_range[i][0] as usize;
            let b = self.hero_range[i][1] as usize;
            mass_hero[i] = tot_hero - card_h[a] - card_h[b];
        }

        let mut card_v = [0.0f64; 52];
        let mut tot_vill = 0.0f64;
        for j in 0..nv {
            let w = villain_reach[j];
            card_v[self.villain_range[j][0] as usize] += w;
            card_v[self.villain_range[j][1] as usize] += w;
            tot_vill += w;
        }
        let mut mass_vill = vec![0.0f64; nv];
        for j in 0..nv {
            let a = self.villain_range[j][0] as usize;
            let b = self.villain_range[j][1] as usize;
            mass_vill[j] = tot_vill - card_v[a] - card_v[b];
        }

        if st.reached_showdown() {
            let mut cfv_h = vec![0.0f64; nh];
            showdown_cfv_two(
                self.hero_range,
                self.hero_rank,
                self.villain_range,
                self.villain_rank,
                villain_reach,
                &mut cfv_h,
            );
            let hero_cfv: Vec<f64> = (0..nh)
                .map(|i| {
                    mass_hero[i] * (vill_inv - hero_inv) / 2.0
                        + cfv_h[i] * (vill_inv + hero_inv) / 2.0
                })
                .collect();

            let mut cfv_v = vec![0.0f64; nv];
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
            let folder = match (last_action, last_actor) {
                (Some(Action::Fold), Some(a)) => a,
                _ => {
                    let to_act = st.to_act();
                    if to_act == self.hero_seat {
                        vill_seat
                    } else {
                        self.hero_seat
                    }
                }
            };
            if folder == self.hero_seat {
                let hero_cfv: Vec<f64> = mass_hero.iter().map(|m| -hero_inv * m).collect();
                let villain_cfv: Vec<f64> = mass_vill.iter().map(|m| hero_inv * m).collect();
                (hero_cfv, villain_cfv)
            } else {
                let hero_cfv: Vec<f64> = mass_hero.iter().map(|m| vill_inv * m).collect();
                let villain_cfv: Vec<f64> = mass_vill.iter().map(|m| -vill_inv * m).collect();
                (hero_cfv, villain_cfv)
            }
        }
    }
}
