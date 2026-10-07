//! VBR (vector best-response), river, combo-level (2026-10-07).
//! The plan's exact ruler, river first: hero pure-bets, villain best-responds
//! (calls iff call-EV > fold-EV = 0). The showdown sums use the O(n) kernels;
//! validated against an O(n*m) brute force. This is T1.2/T1.3.

use cham_search::kernel::showdown_cfv_two;

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

fn hero_ev_brute(
    hero: &[[u8; 2]],
    hr: &[u32],
    hw: &[f64],
    vill: &[[u8; 2]],
    vr: &[u32],
    vw: &[f64],
    pot: f64,
    bet: f64,
) -> f64 {
    let mut call = vec![false; vill.len()];
    for j in 0..vill.len() {
        let mut s = 0.0;
        for i in 0..hero.len() {
            if disjoint(hero[i], vill[j]) {
                s += hw[i] * sgn(vr[j], hr[i]);
            }
        }
        call[j] = s * (pot / 2.0 + bet) > 0.0;
    }
    let mut ev = 0.0;
    for i in 0..hero.len() {
        let mut acc = 0.0;
        for j in 0..vill.len() {
            if !disjoint(hero[i], vill[j]) {
                continue;
            }
            acc += if call[j] {
                vw[j] * (pot / 2.0 + bet) * sgn(hr[i], vr[j])
            } else {
                vw[j] * (pot / 2.0)
            };
        }
        ev += hw[i] * acc;
    }
    ev
}

fn hero_ev_kernel(
    hero: &[[u8; 2]],
    hr: &[u32],
    hw: &[f64],
    vill: &[[u8; 2]],
    vr: &[u32],
    vw: &[f64],
    pot: f64,
    bet: f64,
) -> f64 {
    let nh = hero.len();
    let nv = vill.len();
    // villain call sign-sum per villain combo (kernel: swap roles).
    let mut vsign = vec![0.0f64; nv];
    showdown_cfv_two(vill, vr, hero, hr, hw, &mut vsign);
    let masked: Vec<f64> = (0..nv)
        .map(|j| {
            if vsign[j] * (pot / 2.0 + bet) > 0.0 {
                vw[j]
            } else {
                0.0
            }
        })
        .collect();
    // hero showdown vs the CALLING villain range (kernel).
    let mut hsd = vec![0.0f64; nh];
    showdown_cfv_two(hero, hr, vill, vr, &masked, &mut hsd);
    // hero fold mass per combo (villains NOT calling, disjoint).
    let mut ev = 0.0;
    for i in 0..nh {
        let mut fm = 0.0;
        for j in 0..nv {
            if !disjoint(hero[i], vill[j]) {
                continue;
            }
            if vsign[j] * (pot / 2.0 + bet) <= 0.0 {
                fm += vw[j];
            }
        }
        ev += hw[i] * (hsd[i] * (pot / 2.0 + bet) + fm * (pot / 2.0));
    }
    ev
}

#[test]
fn vbr_river_kernel_matches_brute() {
    let hero: [[u8; 2]; 4] = [[5, 6], [7, 8], [9, 10], [11, 12]];
    let vill: [[u8; 2]; 4] = [[13, 14], [15, 16], [17, 18], [19, 20]];
    let hr = [5u32, 3, 8, 1];
    let vr = [4u32, 7, 2, 6];
    let hw = [0.7f64, 1.0, 0.4, 1.3];
    let vw = [1.1f64, 0.6, 0.9, 0.3];
    let (pot, bet) = (20.0, 10.0);
    let b = hero_ev_brute(&hero, &hr, &hw, &vill, &vr, &vw, pot, bet);
    let k = hero_ev_kernel(&hero, &hr, &hw, &vill, &vr, &vw, pot, bet);
    assert!((b - k).abs() < 1e-9, "brute {b} vs kernel {k}");
}

#[test]
fn vbr_river_symmetric_pure_bet_nonneg_for_bettor() {
    // Hero with strictly stronger ranks should have positive bet EV.
    let hero: [[u8; 2]; 2] = [[5, 6], [7, 8]];
    let vill: [[u8; 2]; 2] = [[13, 14], [15, 16]];
    let hr = [10u32, 9];
    let vr = [2u32, 1];
    let hw = [1.0f64, 1.0];
    let vw = [1.0f64, 1.0];
    let k = hero_ev_kernel(&hero, &hr, &hw, &vill, &vr, &vw, 20.0, 10.0);
    assert!(k > 0.0, "stronger hero betting should be +EV, got {k}");
}
