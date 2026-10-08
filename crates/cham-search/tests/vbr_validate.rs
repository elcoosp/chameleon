//! VBR validation (plan W1 T1.3): the kernel VBR must match a naive
//! O(n*m) RECURSIVE best response over the SAME river tree.

use cham_search::vbr::RiverVbr;

fn disjoint(a: [u8; 2], b: [u8; 2]) -> bool {
    a[0] != b[0] && a[0] != b[1] && a[1] != b[0] && a[1] != b[1]
}
fn sgn(a: u32, b: u32) -> f64 {
    if a > b {
        1.0
    } else if a < b {
        -1.0
    } else {
        0.0
    }
}

struct Brute<'a> {
    hero: &'a [[u8; 2]],
    hr: &'a [u32],
    hw: &'a [f64],
    vill: &'a [[u8; 2]],
    vr: &'a [u32],
    vw: &'a [f64],
    pot: f64,
    stack: f64,
    fracs: &'a [f64],
    vpolicy: fn(&str, usize) -> Vec<f64>,
}

impl<'a> Brute<'a> {
    fn showdown(&self, hi: f64, vi: f64, vreach: &[f64]) -> Vec<f64> {
        let half = self.pot / 2.0;
        (0..self.hero.len())
            .map(|i| {
                let mut acc = 0.0;
                for j in 0..self.vill.len() {
                    if !disjoint(self.hero[i], self.vill[j]) {
                        continue;
                    }
                    let s = sgn(self.hr[i], self.vr[j]);
                    acc += vreach[j] * (s * (half + vi) + (1.0 - s.abs()) * (vi - hi) / 2.0);
                    // s=+1: +half+vi ; s=-1: -(half+hi) ; s=0: (vi-hi)/2
                    // fix: encode directly
                }
                acc
            })
            .collect::<Vec<_>>()
            .into_iter()
            .enumerate()
            .map(|(i, _)| {
                let mut acc = 0.0;
                for j in 0..self.vill.len() {
                    if !disjoint(self.hero[i], self.vill[j]) {
                        continue;
                    }
                    let s = sgn(self.hr[i], self.vr[j]);
                    acc += vreach[j]
                        * if s > 0.0 {
                            half + vi
                        } else if s < 0.0 {
                            -(half + hi)
                        } else {
                            (vi - hi) / 2.0
                        };
                }
                acc
            })
            .collect()
    }
    fn hero_folds(&self, hi: f64, vreach: &[f64]) -> Vec<f64> {
        (0..self.hero.len())
            .map(|i| {
                let mut m = 0.0;
                for j in 0..self.vill.len() {
                    if disjoint(self.hero[i], self.vill[j]) {
                        m += vreach[j];
                    }
                }
                -(self.pot / 2.0 + hi) * m
            })
            .collect()
    }
    fn vill_folds(&self, vi: f64, vreach: &[f64]) -> Vec<f64> {
        (0..self.hero.len())
            .map(|i| {
                let mut m = 0.0;
                for j in 0..self.vill.len() {
                    if disjoint(self.hero[i], self.vill[j]) {
                        m += vreach[j];
                    }
                }
                (self.pot / 2.0 + vi) * m
            })
            .collect()
    }
    fn hero_facing_bet(&self, hi: f64, vi: f64, vreach: &[f64]) -> Vec<f64> {
        let f = self.hero_folds(hi, vreach);
        let c = self.showdown(vi, vi, vreach);
        (0..self.hero.len()).map(|i| f[i].max(c[i])).collect()
    }
    fn vill_facing_bet(&self, hi: f64, vreach: &[f64]) -> Vec<f64> {
        let nv = self.vill.len();
        let mut rfa = vec![vec![0.0; nv]; 3];
        for j in 0..nv {
            let d = (self.vpolicy)("vb", j);
            for a in 0..3 {
                rfa[a][j] = vreach[j] * d.get(a).copied().unwrap_or(0.0);
            }
        }
        let fold = self.vill_folds(0.0, &rfa[0]);
        let call = self.showdown(hi, hi, &rfa[1]);
        let jam = self.hero_facing_bet(hi, self.stack, &rfa[2]);
        (0..self.hero.len())
            .map(|i| fold[i] + call[i] + jam[i])
            .collect()
    }
    fn vill_facing_check(&self, vreach: &[f64]) -> Vec<f64> {
        let nv = self.vill.len();
        let na = self.fracs.len() + 2;
        let mut rfa = vec![vec![0.0; nv]; na];
        for j in 0..nv {
            let d = (self.vpolicy)("vc", j);
            for a in 0..na {
                rfa[a][j] = vreach[j] * d.get(a).copied().unwrap_or(0.0);
            }
        }
        let mut ev = self.showdown(0.0, 0.0, &rfa[0]);
        for (a, f) in self.fracs.iter().enumerate() {
            let bet = (f * self.pot).min(self.stack);
            let child = self.hero_facing_bet(0.0, bet, &rfa[a + 1]);
            for i in 0..self.hero.len() {
                ev[i] += child[i];
            }
        }
        let jam = self.hero_facing_bet(0.0, self.stack, &rfa[na - 1]);
        for i in 0..self.hero.len() {
            ev[i] += jam[i];
        }
        ev
    }
    fn br(&self) -> f64 {
        let vreach = self.vw.to_vec();
        let mut best = self.vill_facing_check(&vreach);
        for f in self.fracs {
            let bet = (f * self.pot).min(self.stack);
            let ev = self.vill_facing_bet(bet, &vreach);
            for i in 0..self.hero.len() {
                if ev[i] > best[i] {
                    best[i] = ev[i];
                }
            }
        }
        let jam = self.vill_facing_bet(self.stack, &vreach);
        for i in 0..self.hero.len() {
            if jam[i] > best[i] {
                best[i] = jam[i];
            }
        }
        best.iter().zip(self.hw).map(|(e, w)| e * w).sum()
    }
}

fn unif(_: &str, _: usize) -> Vec<f64> {
    vec![0.25; 4]
}

#[test]
fn vbr_matches_recursive_brute() {
    let hero: [[u8; 2]; 3] = [[0, 1], [2, 3], [4, 5]];
    let vill: [[u8; 2]; 3] = [[6, 7], [8, 9], [10, 11]];
    let hr = [9u32, 5, 2];
    let vr = [7u32, 4, 1];
    let hw = [1.0f64, 1.0, 1.0];
    let vw = [1.0f64, 1.0, 1.0];
    let (pot, stack) = (20.0, 100.0);
    let fracs = [0.5f64, 1.0];

    let mut vbr = RiverVbr {
        hero: &hero,
        hero_rank: &hr,
        hero_w: &hw,
        vill: &vill,
        vill_rank: &vr,
        vill_w: &vw,
        pot,
        stack,
        bet_fracs: &fracs,
        policy: unif,
    };
    let k = vbr.best_response();

    let bt = Brute {
        hero: &hero,
        hr: &hr,
        hw: &hw,
        vill: &vill,
        vr: &vr,
        vw: &vw,
        pot,
        stack,
        fracs: &fracs,
        vpolicy: unif,
    };
    let b = bt.br();

    assert!((k - b).abs() < 1e-6, "VBR {k} vs brute {b}");
}
