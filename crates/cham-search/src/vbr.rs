//! VBR (vector best-response), river (2026-10-07, plan W1 T1.2/T1.3).
//!
//! Exact card-perfect best response on the river, combo-level, using the O(n)
//! kernels. The opponent plays a FIXED per-combo policy; hero picks the
//! per-combo EV-max action at every hero node. This is the honest ruler:
//! it sees exact hole cards and card removal, unlike the abstraction BR.

use crate::kernel::showdown_cfv_two;
use std::collections::HashMap;

fn combo_key(a: u8, b: u8) -> u16 {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    (lo as u16) * 52 + hi as u16
}

/// Sum of `reach` over villain combos card-disjoint from each hero combo (O(n)).
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
        showdown_cfv_two(
            self.hero,
            self.hero_rank,
            self.vill,
            self.vill_rank,
            vreach,
            &mut cfv,
        );
        let tot = disjoint_mass(self.hero, self.vill, vreach);
        let half = self.pot / 2.0;
        (0..nh)
            .map(|i| {
                let w = (cfv[i] + tot[i]) / 2.0;
                let l = (tot[i] - cfv[i]) / 2.0;
                let t = tot[i] - w - l;
                (half + vi) * w - (half + hi) * l + (vi - hi) / 2.0 * t
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

    fn hero_facing_bet(&self, hi: f64, vi: f64, vreach: &[f64]) -> Vec<f64> {
        let fold = self.hero_folds(hi, vreach);
        let call = self.showdown(vi, vi, vreach);
        (0..self.hero.len()).map(|i| fold[i].max(call[i])).collect()
    }

    fn vill_facing_bet(&mut self, hi: f64, vi: f64, vreach: &[f64]) -> Vec<f64> {
        let nv = self.vill.len();
        let nh = self.hero.len();
        let mut rfa = vec![vec![0.0; nv]; 3];
        for j in 0..nv {
            let d = (self.policy)("vb", j);
            for (a, slot) in rfa.iter_mut().enumerate() {
                slot[j] = vreach[j] * d.get(a).copied().unwrap_or(0.0);
            }
        }
        let fold = self.vill_folds(vi, &rfa[0]);
        let call = self.showdown(hi, hi, &rfa[1]);
        let jam = self.hero_facing_bet(hi, self.stack, &rfa[2]);
        (0..nh).map(|i| fold[i] + call[i] + jam[i]).collect()
    }

    fn vill_facing_check(&mut self, hi: f64, vi: f64, vreach: &[f64]) -> Vec<f64> {
        let nv = self.vill.len();
        let nh = self.hero.len();
        let na = self.bet_fracs.len() + 2;
        let mut rfa = vec![vec![0.0; nv]; na];
        for j in 0..nv {
            let d = (self.policy)("vc", j);
            for (a, slot) in rfa.iter_mut().enumerate() {
                slot[j] = vreach[j] * d.get(a).copied().unwrap_or(0.0);
            }
        }
        let mut ev = self.showdown(hi, vi, &rfa[0]);
        for (a, f) in self.bet_fracs.iter().enumerate() {
            let bet = (f * self.pot).min(self.stack);
            let child = self.hero_facing_bet(hi, vi + bet, &rfa[a + 1]);
            for i in 0..nh {
                ev[i] += child[i];
            }
        }
        let jam = self.hero_facing_bet(hi, self.stack, &rfa[na - 1]);
        for i in 0..nh {
            ev[i] += jam[i];
        }
        ev
    }

    /// Hero's exact BR value (weighted by `hero_w`) against the fixed policy.
    pub fn best_response(&mut self) -> f64 {
        let vw = self.vill_w.to_vec();
        let mut best = self.vill_facing_check(0.0, 0.0, &vw);
        for f in self.bet_fracs.to_vec() {
            let bet = (f * self.pot).min(self.stack);
            let ev = self.vill_facing_bet(bet, 0.0, &vw);
            for i in 0..self.hero.len() {
                if ev[i] > best[i] {
                    best[i] = ev[i];
                }
            }
        }
        let jam = self.vill_facing_bet(self.stack, 0.0, &vw);
        for i in 0..self.hero.len() {
            if jam[i] > best[i] {
                best[i] = jam[i];
            }
        }
        best.iter()
            .zip(self.hero_w.iter())
            .map(|(e, w)| e * w)
            .sum()
    }
}
