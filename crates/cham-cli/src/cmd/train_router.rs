//! `chameleon train-router` (SPECS/05): softmax router training + gates.

pub fn run(rows_path: &str, out: &str, feature_set: &str) -> i32 {
    let (rows, nf) = match cham_router::read_dataset(std::path::Path::new(rows_path)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("read {rows_path}: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    // PERF (2026-09-29): accept any feature count. The 20-dim
    // opportunity-gated vector has been shown to be (opponent,hero)-
    // dependent; the honest 10-dim opponent-only vector is the intended
    // replacement. See docs/plans/ROUTER-FEATURE-LEAK-2026-09-29.md.
    if nf == 0 {
        eprintln!("dataset has 0 features");
        return crate::cmd::EXIT_FAIL;
    }
    eprintln!("train-router: {nf} features");
    // Spec gate (SPECS/05 §4): the trainer refuses any archetype with < 2k rows
    // in the A split. The library check is relaxed to unit-scale so fixtures can
    // exercise the training math; the PRODUCTION entry point enforces the real
    // number here.
    {
        use cham_router::dataset::{SESSION_A, split_of_session};
        for c in 0..4u8 {
            let n = rows
                .iter()
                .filter(|r| r.label == c && split_of_session(r.session_id) == SESSION_A)
                .count();
            if n < 2_000 {
                eprintln!(
                    "train: class {c} has {n} rows in split A — spec requires ≥ 2000 (collect more sessions)"
                );
                return crate::cmd::EXIT_FAIL;
            }
        }
    }
    let (model, report) = match cham_router::train_model(&rows) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("train: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    let out_dir = std::path::Path::new(out);
    if let Err(e) = std::fs::create_dir_all(out_dir) {
        eprintln!("mkdir: {e}");
        return crate::cmd::EXIT_FAIL;
    }
    // 2026-09-30: fold temperature-scaling calibration into the model's
    // weights so the runtime's forward() produces the calibrated softmax.
    // Also record the feature set name so the agent pipeline can dispatch
    // the right feature constructor at inference.
    let model = model
        .with_temperature(report.temperature)
        .with_feature_set(feature_set.to_string());
    let model_json = serde_json::to_vec_pretty(&model).unwrap_or_default();
    // Write BOTH filenames:
    //   * `model.bin` — the historical name train-router has always used
    //   * `router.bin` — the name hero/play/probe/audit_buckets look for when
    //     loading a bundle (`cmd/hero.rs:73` etc.)
    // Before this, `train-router` wrote only `model.bin`, so a freshly
    // trained router was NEVER picked up by any agent construction path —
    // the fallback (deterministic small init) served every measurement.
    if let Err(e) = std::fs::write(out_dir.join("model.bin"), &model_json) {
        eprintln!("write model: {e}");
        return crate::cmd::EXIT_FAIL;
    }
    if let Err(e) = std::fs::write(out_dir.join("router.bin"), &model_json) {
        eprintln!("write router: {e}");
        return crate::cmd::EXIT_FAIL;
    }
    let _ = std::fs::write(
        out_dir.join("metrics.json"),
        serde_json::to_vec_pretty(&report).unwrap_or_default(),
    );
    println!(
        "train-router: rows={} epochs={} loss_b_dev={:.4} top1_b_dev={:.3} top1_b_test={:.3} ece_b_test={:.3} ece_family_c={:.3} recall={:?} gates={}",
        rows.len(),
        report.epochs,
        report.final_loss_b_dev,
        report.top1_b_dev,
        report.top1_b_test,
        report.ece_b_test,
        report.ece_family_c,
        report.per_class_recall,
        if report.gates_passed { "PASS" } else { "FAIL" }
    );
    if report.gates_passed {
        crate::cmd::EXIT_OK
    } else {
        crate::cmd::EXIT_FAIL
    }
}
