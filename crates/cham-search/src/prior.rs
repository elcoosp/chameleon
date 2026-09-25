//! Priors (SPECS/06 §3): blueprint strategy + visit-confidence flattening.
//! Classes collapse the routed blueprint's reach: per-combo reach through the
//! abstraction (pseudo-harmonic weights for off-tree sizes), flattened by the
//! per-path product of visit confidence `c(i) = visits/(visits+64)`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriorStrats {
    /// per infoset path: distribution over actions
    pub strat: std::collections::BTreeMap<String, Vec<f64>>,
}

impl PriorStrats {
    pub fn empty() -> PriorStrats {
        PriorStrats { strat: std::collections::BTreeMap::new() }
    }
    pub fn set(&mut self, path: &str, probs: Vec<f64>) {
        self.strat.insert(path.to_string(), probs);
    }
    pub fn get(&self, path: &str) -> Option<&Vec<f64>> {
        self.strat.get(path)
    }

    /// Visit-confidence flattening (SPECS/06 §3): per-path product of
    /// `c(i) = visits/(visits+64)`; paths with product < 0.1 floored at 0.1×
    /// prior share — the solver knows where the blueprint is unvisited.
    pub fn flatten(&self, path: &str, confidence: f64) -> Option<Vec<f64>> {
        let base = self.strat.get(path)?;
        let c = confidence.clamp(0.0, 1.0);
        if c >= 0.1 {
            Some(base.clone())
        } else {
            // floor toward uniform at low confidence: blend with uniform so the
            // solver is not misled by an unvisited path
            let u = 1.0 / base.len() as f64;
            let floor = 0.1;
            let w = (c / floor).clamp(0.0, 1.0); // 0 at zero confidence → uniform
            Some(base.iter().map(|&p| w * p + (1.0 - w) * u).collect())
        }
    }
}

/// Class collapse from weighted combos: sort by strength, bucket into `k` classes
/// with normalized weights (deterministic; card removal applied upstream — the
/// caller passes dead-card-adjusted combo weights).
pub fn collapse_to_classes(mut weighted: Vec<(f64 /*weight*/, f64 /*strength*/)>, k: usize) -> Vec<crate::subgame::Class> {
    weighted.retain(|(w, _)| *w > 0.0); // dead-card removal upstream zeroes combos
    weighted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let total: f64 = weighted.iter().map(|(w, _)| *w).sum();
    if total <= 0.0 {
        return Vec::new();
    }
    let mut classes: Vec<crate::subgame::Class> = Vec::new();
    let chunk = weighted.len().div_ceil(k);
    let mut start = 0usize;
    while start < weighted.len() {
        let end = (start + chunk).min(weighted.len());
        let mut w_sum = 0.0;
        let mut s_sum = 0.0;
        for r in &weighted[start..end] {
            w_sum += r.0;
            s_sum += r.1 * r.0;
        }
        if w_sum > 0.0 {
            classes.push(crate::subgame::Class { weight: w_sum / total, strength: s_sum / w_sum });
        }
        start = end;
    }
    // normalize weights exactly
    let total: f64 = classes.iter().map(|c| c.weight).sum();
    for c in classes.iter_mut() {
        c.weight /= total;
    }
    classes
}
