//! Statistics (SPECS/08 §3): mean/SE/bootstrap/Welch/SPRT/session-cluster/paired —
//! no deps beyond std.

use serde::{Deserialize, Serialize};

use crate::EvalError;

pub fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().sum::<f64>() / v.len() as f64
}

pub fn se(v: &[f64]) -> f64 {
    let n = v.len();
    if n < 2 {
        return 0.0;
    }
    let m = mean(v);
    let var = v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1) as f64;
    (var / n as f64).sqrt()
}

/// Bootstrap CI over resampled DEALS (seeded; percentile method).
pub fn bootstrap_ci(
    v: &[f64],
    conf: f64,
    resamples: usize,
    rng: &mut cham_core::rng::Rng,
) -> (f64, f64) {
    let n = v.len();
    if n == 0 {
        return (0.0, 0.0);
    }
    let mut means = Vec::with_capacity(resamples);
    for _ in 0..resamples {
        let mut acc = 0.0;
        for _ in 0..n {
            let i = cham_core::rng::pick(rng, n);
            acc += v[i];
        }
        means.push(acc / n as f64);
    }
    means.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let alpha = (1.0 - conf) / 2.0;
    let lo = means[(alpha * resamples as f64) as usize % resamples];
    let hi = means[((1.0 - alpha) * resamples as f64) as usize % resamples];
    (lo, hi)
}

/// SESSION-CLUSTERED bootstrap (review C1): sessions are the independent units
/// (the opponent draw is per session); used for ABSOLUTE winrates. Paired A/B
/// diffs cancel session effects → deal-level paired CI instead.
pub fn session_cluster_ci(
    per_deal: &[f64],
    session_of_deal: &[u32],
    conf: f64,
    rng: &mut cham_core::rng::Rng,
) -> (f64, f64) {
    assert_eq!(per_deal.len(), session_of_deal.len());
    // group deals by session
    let mut sessions: std::collections::BTreeMap<u32, Vec<f64>> = std::collections::BTreeMap::new();
    for (d, s) in per_deal.iter().zip(session_of_deal.iter()) {
        sessions.entry(*s).or_default().push(*d);
    }
    let keys: Vec<u32> = sessions.keys().copied().collect();
    if keys.len() < 2 {
        return (0.0, 0.0);
    }
    let session_means: Vec<f64> = keys.iter().map(|k| mean(&sessions[k])).collect();
    let k = keys.len();
    let mut means = Vec::with_capacity(400);
    let resamples = 400;
    for _ in 0..resamples {
        let mut acc = 0.0;
        for _ in 0..k {
            let i = cham_core::rng::pick(rng, k);
            acc += session_means[i];
        }
        means.push(acc / k as f64);
    }
    means.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let alpha = (1.0 - conf) / 2.0;
    let lo = means[(alpha * resamples as f64) as usize % resamples];
    let hi = means[((1.0 - alpha) * resamples as f64) as usize % resamples];
    (lo, hi)
}

/// Paired CI over per-deal diffs (deal-level bootstrap).
pub fn paired_ci(diffs: &[f64], conf: f64, rng: &mut cham_core::rng::Rng) -> (f64, f64) {
    bootstrap_ci(diffs, conf, 400, rng)
}

/// Welch's t (approximate CI for the difference of means).
pub fn welch_t(a: &[f64], b: &[f64]) -> (f64, f64) {
    let (ma, mb) = (mean(a), mean(b));
    let (va, vb) = (variance(a), variance(b));
    let (na, nb) = (a.len().max(1) as f64, b.len().max(1) as f64);
    let se = (va / na + vb / nb).sqrt().max(1e-12);
    let t = (ma - mb) / se;
    // 95% CI with normal approximation
    let ci = 1.96 * se;
    (t, ci)
}

fn variance(v: &[f64]) -> f64 {
    let n = v.len();
    if n < 2 {
        return 0.0;
    }
    let m = mean(v);
    v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1) as f64
}

/// SPRT state (Wald; normal model with known-ish σ).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SprtState {
    Continue,
    AcceptH0,
    AcceptH1,
}

/// Wald SPRT over accumulating paired diffs: H0: Δ ≤ delta0 vs H1: Δ ≥ delta1
/// (defaults 0 vs +25 mb/seating, α = 0.05, β = 0.10). σ from the data.
pub fn sprrt(
    diffs: &[f64],
    delta0: f64,
    delta1: f64,
    alpha: f64,
    beta: f64,
) -> Result<SprtState, EvalError> {
    if diffs.is_empty() {
        return Ok(SprtState::Continue);
    }
    if delta1 <= delta0 {
        return Err(EvalError::Stats("delta1 must exceed delta0".into()));
    }
    let sd = se(diffs).max(1e-9);
    let n = diffs.len() as f64;
    let m = mean(diffs);
    // LLR ≈ n[(m − δ0)² − (m − δ1)²] / (2 sd²) — Wald's approximation
    let llr = n * ((m - delta0).powi(2) - (m - delta1).powi(2)) / (2.0 * sd * sd);
    let a = ((1.0 - beta) / alpha).ln();
    let b = (beta / (1.0 - alpha)).ln();
    if llr >= a {
        Ok(SprtState::AcceptH1)
    } else if llr <= b {
        Ok(SprtState::AcceptH0)
    } else {
        Ok(SprtState::Continue)
    }
}

/// EXP-018: empirical payoff matrix over the agent zoo from ledger A/B rows.
/// `triples` are (a_mode, b_mode, delta_mb) with delta signed a-minus-b.
/// Missing cells fill 0.0 (reported by the caller, never assumed).
pub fn build_payoff_matrix(modes: &[&str], triples: &[(String, String, f64)]) -> Vec<Vec<f64>> {
    let n = modes.len();
    let mut sum = vec![vec![0.0; n]; n];
    let mut count = vec![vec![0u32; n]; n];
    for (a, b, d) in triples {
        if let (Some(i), Some(j)) = (
            modes.iter().position(|&m| m == a),
            modes.iter().position(|&m| m == b),
        ) {
            sum[i][j] += d;
            sum[j][i] -= d;
            count[i][j] += 1;
            count[j][i] += 1;
        }
    }
    (0..n)
        .map(|i| {
            (0..n)
                .map(|j| {
                    if count[i][j] > 0 {
                        sum[i][j] / count[i][j] as f64
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect()
}

/// Holm step-down correction (SPECS/08 §3): returns which hypotheses are rejected
/// at familywise α.
pub fn holm(pvals: &[f64], alpha: f64) -> Vec<bool> {
    let n = pvals.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| {
        pvals[i]
            .partial_cmp(&pvals[j])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut rejected = vec![false; n];
    for (rank, &i) in order.iter().enumerate() {
        let adj = pvals[i] * (n - rank) as f64;
        if adj <= alpha {
            rejected[i] = true;
        } else {
            break; // step-down stops at the first non-rejection
        }
    }
    rejected
}

/// Required seatings for a CI half-width of `delta_mb` at `conf`.
pub fn required_seatings(sigma_pair: f64, delta_mb: f64, conf: f64) -> u64 {
    // n = (z(conf)·σ/δ)²; z for two-sided conf via the rational approximation of
    // the inverse normal (Acklam-lite; sufficient for budget tables)
    let z = z_for(conf);
    let s = sigma_pair * 1000.0; // bb → mb
    ((z * s / delta_mb).powi(2)).ceil() as u64
}

fn z_for(conf: f64) -> f64 {
    // two-sided inverse normal (Acklam's rational approximation)
    let p = (1.0 - conf) / 2.0;
    let a = [
        -3.969683028665376e1,
        2.209460984245205e2,
        -2.759285104469687e2,
        1.38357751867269e2,
        -3.066479806614716e1,
        2.506628277459239,
    ];
    let b = [
        -5.447609879822406e1,
        1.615858368580409e2,
        -1.556989798598866e2,
        6.680131188771972e1,
        -1.328068155288572e1,
    ];
    let c = [
        -7.784894002430293e-3,
        -3.223964580411365e-1,
        -2.400758277161838,
        -2.549732539343734,
        4.374664141464968,
        2.938163982698783,
    ];
    let d = [
        7.784695709041462e-3,
        3.224671290700398e-1,
        2.445134137142996,
        3.754408661907416,
    ];
    let p_low = 0.02425;
    let x = if p < p_low {
        let q = (-p.ln()).sqrt();
        (((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5])
            / ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0)
    } else if p <= 1.0 - p_low {
        let q = p - 0.5;
        let r = q * q;
        (((((a[0] * r + a[1]) * r + a[2]) * r + a[3]) * r + a[4]) * r + a[5]) * q
            / (((((b[0] * r + b[1]) * r + b[2]) * r + b[3]) * r + b[4]) * r + 1.0)
    } else {
        let q = (-(1.0 - p).ln()).sqrt();
        -(((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5])
            / ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0)
    };
    -x
}
