//! Full-game VBR (2026-10-08, plan §8 step 2). Design:
//! docs/plans/FULLGAME-VBR-DESIGN-2026-10-08.md.
//!
//! Exact card-perfect best response over the card-independent
//! [`crate::pubtree::PublicTree`]. The sampled board is baked into the
//! initial [`State`] deck, so chance nodes are implicit: when a tree
//! action triggers a street transition, `State::apply` deals the next
//! board card from the deck prefix. Hero picks the EV-max action per
//! combo; villain reaches are split by the policy's action
//! probabilities. Terminal nodes use the O(n) kernels, so card removal
//! (shared cards between hero and villain ranges, or with the board) is
//! handled exactly.

use crate::kernel::showdown_cfv_two;
use crate::pubtree::PublicTree;
use cham_core::card::{Card, Deck};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_engine::encoder::ActionSeq;

/// Full-game best-response driver. The villain policy is queried at
/// each villain node with the action history so far, the current
/// `State` (which exposes the board and the player to act), and the
/// villain combo index. It returns the villain's action probabilities
/// at that node.
pub struct FullGameVbr<'a, F>
where
    F: FnMut(&[Action], &ActionSeq, usize) -> Vec<f64>,
{
    pub tree: &'a PublicTree,
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
    F: FnMut(&[Action], &ActionSeq, usize) -> Vec<f64>,
{
    /// Exact BR value (hero-weighted net chips), in big blinds. `board`
    /// is the fully-sampled 5-card runout; `None` when the ranges are
    /// empty or the dummy-hole allocation fails.
    pub fn best_response(&mut self, board: &[Card; 5]) -> Option<f64> {
        let nh = self.hero_range.len();
        let nv = self.villain_range.len();
        if nh == 0 || nv == 0 {
            return None;
        }
        // Dummy hero hole cards, disjoint from board and every villain
        // combo. The walk never reads hero's State hole cards — it uses
        // `hero_range` for EV — but the State still needs a valid deal.
        let mut used = [false; 52];
        for c in board {
            used[c.idx() as usize] = true;
        }
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
                if k == 2 {
                    break;
                }
            }
        }
        if k < 2 {
            return None;
        }
        // Deck order: [villain_h1, hero_h1, villain_h2, hero_h2, board...].
        // Seat 0 holds the first card of each pair; seat 1 the second.
        let prefix = [
            Card(self.villain_range[0][0]),
            Card(dummy[0]),
            Card(self.villain_range[0][1]),
            Card(dummy[1]),
            board[0],
            board[1],
            board[2],
            board[3],
            board[4],
        ];
        let st = State::new(self.cfg, Deck::with_prefix(&prefix)).ok()?;
        let seq = ActionSeq::default();
        let vr = self.villain_w.to_vec();
        let hist: Vec<Action> = Vec::new();
        let ev = walk(
            self.tree,
            self.hero_range,
            self.hero_rank,
            self.villain_range,
            self.villain_rank,
            self.cfg,
            self.hero_seat,
            &mut self.policy,
            self.tree.root,
            st,
            seq,
            &hist,
            &vr,
        );
        let bb = self.cfg.bb as f64;
        let sum: f64 = ev.iter().zip(self.hero_w.iter()).map(|(e, w)| e * w).sum();
        Some(sum / bb)
    }
}

#[allow(clippy::too_many_arguments)]
fn walk<F>(
    tree: &PublicTree,
    hero_range: &[[u8; 2]],
    hero_rank: &[u32],
    villain_range: &[[u8; 2]],
    villain_rank: &[u32],
    cfg: EngineConfig,
    hero_seat: usize,
    policy: &mut F,
    node: u32,
    mut st: State,
    seq: ActionSeq,
    history: &[Action],
    villain_reach: &[f64],
) -> Vec<f64>
where
    F: FnMut(&[Action], &ActionSeq, usize) -> Vec<f64>,
{
    let n = &tree.nodes[node as usize];
    if n.terminal {
        return terminal_ev(
            hero_range,
            hero_rank,
            villain_range,
            villain_rank,
            cfg,
            hero_seat,
            &st,
            villain_reach,
        );
    }
    let p = n.player as usize;
    let is_hero = p == hero_seat;
    let nh = hero_range.len();
    let nv = villain_range.len();

    if is_hero {
        let mut best = vec![f64::NEG_INFINITY; nh];
        for (i, &a) in n.actions.iter().enumerate() {
            let child = n.children[i];
            let mut st2 = st;
            if st2.apply(a).is_err() {
                continue;
            }
            let mut hist2 = history.to_vec();
            hist2.push(a);
            let ev = walk(
                tree,
                hero_range,
                hero_rank,
                villain_range,
                villain_rank,
                cfg,
                hero_seat,
                policy,
                child,
                st2,
                seq,
                &hist2,
                villain_reach,
            );
            for k in 0..nh {
                if ev[k] > best[k] {
                    best[k] = ev[k];
                }
            }
        }
        best
    } else {
        // Query the policy once per combo, then split the reach.
        let na = n.actions.len();
        let mut probs_per_action: Vec<Vec<f64>> =
            (0..na).map(|_| vec![0.0; nv]).collect();
        for j in 0..nv {
            let probs = policy(history, &seq, j);
            for i in 0..na {
                probs_per_action[i][j] = probs.get(i).copied().unwrap_or(0.0);
            }
        }
        let mut total = vec![0.0; nh];
        for (i, &a) in n.actions.iter().enumerate() {
            let child = n.children[i];
            let mut st2 = st;
            if st2.apply(a).is_err() {
                continue;
            }
            let mut new_reach = vec![0.0; nv];
            for j in 0..nv {
                new_reach[j] = villain_reach[j] * probs_per_action[i][j];
            }
            let mut hist2 = history.to_vec();
            hist2.push(a);
            let ev = walk(
                tree,
                hero_range,
                hero_rank,
                villain_range,
                villain_rank,
                cfg,
                hero_seat,
                policy,
                child,
                st2,
                seq,
                &hist2,
                &new_reach,
            );
            for k in 0..nh {
                total[k] += ev[k];
            }
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
) -> Vec<f64> {
    let nh = hero_range.len();
    let nv = villain_range.len();
    let vill_seat = 1 - hero_seat;
    let stacks = st.stacks();
    let hero_inv = (cfg.start_stack - stacks[hero_seat]) as f64;
    let vill_inv = (cfg.start_stack - stacks[vill_seat]) as f64;

    // Card-removal-correct mass of villain combos disjoint from hero
    // combo i. In this harness hero and villain ranges are disjoint, so
    // the both-cards correction vanishes.
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
        let overlap = card[a] + card[b];
        mass_i[i] = tot_mass - overlap;
    }

    if st.reached_showdown() {
        // Symmetric-or-not showdown EV, net of hero's and villain's
        // investments. Derivation: `EV_i = mass_i*(vill-hero)/2 +
        // cfv_i*(vill+hero)/2` where `cfv_i = win_mass - lose_mass` is
        // the O(n) kernel output. Reduces to the river VBR formula when
        // hero and villain invested equally.
        let mut cfv = vec![0.0f64; nh];
        showdown_cfv_two(
            hero_range,
            hero_rank,
            villain_range,
            villain_rank,
            villain_reach,
            &mut cfv,
        );
        (0..nh)
            .map(|i| {
                mass_i[i] * (vill_inv - hero_inv) / 2.0
                    + cfv[i] * (vill_inv + hero_inv) / 2.0
            })
            .collect()
    } else {
        // Fold terminal. The chips have already moved, so hero's stack
        // delta identifies the winner without re-reading the last action.
        let hero_net = stacks[hero_seat] - cfg.start_stack;
        if hero_net > 0 {
            mass_i.iter().map(|m| vill_inv * m).collect()
        } else {
            mass_i.iter().map(|m| -hero_inv * m).collect()
        }
    }
}
