//! Tests for `expand_classes_to_combos`: the bridge between the
//! tracker's class-level range and the combo solver.

use cham_core::card::{Card, Hand2};
use cham_search::river_cfr::expand_classes_to_combos;

fn board() -> [Card; 5] {
    [Card(40), Card(41), Card(42), Card(43), Card(44)]
}

#[test]
fn weights_sum_to_one_when_classes_sum_to_one() {
    let b = board();
    let all: Vec<[u8; 2]> = {
        let mut used = [false; 52];
        for c in &b {
            used[c.idx() as usize] = true;
        }
        let mut out = Vec::new();
        for a in 0..52u8 {
            if used[a as usize] {
                continue;
            }
            for bb in (a + 1)..52u8 {
                if used[bb as usize] {
                    continue;
                }
                out.push([a, bb]);
            }
        }
        out
    };
    let equity_of = |c: &[u8; 2]| -> f64 {
        cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &b)
    };
    // Three classes summing to 1, matching the tracker's fallback shape.
    let classes = vec![(1.0 / 3.0, 0.20), (1.0 / 3.0, 0.50), (1.0 / 3.0, 0.80)];
    let (range, w) = expand_classes_to_combos(&classes, &all, &equity_of);
    assert!(!range.is_empty());
    assert_eq!(range.len(), w.len());
    let total: f64 = w.iter().sum();
    assert!(
        (total - 1.0).abs() < 1e-9,
        "weights should sum to 1.0, got {total}"
    );
}

#[test]
fn empty_classes_yields_empty_range() {
    let eq = |_: &[u8; 2]| -> f64 { 0.5 };
    let (r, w) = expand_classes_to_combos(&[], &[[0, 1]], &eq);
    assert!(r.is_empty() && w.is_empty());
}

#[test]
fn every_combo_is_assigned_to_exactly_one_class() {
    let b = board();
    let combos: Vec<[u8; 2]> = vec![[0, 1], [2, 3], [4, 5], [6, 7]];
    let equity_of = |c: &[u8; 2]| -> f64 {
        cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &b)
    };
    let classes = vec![(0.5, 0.2), (0.5, 0.8)];
    let (range, _) = expand_classes_to_combos(&classes, &combos, &equity_of);
    assert_eq!(range.len(), combos.len());
}

#[test]
fn cap_limits_combos_per_class() {
    // 3 classes, 100 combos, cap 5 -> at most 15 combos out.
    use cham_search::river_cfr::expand_classes_to_combos_capped;
    let b = [Card(40), Card(41), Card(42), Card(43), Card(44)];
    let mut combos: Vec<[u8; 2]> = Vec::new();
    for a in 0..10u8 {
        for bb in (a + 1)..11u8 {
            combos.push([a, bb]);
            if combos.len() >= 100 {
                break;
            }
        }
        if combos.len() >= 100 {
            break;
        }
    }
    let eq = |c: &[u8; 2]| -> f64 {
        cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &b)
    };
    let classes = vec![(1.0 / 3.0, 0.2), (1.0 / 3.0, 0.5), (1.0 / 3.0, 0.8)];
    let (r, w) = expand_classes_to_combos_capped(&classes, &combos, &eq, 5);
    assert!(r.len() <= 15, "cap 5 * 3 classes gave {} combos", r.len());
    assert_eq!(r.len(), w.len());
    let total: f64 = w.iter().sum();
    assert!((total - 1.0).abs() < 1e-9);
}

#[test]
fn uncapped_matches_capped_high_limit() {
    use cham_search::river_cfr::{expand_classes_to_combos, expand_classes_to_combos_capped};
    let b = [Card(40), Card(41), Card(42), Card(43), Card(44)];
    let combos: Vec<[u8; 2]> = vec![[0, 1], [2, 3], [4, 5], [6, 7], [8, 9]];
    let eq = |c: &[u8; 2]| -> f64 {
        cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &b)
    };
    let classes = vec![(0.5, 0.3), (0.5, 0.7)];
    let (r1, w1) = expand_classes_to_combos(&classes, &combos, &eq);
    let (r2, w2) = expand_classes_to_combos_capped(&classes, &combos, &eq, usize::MAX);
    assert_eq!(r1, r2);
    assert_eq!(w1, w2);
}
