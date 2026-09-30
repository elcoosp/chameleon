//! Cross-entropy trainer (SPECS/05 §3): minibatch 512 SGD, lr 0.05 ×0.5/10 epochs,
//! L2 1e-4, ≤ 100 epochs, early stop on B-dev loss plateau.

use crate::RouterError;
use crate::dataset::{RbinRow, SESSION_A, SESSION_BDEV};
use crate::model::SoftmaxModel;

pub const MINIBATCH: usize = 512;
pub const LR0: f64 = 0.05;
pub const L2: f64 = 1e-4;
pub const MAX_EPOCHS: usize = 100;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TrainReport {
    pub epochs: usize,
    pub final_loss_b_dev: f64,
    pub top1_b_dev: f64,
    pub top1_b_test: f64,
    pub ece_b_test: f64,
    pub ece_family_c: f64,
    /// Temperature-scaling T fitted on B-dev (2026-09-30). T=1 means the
    /// raw softmax is already well-calibrated. Use `RouterRuntime::new`
    /// with this value to apply it at inference.
    pub temperature: f64,
    /// Raw (uncalibrated) ECE on B-test for comparison.
    pub ece_b_test_raw: f64,
    pub per_class_recall: [f64; 4],
    pub gates_passed: bool,
}

fn split_rows(rows: &[RbinRow], split: u8) -> Vec<&RbinRow> {
    rows.iter()
        .filter(|r| crate::dataset::split_of_session(r.session_id) == split)
        .collect()
}

/// Train on split-A rows, early stop on B-dev plateau; returns the model + report.
pub fn train_model(rows: &[RbinRow]) -> Result<(SoftmaxModel, TrainReport), RouterError> {
    let a: Vec<&RbinRow> = split_rows(rows, SESSION_A);
    let bdev: Vec<&RbinRow> = split_rows(rows, SESSION_BDEV);
    if a.len() < 100 {
        return Err(RouterError::Dataset(format!(
            "split-A too small: {}",
            a.len()
        )));
    }
    // class balance check: any class < 2k rows in A → refuse (SPECS/05 §3).
    // M-3 fix (2026-09-27): the code was checking `n < 2` — a class with 3
    // rows in a 2M-row dataset passed the "2k rows" gate silently.
    const MIN_CLASS_ROWS: usize = 2_000;
    for c in 0..4u8 {
        let n = a.iter().filter(|r| r.label == c).count();
        if n < MIN_CLASS_ROWS {
            return Err(RouterError::Dataset(format!(
                "class {c} has {n} rows in A (min {MIN_CLASS_ROWS})"
            )));
        }
    }
    // PERF (2026-09-29): derive the feature dimension from the data
    // instead of hardcoding 20. Enables the 10-feature opponent-only
    // vector (see docs/plans/ROUTER-FEATURE-LEAK-2026-09-29.md) to train
    // without a router-side change.
    let n_features = a.first().map(|r| r.features.len()).unwrap_or(20);
    let mut model = SoftmaxModel::new(n_features, 4);
    let mut lr = LR0;
    let mut best_loss = f64::INFINITY;
    let mut best_epoch = 0usize;
    let mut epochs_used = 0usize;
    for epoch in 0..MAX_EPOCHS {
        // minibatches: CLASS-BALANCED round-robin interleave (deterministic).
        // A plain label sort puts each class in one contiguous block, so a
        // 512-row minibatch is 1-2 classes and the bias updates oscillate —
        // the interleave gives every minibatch the same class mix.
        let mut buckets: [Vec<usize>; 4] = Default::default();
        for (i, r) in a.iter().enumerate() {
            buckets[r.label as usize % 4].push(i);
        }
        let mut order: Vec<usize> = Vec::with_capacity(a.len());
        let mut cursors = [0usize; 4];
        loop {
            let mut advanced = false;
            for (k, b) in buckets.iter().enumerate() {
                if cursors[k] < b.len() {
                    order.push(b[cursors[k]]);
                    cursors[k] += 1;
                    advanced = true;
                }
            }
            if !advanced {
                break;
            }
        }
        for chunk in order.chunks(MINIBATCH) {
            let batch: Vec<(Vec<f32>, usize)> = chunk
                .iter()
                .map(|&i| (a[i].features.clone(), a[i].label as usize))
                .collect();
            model.sgd_step(&batch, lr, L2);
        }
        // dev loss
        let dev_loss = loss(&model, &bdev);
        if dev_loss < best_loss - 1e-5 {
            best_loss = dev_loss;
            best_epoch = epoch;
        } else if epoch - best_epoch >= 15 {
            break; // plateau early stop (patience 15: batch-averaged lr-0.05
            // gradients move slowly — patience 5 stopped before learning)
        }
        if (epoch + 1) % 10 == 0 {
            lr *= 0.5;
        }
        epochs_used = epoch + 1;
    }
    let top1_b_dev = top1(&model, &bdev);
    let btest: Vec<&RbinRow> = split_rows(rows, crate::dataset::SESSION_BTEST);
    let c: Vec<&RbinRow> = split_rows(rows, crate::dataset::SESSION_C);
    let top1_b_test = top1(&model, &btest);
    // 2026-09-30: temperature-scaling calibration. Grid-search `T > 1`
    // that minimizes ECE on B-dev; report both raw and calibrated ECE.
    // Temperature scaling is monotone in the logits, so top-1 and recall
    // are unchanged; only the reported confidence distribution changes.
    let temperature = calibrate_temperature(&model, &bdev);
    let ece_b_test_raw = ece(&model, &btest);
    let ece_b_test = ece_with_temperature(&model, &btest, temperature);
    let ece_family_c = ece_with_temperature(&model, &c, temperature);
    let per_class_recall = recall(&model, &bdev);
    // G3 gates (SPECS/10 §6): top-1 ≥ 0.80 B-dev; ECE ≤ 0.15 on B-test and
    // family-C; per-class recall B-dev ≥ 0.70 (was computed but never
    // enforced — M-4 fix 2026-09-27). Without the recall gate, a model that
    // ignores one archetype entirely ships as "passing".
    let recall_ok = per_class_recall.iter().all(|&r| r >= 0.70);
    let gates_passed =
        top1_b_dev >= 0.80 && ece_b_test <= 0.15 && ece_family_c <= 0.15 && recall_ok;
    Ok((
        model,
        TrainReport {
            epochs: epochs_used,
            final_loss_b_dev: best_loss,
            top1_b_dev,
            top1_b_test,
            ece_b_test,
            ece_family_c,
            temperature,
            ece_b_test_raw,
            per_class_recall,
            gates_passed,
        },
    ))
}

fn loss(model: &SoftmaxModel, rows: &[&RbinRow]) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    rows.iter()
        .map(|r| {
            let p = model.forward(&r.features);
            -(p[r.label as usize].max(1e-12)).ln()
        })
        .sum::<f64>()
        / rows.len() as f64
}

fn top1(model: &SoftmaxModel, rows: &[&RbinRow]) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    let hits = rows
        .iter()
        .filter(|r| {
            let p = model.forward(&r.features);
            p.iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
                == Some(r.label as usize)
        })
        .count();
    hits as f64 / rows.len() as f64
}

fn recall(model: &SoftmaxModel, rows: &[&RbinRow]) -> [f64; 4] {
    let mut out = [0f64; 4];
    for c in 0..4u8 {
        let class_rows: Vec<&&RbinRow> = rows.iter().filter(|r| r.label == c).collect();
        out[c as usize] = if class_rows.is_empty() {
            0.0
        } else {
            let hits = class_rows
                .iter()
                .filter(|r| {
                    let p = model.forward(&r.features);
                    p.iter()
                        .enumerate()
                        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                        .map(|(i, _)| i)
                        == Some(c as usize)
                })
                .count();
            hits as f64 / class_rows.len() as f64
        };
    }
    out
}

/// Expected calibration error (10 equal-width bins on max probability).
fn ece(model: &SoftmaxModel, rows: &[&RbinRow]) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    let mut bins = [(0u64, 0.0f64, 0.0f64); 10]; // count, conf_sum, correct_sum
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

/// Grid-search a temperature T > 1 that minimizes ECE on the given rows.
///
/// The model's softmax output `p` is re-tempered as `p_i^(1/T)` normalized.
/// T=1 preserves the raw softmax; larger T softens the distribution. The
/// argmax is invariant under monotone temperature scaling (probability
/// mass moves toward uniform but the ordering is preserved), so top-1
/// accuracy and per-class recall are unaffected.
///
/// Returns the T that minimizes ECE. The grid runs 0.5..5.0 in 0.05
/// steps. `T = 1.0` is always in the grid, so if no T improves on the raw
/// softmax the raw is returned.
fn calibrate_temperature(model: &SoftmaxModel, rows: &[&RbinRow]) -> f64 {
    if rows.is_empty() {
        return 1.0;
    }
    let mut best_t = 1.0;
    let mut best_ece = ece_with_temperature(model, rows, 1.0);
    let mut t = 0.5;
    while t <= 5.0 {
        let e = ece_with_temperature(model, rows, t);
        if e < best_ece {
            best_ece = e;
            best_t = t;
        }
        t += 0.05;
    }
    best_t
}

/// ECE with temperature-scaled softmax. Same binning as [`ece`] but the
/// per-row probability is `softmax(logits / T)` instead of the raw.
fn ece_with_temperature(model: &SoftmaxModel, rows: &[&RbinRow], temperature: f64) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    let t = temperature.max(1e-6);
    let mut bins = [(0u64, 0.0f64, 0.0f64); 10];
    for r in rows {
        let logits = model.logits(&r.features);
        // Softmax with temperature: p_i = exp(z_i / T) / sum(exp(z_j / T))
        let scaled: Vec<f64> = logits.iter().map(|&z| z / t).collect();
        let m = scaled.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let exps: Vec<f64> = scaled.iter().map(|&z| (z - m).exp()).collect();
        let sum: f64 = exps.iter().sum();
        let p: Vec<f64> = exps.iter().map(|&e| e / sum).collect();
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
