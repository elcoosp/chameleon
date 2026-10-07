//! VBR (vector best-response), river, combo-level, PATH-THREADED
//! (2026-10-07, plan W1 T1.2-T1.4).
//!
//! Exact card-perfect best response on the river. The opponent plays a
//! FIXED per-combo policy `policy(path, combo_idx) -> action probs`, keyed
//! by the public betting PATH (so a sequence-keyed blueprint works) and the
//! combo index. Hero picks the EV-max action per combo at every hero node.
//!
//! Full river tree: hero {check, bet(f), jam}; after check villain
//! {check-behind, bet(f), jam}; facing a bet the defender {fold, call,
//! raise-to-jam}; facing the raise the original bettor {fold, call}.
//! `stack` is the river effective stack; bets are pot fractions clamped.

use std::collections::HashMap;
use crate::kernel::showdown_cfv_two;

fn combo_key(a: u8, b: u8) -> u16 {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    (lo as u16) * 52 + hi as u16
}

fn disjoint_mass(hero: &[[u8; 2]], vill: &[[u8; 2]], reach: &[f64]) -> Vec<f64> {
    let total: f64 = reach.iter().sum();
    let mut card = [0.0f64; 52];
    let mut both: HashMap<u16, f64> = HashMap::new();
    for j in 0..vill.len() {
        let (a, b) = (vill[j][0], vill[j][1]);
        card[a as usize] += reach[j];
        card[b as usize] += reach[j];
        *both.entry(combo_key(a, b)).or_insert(0.0) += reach[j];
    }
    hero.iter()
        .map(|h| {
            let (a, b) = (h[0], h[1]);
            total - card[a as usize] - card[b as usize]
                + both.get(&combo_key(a, b)).copied().unwrap_or(0.0)
        })
        .collect()
}

pub struct RiverVbr<'a, F>
where
    F: FnMut(&str, usize) -> Vec<f64>,
{
    pub hero: &'a [[u8; 2]],
    pub hero_rank: &'a [u32],
    pub hero_w: &'a [f64],
    pub vill: &'a [[u8; 2]],
    pub vill_rank: &'a [u32],
    pub vill_w: &'a [f64],
    pub pot: f64,
    pub stack: f64,
    pub bet_fracs: &'a [f64],
    pub policy: F,
}

impl<'a, F> RiverVbr<'a, F>
where
    F: FnMut(&str, usize) -> Vec<f64>,
{
    fn showdown(&self, hi: f64, vi: f64, vreach: &[f64]) -> Vec<f64> {
        let nh = self.hero.len();
        let mut cfv = vec![0.0; nh];
        showdown_cfv_two(self.hero, self.hero_rank, self.vill, self.vill_rank, vreach, &mut cfv);
        let tot = disjoint_mass(self.hero, self.vill, vreach);
        let half = self.pot / 2.0;
        (0..nh)
            .map(|i| {
                let win = (cfv[i] + tot[i]) / 2.0;
                let lose = (tot[i] - cfv[i]) / 2.0;
                let tie = tot[i] - win - lose;
                (half + vi) * win - (half + hi) * lose + (vi - hi) / 2.0 * tie
            })
            .collect()
    }
    fn hero_folds(&self, hi: f64, vreach: &[f64]) -> Vec<f64> {
        let tot = disjoint_mass(self.hero, self.vill, vreach);
        let v = -(self.pot / 2.0 + hi);
        tot.iter().map(|m| v * m).collect()
    }
    fn vill_folds(&self, vi: f64, vreach: &[f64]) -> Vec<f64> {
        let tot = disjoint_mass(self.hero, self.vill, vreach);
        let v = self.pot / 2.0 + vi;
        tot.iter().map(|m| v * m).collect()
    }

    /// Hero faces a bet of `to` (villain invested `to`): max(fold, call).
    fn hero_vs_bet(&self, hi: f64, to: f64, vreach: &[f64]) -> Vec<f64> {
        let fold = self.hero_folds(hi, vreach);
        let call = self.showdown(to, to, vreach);
        (0..self.hero.len()).map(|i| fold[i].max(call[i])).collect()
    }

    /// Villain faces a hero bet of `to`, on betting path `path`.
    fn vill_vs_bet(&mut self, to: f64, path: &str, vreach: &[f64]) -> Vec<f64> {
        let nv = self.vill.len();
        let nh = self.hero.len();
        // 3 actions: fold, call, raise(=jam to stack).
        let mut rf = vec![0.0; nv];
        let mut rc = vec![0.0; nv];
        let mut rr = vec![0.0; nv];
        for j in 0..nv {
            let d = (self.policy)(path, j);
            rf[j] = vreach[j] * d.first().copied().unwrap_or(0.0);
            rc[j] = vreach[j] * d.get(1).copied().unwrap_or(0.0);
            rr[j] = vreach[j] * d.get(2).copied().unwrap_or(0.0);
        }
        let fold = self.vill_folds(0.0, &rf);
        let call = self.showdown(to, to, &rc);
        let raise = self.hero_vs_bet(to, self.stack, &rr);
        (0..nh).map(|i| fold[i] + call[i] + raise[i]).collect()
    }

    /// Villain checks-behind or bets on betting path `path` (hero checked).
    fn vill_vs_check(&mut self, path: &str, vreach: &[f64]) -> Vec<f64> {
        let nv = self.vill.len();
        let nh = self.hero.len();
        let na = self.bet_fracs.len() + 2; // check, bets..., jam
        let mut rf = vec![vec![0.0; nv]; na];
        for j in 0..nv {
            let d = (self.policy)(path, j);
            for a in 0..na {
                rf[a][j] = vreach[j] * d.get(a).copied().unwrap_or(0.0);
            }
        }
        let mut ev = self.showdown(0.0, 0.0, &rf[0]);
        for (a, f) in self.bet_fracs.iter().enumerate() {
            let bet = (f * self.pot).min(self.stack);
            let child = self.hero_vs_bet(0.0, bet, &rf[a + 1]);
            for i in 0..nh { ev[i] += child[i]; }
        }
        let jam = self.hero_vs_bet(0.0, self.stack, &rf[na - 1]);
        for i in 0..nh { ev[i] += jam[i]; }
        ev
    }

    /// Hero's exact BR value (weighted by `hero_w`).
    pub fn best_response(&mut self) -> f64 {
        let vreach = self.vill_w.to_vec();
        // Hero root: check | bet(f) | jam.
        let mut best = self.vill_vs_check("c", &vreach);
        for (a, f) in self.bet_fracs.iter().enumerate() {
            let bet = (f * self.pot).min(self.stack);
            let p = format!("b{a}");
            let ev = self.vill_vs_bet(bet, &p, &vreach);
            for i in 0..self.hero.len() { if ev[i] > best[i] { best[i] = ev[i]; } }
        }
        let jam = self.vill_vs_bet(self.stack, "j", &vreach);
        for i in 0..self.hero.len() { if jam[i] > best[i] { best[i] = jam[i]; } }
        best.iter().zip(self.hero_w.iter()).map(|(e, w)| e * w).sum()
    }
}
