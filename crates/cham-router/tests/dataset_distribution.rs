//! Diagnostic (2026-10-03): per-class feature distribution in a router
//! training dataset. The shipped `raw-opponent-19` router is degenerate
//! at inference despite 90.6% test accuracy on this data. This dumps the
//! TRAINING-side per-class means to compare against inference-side
//! features. Run with --no-capture.

use cham_router::dataset::read_dataset;

#[test]
fn training_feature_means_by_class() {
    // nextest runs with CWD = the crate dir, so resolve from the manifest.
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/router_raw_19.rbin");
    if !path.exists() {
        eprintln!("no dataset at {}; skipping", path.display());
        return;
    }
    let (rows, nf) = read_dataset(&path).expect("read dataset");
    eprintln!(
        "\n=== {}: {} rows, {nf} features ===",
        path.display(),
        rows.len()
    );

    let mut sums = vec![[0.0f64; 32]; 4];
    let mut counts = [0u64; 4];
    for r in &rows {
        let c = r.label as usize;
        if c >= 4 {
            continue;
        }
        for (i, &v) in r.features.iter().enumerate().take(32) {
            sums[c][i] += v as f64;
        }
        counts[c] += 1;
    }
    for c in 0..4 {
        if counts[c] == 0 {
            continue;
        }
        let mean: Vec<f64> = sums[c][..nf.min(32)]
            .iter()
            .map(|s| s / counts[c] as f64)
            .collect();
        let shown: Vec<String> = mean.iter().take(12).map(|v| format!("{v:.3}")).collect();
        eprintln!("  class {c} (n={}): [{}]", counts[c], shown.join(", "));
    }
    assert!(rows.len() > 0);
}
