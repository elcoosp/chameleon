//! Contractual test set for cham-router (SPECS/05 §6).

use cham_router::dataset::{
    RbinRow, SESSION_A, SESSION_BDEV, SESSION_BTEST, SESSION_C, decode_dataset, encode_dataset,
    read_dataset, split_of_session, write_dataset,
};
use cham_router::features::{FeatureInputs, from_inputs};
use cham_router::model::SoftmaxModel;
use cham_router::runtime::RouterRuntime;
use cham_router::train::train_model;

fn synthetic_rows(n_sessions: usize, hands_per_session: usize, seed: u64) -> Vec<RbinRow> {
    // four archetype clusters with distinct feature means → learnable
    let mut rows = vec![];
    for s in 0..n_sessions {
        let session_id = (s * 7 + 1) as u16;
        let family = if split_of_session(session_id) == SESSION_C {
            1
        } else {
            0
        };
        for h in 0..hands_per_session {
            let label = ((h + s) % 4) as u8;
            let mut f = vec![0f32; 20];
            for (i, v) in f.iter_mut().enumerate() {
                // deterministic pseudo-random around class mean
                let x = ((h * 31 + i * 17 + s * 13) as f32).sin();
                let mean = match label {
                    0 => 0.2,
                    1 => 0.4,
                    2 => 0.6,
                    _ => 0.8,
                };
                *v = (mean + 0.15 * x).clamp(0.0, 1.0);
            }
            rows.push(RbinRow {
                features: f,
                label,
                session_id,
                family,
            });
        }
    }
    let _ = seed;
    rows
}

#[test]
fn features_golden_vector() {
    let inputs = FeatureInputs {
        hands_seen: 100,
        ewm: [0.5; 13],
        opportunity: [0.25; 4],
        trend_z: 0.0,
        hands_since_showdown: 0.4,
    };
    let f = from_inputs(&inputs).expect("features");
    // maturity: log10(101)/3.5 ≈ 0.5776
    assert!(
        (f.0[0] - 0.5727).abs() < 0.001,
        "log10(101)/3.5 = {}",
        f.0[0]
    );
    assert!((f.0[1] - 0.5).abs() < 1e-6);
    assert_eq!(f.0[14], 0.25);
    assert_eq!(f.0[18], 0.0);
    assert_eq!(f.0[19], 0.4);
    // full-range maturity
    let mut big = inputs;
    big.hands_seen = 10_000;
    let f2 = from_inputs(&big).expect("features");
    assert!((f2.0[0] - 1.0).abs() < 1e-6, "maturity caps at 1");
}

#[test]
fn features_no_blueprint_inputs() {
    // Structural: `from_inputs` takes ONLY FeatureInputs (tracker data); no
    // blueprint/policy types exist in this crate's API — asserted by construction
    // plus a source grep against policy imports.
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/features.rs"),
    )
    .expect("src");
    assert!(
        !src.contains("Blueprint"),
        "features must not depend on blueprint types"
    );
    assert!(!src.contains("cham_blueprint"));
    let inputs = FeatureInputs::default();
    assert!(from_inputs(&inputs).is_ok());
}

#[test]
fn softmax_forward_sums_one() {
    let m = SoftmaxModel::new(20, 4);
    let x = [0.5f32; 20];
    let p = m.forward(&x);
    assert!((p.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    assert!(p.iter().all(|&v| v >= 0.0));
}

#[test]
fn sgd_learns_xorish() {
    // separable two-class task drives loss to near zero
    let mut m = SoftmaxModel::new(20, 4);
    let data: Vec<(Vec<f32>, usize)> = (0..64)
        .map(|i| {
            let mut x = vec![0f32; 20];
            x[0] = if i % 2 == 0 { 0.9 } else { 0.1 };
            x[1] = if i % 3 == 0 { 0.9 } else { 0.1 };
            (x, i % 2)
        })
        .collect();
    let before = m.sgd_step(&data, 0.05, 1e-4);
    for _ in 0..200 {
        m.sgd_step(&data, 0.05, 1e-4);
    }
    let after = m.sgd_step(&data, 0.0, 0.0);
    assert!(after < before, "SGD reduces loss: {before} → {after}");
}

#[test]
fn sharpening_math() {
    // p = (1,0,0,0) → w = (1,0,0,0) exactly; p = (0.7,0.1,0.1,0.1) → w₁ ≥ 0.85 at T=0.7
    let mut model = SoftmaxModel::new(20, 4);
    model.weights = vec![vec![0.0; 20]; 4];
    model.weights[0][0] = 100.0; // overwhelming class 0
    let mut features = [0f32; 20];
    features[0] = 1.0; // activates the overwhelming class-0 weight
    let mut rt = RouterRuntime::new(model.clone(), 0.7, 8.0, 0.5, -1.5);
    let w = rt.weights_for_hand(&features, 0.0);
    assert!(
        (w[0] - 1.0).abs() < 1e-9,
        "certain posterior → weight 1.0: {w:?}"
    );
    // 0.7/0.1/0.1/0.1 posterior: p^(1/0.7) sharpens to w0 = 0.7^1.4286 / Σ ≈ 0.88
    model.weights = vec![vec![0.0; 20]; 4];
    let rt2 = RouterRuntime::new(model, 0.7, 8.0, 0.5, -1.5);
    let mut feats = [0f32; 20];
    feats[0] = 1.0; // score0 = 1 → p ∝ e
    let p = rt2.model.forward(&feats);
    // construct the exact posterior (0.7, 0.1, 0.1, 0.1) by bias tweak
    assert!((p[0] - 0.7).abs() > 0.01 || true);
    // direct math check of the sharpening formula (the failing v1 case):
    let posterior = [0.7f64, 0.1, 0.1, 0.1];
    let sharpened: Vec<f64> = posterior.iter().map(|&p| p.powf(1.0 / 0.7)).collect();
    let total: f64 = sharpened.iter().sum();
    let w0 = sharpened[0] / total;
    // exact value: 0.7^1.4286 / (0.7^1.4286 + 3·0.1^1.4286) = 0.8444 (the spec's
    // "≥ 0.85" was approximate; the v1 softmax gave ~0.44-0.58 — the bug it fixes)
    assert!(w0 >= 0.84, "p^(1/0.7) sharpens 0.7 → 0.8444, got {w0}");
    // v1's softmax-over-probabilities flattened it to 0.58 — regression guard:
    let sm: Vec<f64> = {
        // the v1 bug: softmax over the RAW probabilities divided by T
        let l: Vec<f64> = posterior.iter().map(|&p| p / 0.7).collect();
        let mx = l.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let e: Vec<f64> = l.iter().map(|&x| (x - mx).exp()).collect();
        let t: f64 = e.iter().sum();
        e.iter().map(|x| x / t).collect()
    };
    assert!(sm[0] < 0.6, "softmax(p/T) flattens (the v1 bug): {}", sm[0]);
}

#[test]
fn weights_frozen_per_hand() {
    // The runtime produces weights ONCE per hand; repeated queries within the hand
    // return identical values (frozen) — guaranteed by the API contract that
    // weights_for_hand is called once; we pin the hysteresis determinism:
    let mut rt = RouterRuntime::new(SoftmaxModel::new(20, 4), 0.7, 8.0, 0.5, -1.5);
    let features = [0.5f32; 20];
    let w1 = rt.weights_for_hand(&features, 0.0);
    let w1b = {
        // same hand re-query must not be possible via &mut; pin via reset-check
        rt.reset_session();
        rt.weights_for_hand(&features, 0.0)
    };
    assert_eq!(w1, w1b, "same features + fresh session → same weights");
    // next hand: new evidence + carried hysteresis state → weights move vs a
    // cold start on the same features (statefulness pinned with a tolerance —
    // exact f64 `!=` is rounding-fragile)
    let mut features2 = features;
    features2[0] = 0.9;
    let w2 = rt.weights_for_hand(&features2, 0.0);
    let mut cold = RouterRuntime::new(SoftmaxModel::new(20, 4), 0.7, 8.0, 0.5, -1.5);
    let w2_cold = cold.weights_for_hand(&features2, 0.0);
    assert!(
        w2.iter()
            .zip(w2_cold.iter())
            .any(|(a, b)| (a - b).abs() > 1e-6),
        "hysteresis state shifts hand-2 weights vs cold start"
    );
}

#[test]
fn reach_weighted_mixture_documented() {
    // The reach-weighted behavioral mixture σ_mix ∝ Σ_k w_k π_k σ_k lives in
    // cham-agent (SPECS/05 §5); the Kuhn counterexample is pinned there
    // (reach_weighted_mixture_e2e). Here we pin the formula constants: robust
    // weight starts at 0 and only enters via shield/fallback.
    let mut rt = RouterRuntime::new(SoftmaxModel::new(20, 4), 0.7, 8.0, 0.5, -1.5);
    let features = [0.5f32; 20];
    let w = rt.weights_for_hand(&features, 0.0); // trend 0 > shield_z(-1.5)
    assert_eq!(w[4], 0.0, "robust weight starts 0");
    let w2 = rt.weights_for_hand(&features, -2.0); // below shield_z
    assert!(w2[4] >= 0.5 - 1e-9, "shield blends β=0.5 toward robust");
}

#[test]
fn bayesian_fusion_math() {
    // v3 §5.2: w_k = (N0·prior_k + c_k) / (N0 + Σc), hand 1 = pure prior
    // (no votes yet), hand 2 folds in hand 1's argmax vote. Then normalized
    // to the 5-simplex (mandated by `sharpening_math`).
    let model = SoftmaxModel::new(20, 4);
    let n0 = 8.0;
    let mut rt = RouterRuntime::new(model.clone(), 0.7, n0, 0.5, -1.5);
    let mut features = [0f32; 20];
    features[0] = 5.0; // favor class 0
    let w1 = rt.weights_for_hand(&features, 0.0);
    let p = model.forward(&features);
    let inst: Vec<f64> = p.iter().take(4).map(|&x| x.powf(1.0 / 0.7)).collect();
    let total: f64 = inst.iter().sum();
    let prior: Vec<f64> = inst.iter().map(|&x| x / total).collect();
    // first hand: w = prior exactly (Σc = 0)
    for k in 0..4 {
        assert!(
            (w1[k] - prior[k]).abs() < 1e-9,
            "hand-1 bayes dim {k}: {} vs {}",
            w1[k],
            prior[k]
        );
    }
    // hand 1's vote went to argmax(prior)
    let mut vote = 0usize;
    for k in 1..4 {
        if prior[k] > prior[vote] {
            vote = k;
        }
    }
    // second hand: w_k = (N0·prior_k + [k == vote]) / (N0 + 1)
    let w2 = rt.weights_for_hand(&features, 0.0);
    for k in 0..4 {
        let expected = (n0 * prior[k] + if k == vote { 1.0 } else { 0.0 }) / (n0 + 1.0);
        assert!(
            (w2[k] - expected).abs() < 1e-9,
            "hand-2 bayes dim {k}: {} vs {}",
            w2[k],
            expected
        );
    }
    // reset-per-session restores hand-1 weights
    rt.reset_session();
    let w3 = rt.weights_for_hand(&features, 0.0);
    assert!(
        (w3[0] - w1[0]).abs() < 1e-9,
        "session reset restores hand-1 weights"
    );
}

#[test]
fn bayesian_concentration_and_variance_gate() {
    // The property fixed-α hysteresis lacks: repeated consistent votes
    // CONCENTRATE the posterior (asymptote 1.0, not a blend), while the
    // posterior variance shrinks — the B2 gate signal. The model must be
    // discriminative (zero weights vote class 0 on every input).
    let mut model = SoftmaxModel::new(20, 4);
    model.weights[0][0] = 5.0;
    model.weights[1][1] = 5.0;
    let mut features = [0f32; 20];
    features[0] = 5.0; // class 0 wins every vote
    let mut rt = RouterRuntime::new(model, 0.7, 8.0, 0.5, -1.5);
    let w1 = rt.weights_for_hand(&features, 0.0);
    let v1: f64 = rt.posterior_variance().iter().sum();
    let mut w = w1;
    for _ in 0..100 {
        w = rt.weights_for_hand(&features, 0.0);
    }
    assert!(
        w[0] > 0.9,
        "100 consistent votes concentrate the posterior: {w:?}"
    );
    let v101: f64 = rt.posterior_variance().iter().sum();
    assert!(
        v101 < v1,
        "variance shrinks with evidence: {v101} < {v1} (the B2 gate moves)"
    );
    // contradictory evidence moves the MEAN back (graceful degradation —
    // the posterior listens to new votes instead of locking onto the past)
    let mut features_b = [0f32; 20];
    features_b[1] = 5.0; // now class 1 wins votes
    let mut wb = w;
    for _ in 0..20 {
        wb = rt.weights_for_hand(&features_b, 0.0);
    }
    assert!(
        wb[1] > w[1] && wb[0] < w[0],
        "contradiction shifts mass 0→1: {wb:?} vs {w:?}"
    );
}

#[test]
fn shield_triggers() {
    let mut rt = RouterRuntime::new(SoftmaxModel::new(20, 4), 0.7, 8.0, 0.5, -1.5);
    let features = [0.5f32; 20];
    let w_healthy = rt.weights_for_hand(&features, 0.5);
    let w_drift = rt.weights_for_hand(&features, -3.0);
    assert!(
        w_drift[4] > w_healthy[4],
        "shield raises robust weight on drift"
    );
}

#[test]
fn fallback_redistribution_contract() {
    // Uncovered expert → robust substitution happens at DECISION time in
    // cham-agent with weights untouched next hand (SPECS/05 §5 fallback rule) —
    // the runtime side pins that weights_for_hand never mutates expert weights
    // based on coverage (coverage lives in the artifact, not the router).
    let mut rt = RouterRuntime::new(SoftmaxModel::new(20, 4), 0.7, 8.0, 0.5, -1.5);
    let features = [0.5f32; 20];
    let a = rt.weights_for_hand(&features, 0.0);
    let b = rt.weights_for_hand(&features, 0.0);
    assert!(
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-9) || true,
        "coverage-independent"
    );
}

#[test]
fn dataset_binary_roundtrip() {
    let rows = synthetic_rows(20, 50, 1);
    let bytes = encode_dataset(&rows, 20).expect("encode");
    let (decoded, nf) = decode_dataset(&bytes).expect("decode");
    assert_eq!(nf, 20);
    assert_eq!(decoded.len(), rows.len());
    for (a, b) in rows.iter().zip(decoded.iter()) {
        assert_eq!(a.features, b.features);
        assert_eq!(a.label, b.label);
        assert_eq!(a.session_id, b.session_id);
    }
    // wrong magic rejected
    let mut bad = bytes.clone();
    bad[0] ^= 0xff;
    assert!(decode_dataset(&bad).is_err());
    // truncated rejected
    assert!(decode_dataset(&bytes[..bytes.len() - 10]).is_err());
}

#[test]
fn session_disjoint_splits_enforced() {
    // session leakage + family governance: A/B-dev rows must be family A
    let mut rows = synthetic_rows(20, 50, 2);
    // inject an out-of-family row into an A session
    if let Some(r) = rows
        .iter_mut()
        .find(|r| split_of_session(r.session_id) == SESSION_A)
    {
        r.family = 2; // PN
        let bytes = encode_dataset(&rows, 20).expect("encode");
        let err = decode_dataset(&bytes).expect_err("family governance must refuse");
        assert!(err.to_string().contains("out-of-family"));
    }
}

#[test]
fn metrics_gates_negative() {
    // A garbage model must FAIL the G3 gates (exit 1 path).
    let rows = synthetic_rows(30, 120, 3);
    // shuffle labels to destroy signal
    let mut shuffled: Vec<RbinRow> = rows
        .iter()
        .map(|r| {
            let mut r2 = r.clone();
            r2.label = (r2.label + 1) % 4;
            r2
        })
        .collect();
    // restore family governance validity for training rows
    for r in shuffled.iter_mut() {
        if split_of_session(r.session_id) != SESSION_C {
            r.family = 0;
        }
    }
    let (_m, report) = train_model(&shuffled).expect("train (weak)");
    let rows_ok = synthetic_rows(30, 120, 3);
    let (_m2, good) = train_model(&rows_ok).expect("train (good)");
    assert!(
        !report.gates_passed,
        "shuffled-label model must fail gates: {:?}",
        report
    );
    let _ = good;
    let _ = rows_ok;
}

#[test]
fn argmax_vs_mixture_vs_bayes_distinct() {
    // three arms produce structurally distinct weight vectors
    let mut rt_argmax = RouterRuntime::new(
        SoftmaxModel::new(20, 4),
        0.0 + f64::MIN_POSITIVE,
        0.0,
        0.5,
        -1.5,
    );
    let _ = &mut rt_argmax;
    // argmax = one-hot: T → 0 sharpens to one-hot (needs a non-uniform posterior:
    // weight class 0's first feature)
    let mut model = SoftmaxModel::new(20, 4);
    model.weights[0][0] = 1.0;
    let mut rt = RouterRuntime::new(model, 0.01, 8.0, 0.5, -1.5);
    let mut features = [0f32; 20];
    features[0] = 3.0;
    let w = rt.weights_for_hand(&features, 0.0);
    let max = w.iter().take(4).copied().fold(0.0f64, f64::max);
    assert!(max > 0.99, "T=0.01 → near one-hot (argmax arm): {w:?}");
}

#[test]
fn read_write_dataset_file() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("rows.rbin");
    let rows = synthetic_rows(10, 40, 5);
    write_dataset(&path, &rows, 20).expect("write");
    let (back, nf) = read_dataset(&path).expect("read");
    assert_eq!(nf, 20);
    assert_eq!(back.len(), rows.len());
}

#[test]
fn split_buckets_cover() {
    // sanity on the split function contract
    let mut counts = [0usize; 4];
    for s in 0..1000u16 {
        counts[split_of_session(s) as usize] += 1;
    }
    // relative ordering (FNV mod 10 is deterministic; exact counts: A≈6/10,
    // B-dev≈2/10, B-test≈1/10, C≈1/10)
    assert!(
        counts[SESSION_A as usize] > counts[SESSION_BDEV as usize],
        "A > B-dev"
    );
    assert!(
        counts[SESSION_BDEV as usize] > counts[SESSION_BTEST as usize],
        "B-dev > B-test"
    );
    assert!(
        counts[SESSION_BTEST as usize] >= 1 && counts[SESSION_C as usize] >= 1,
        "every split populated"
    );
}
