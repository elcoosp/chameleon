//! Router metrics (SPECS/05 §6): top-1, per-class recall, ECE, confusion — all
//! family-aware. The heavy lifting lives in `train.rs`; this module exposes the
//! standalone evaluation used by `chameleon probe`.

use crate::dataset::{RbinRow, SESSION_BTEST};
use crate::model::SoftmaxModel;

/// Evaluate a model on rows: (top1, per-class recall, ECE, confusion matrix).
pub fn evaluate(model: &SoftmaxModel, rows: &[RbinRow]) -> (f64, [f64; 4], f64, [[u64; 4]; 4]) {
    let refs: Vec<&RbinRow> = rows.iter().collect();
    let top1 = top1(model, &refs);
    let recall = recall(model, &refs);
    let ece = ece(model, &refs);
    let mut conf = [[0u64; 4]; 4];
    for r in rows {
        let p = model.forward(&r.features);
        let pred = argmax(&p);
        conf[r.label as usize][pred] += 1;
    }
    (top1, recall, ece, conf)
}

/// B-test-only evaluation convenience (the headline split).
pub fn evaluate_b_test(model: &SoftmaxModel, rows: &[RbinRow]) -> (f64, f64, [f64; 4]) {
    let btest: Vec<&RbinRow> = rows
        .iter()
        .filter(|r| crate::dataset::split_of_session(r.session_id) == SESSION_BTEST)
        .collect();
    (top1(model, &btest), ece(model, &btest), recall(model, &btest))
}

fn argmax(p: &[f64]) -> usize {
    let mut best = 0usize;
    for (i, &v) in p.iter().enumerate() {
        if v > p[best] {
            best = i;
        }
    }
    best
}

fn top1(model: &SoftmaxModel, rows: &[&RbinRow]) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    let hits = rows
        .iter()
        .filter(|r| argmax(&model.forward(&r.features)) == r.label as usize)
        .count();
    hits as f64 / rows.len() as f64
}

fn recall(model: &SoftmaxModel, rows: &[&RbinRow]) -> [f64; 4] {
    let mut out = [0f64; 4];
    for c in 0..4usize {
        let n = rows.iter().filter(|r| r.label as usize == c).count();
        out[c] = if n == 0 {
            0.0
        } else {
            let hits = rows
                .iter()
                .filter(|r| r.label as usize == c && argmax(&model.forward(&r.features)) == c)
                .count();
            hits as f64 / n as f64
        };
    }
    out
}

fn ece(model: &SoftmaxModel, rows: &[&RbinRow]) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    let mut bins = [(0u64, 0.0f64, 0.0f64); 10];
    for r in rows {
        let p = model.forward(&r.features);
        let (imax, &pmax) = p
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((0, &0.0));
        let b = (pmax * 9.999).floor().min(9.0) as usize;
        bins[b].0 += 1;
        bins[b].1 += pmax;
        bins[b].2 += (imax == r.label as usize) as u64 as f64;
    }
    let total = rows.len() as f64;
    bins.iter()
        .filter(|b| b.0 > 0)
        .map(|b| (b.0 as f64 / total) * (b.1 / b.0 as f64 - b.2 / b.0 as f64).abs())
        .sum()
}
