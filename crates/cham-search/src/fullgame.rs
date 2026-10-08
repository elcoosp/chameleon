//! Full-game VBR. Design: docs/plans/FULLGAME-VBR-DESIGN-2026-10-08.md.
//!
//! Policy contract: `FnMut(&State, &ActionSeq, usize /*na*/, usize /*combo*/)
//! -> Vec<f64>`. The state and seq at the villain node are exactly what
//! `BlueprintPolicy::strategy` needs (`Observables::view(&state, ...)` plus
//! the encoded sequence). Return exactly `na` probs.

use crate::kernel::showdown_cfv_two;
use crate::pubtree::PublicTree;
use cham_core::card::{Card, Deck};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::obs::{Observables, Player};
use cham_engine::encoder::ActionSeq;
use cham_engine::ladder::ActionLadder;

pub struct FullGameVbr<'a, F>
where
    F: FnMut(&State, &[Action], &ActionSeq, usize, usize) -> Vec<f64>,
{
    pub tree: &'a PublicTree,
    pub ladder: &'a ActionLadder,
    pub hero_range: &'a [[u8; 2]],
    pub hero_rank: &'a [u32],
    pub hero_w: &'a [f64],
    pub villain_range: &'a [[u8; 2]],
    pub villain_rank: &'a [u32],
    pub villain_w: &'a [f64],
    pub cfg: EngineConfig,
    pub hero_seat: usize,
    pub policy: F,
}

impl<'a, F> FullGameVbr<'a, F>
where
    F: FnMut(&State, &[Action], &ActionSeq, usize, usize) -> Vec<f64>,
{
    pub fn best_response(&mut self, board: &[Card; 5]) -> Option<f64> {
        let nh = self.hero_range.len();
        let nv = self.villain_range.len();
        if nh == 0 || nv == 0 { return None; }
        let mut used = [false; 52];
        for c in board { used[c.idx() as usize] = true; }
        for v in self.villain_range {
            used[v[0] as usize] = true;
            used[v[1] as usize] = true;
        }
        let mut dummy = [0u8; 2];
        let mut k = 0usize;
        for c in 0..52u8 {
            if !used[c as usize] {
                dummy[k] = c;
                k += 1;
                if k == 2 { break; }
            }
        }
        if k < 2 { return None; }
        let prefix = [
            Card(self.villain_range[0][0]), Card(dummy[0]),
            Card(self.villain_range[0][1]), Card(dummy[1]),
            board[0], board[1], board[2], board[3], board[4],
        ];
        let st = State::new(self.cfg, Deck::with_prefix(&prefix)).ok()?;
        let vr = self.villain_w.to_vec();
        let seq = ActionSeq::default();
        let hist: Vec<Action> = Vec::new();
        let ev = walk(
            self.tree, self.ladder,
            self.hero_range, self.hero_rank,
            self.villain_range, self.villain_rank,
            self.cfg, self.hero_seat, &mut self.policy,
            self.tree.root, st, seq, &hist, &vr, None, None,
        );
        let bb = self.cfg.bb as f64;
        let sum: f64 = ev.iter().zip(self.hero_w.iter()).map(|(e, w)| e * w).sum();
        Some(sum / bb)
    }
}

#[allow(clippy::too_many_arguments)]
fn walk<F>(
    tree: &PublicTree,
    ladder: &ActionLadder,
    hero_range: &[[u8; 2]],
    hero_rank: &[u32],
    villain_range: &[[u8; 2]],
    villain_rank: &[u32],
    cfg: EngineConfig,
    hero_seat: usize,
    policy: &mut F,
    node: u32,
    st: State,
    seq: ActionSeq,
    history: &[Action],
    villain_reach: &[f64],
    last_action: Option<Action>,
    last_actor: Option<usize>,
) -> Vec<f64>
where
    F: FnMut(&State, &[Action], &ActionSeq, usize, usize) -> Vec<f64>,
{
    let n = &tree.nodes[node as usize];
    if n.terminal {
        return terminal_ev(
            hero_range, hero_rank, villain_range, villain_rank,
            cfg, hero_seat, &st, villain_reach, last_action, last_actor,
        );
    }
    let p = n.player as usize;
    let is_hero = p == hero_seat;
    let nh = hero_range.len();
    let nv = villain_range.len();

    if is_hero {
        let mut best = vec![f64::NEG_INFINITY; nh];
        for (i, &a) in n.actions.iter().enumerate() {
            let mut st2 = st;
            let obs = Observables::view(&st2, Player::from_usize(p));
            let mut seq2 = seq;
            cham_engine::ladder::record_action(
                ladder, &obs, Player::from_usize(p), a, &mut seq2,
            );
            if st2.apply(a).is_err() { continue; }
            let mut hist2 = history.to_vec();
            hist2.push(a);
            let ev = walk(
                tree, ladder, hero_range, hero_rank, villain_range, villain_rank,
                cfg, hero_seat, policy, n.children[i], st2, seq2, &hist2, villain_reach,
                Some(a), Some(p),
            );
            for k in 0..nh {
                if ev[k] > best[k] { best[k] = ev[k]; }
            }
        }
        best
    } else {
        let na = n.actions.len();
        let mut probs_per_action: Vec<Vec<f64>> =
            (0..na).map(|_| vec![0.0; nv]).collect();
        for j in 0..nv {
            let probs = policy(&st, history, &seq, na, j);
            for i in 0..na {
                probs_per_action[i][j] = probs.get(i).copied().unwrap_or(0.0);
            }
        }
        let mut total = vec![0.0; nh];
        for (i, &a) in n.actions.iter().enumerate() {
            let mut st2 = st;
            let obs = Observables::view(&st2, Player::from_usize(p));
            let mut seq2 = seq;
            cham_engine::ladder::record_action(
                ladder, &obs, Player::from_usize(p), a, &mut seq2,
            );
            if st2.apply(a).is_err() { continue; }
            let mut new_reach = vec![0.0; nv];
            for j in 0..nv {
                new_reach[j] = villain_reach[j] * probs_per_action[i][j];
            }
            let mut hist2 = history.to_vec();
            hist2.push(a);
            let ev = walk(
                tree, ladder, hero_range, hero_rank, villain_range, villain_rank,
                cfg, hero_seat, policy, n.children[i], st2, seq2, &hist2, &new_reach,
                Some(a), Some(p),
            );
            for k in 0..nh { total[k] += ev[k]; }
        }
        total
    }
}

#[allow(clippy::too_many_arguments)]
fn terminal_ev(
    hero_range: &[[u8; 2]],
    hero_rank: &[u32],
    villain_range: &[[u8; 2]],
    villain_rank: &[u32],
    cfg: EngineConfig,
    hero_seat: usize,
    st: &State,
    villain_reach: &[f64],
    last_action: Option<Action>,
    last_actor: Option<usize>,
) -> Vec<f64> {
    let nh = hero_range.len();
    let nv = villain_range.len();
    let vill_seat = 1 - hero_seat;
    let stacks = st.stacks();
    let hero_inv = (cfg.start_stack - stacks[hero_seat]) as f64;
    let vill_inv = (cfg.start_stack - stacks[vill_seat]) as f64;

    let mut card = [0.0f64; 52];
    let mut tot_mass = 0.0f64;
    for j in 0..nv {
        let w = villain_reach[j];
        card[villain_range[j][0] as usize] += w;
        card[villain_range[j][1] as usize] += w;
        tot_mass += w;
    }
    let mut mass_i = vec![0.0f64; nh];
    for i in 0..nh {
        let a = hero_range[i][0] as usize;
        let b = hero_range[i][1] as usize;
        mass_i[i] = tot_mass - card[a] - card[b];
    }

    if st.reached_showdown() {
        let mut cfv = vec![0.0f64; nh];
        showdown_cfv_two(
            hero_range, hero_rank, villain_range, villain_rank,
            villain_reach, &mut cfv,
        );
        (0..nh)
            .map(|i| {
                mass_i[i] * (vill_inv - hero_inv) / 2.0
                    + cfv[i] * (vill_inv + hero_inv) / 2.0
            })
            .collect()
    } else {
        let folder = match (last_action, last_actor) {
            (Some(Action::Fold), Some(a)) => a,
            _ => {
                let to_act = st.to_act();
                if to_act == hero_seat { vill_seat } else { hero_seat }
            }
        };
        if folder == hero_seat {
            mass_i.iter().map(|m| -hero_inv * m).collect()
        } else {
            mass_i.iter().map(|m| vill_inv * m).collect()
        }
    }
}
