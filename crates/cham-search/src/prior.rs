//! Priors (SPECS/06 §3): blueprint strategy + visit-confidence flattening.
//! Classes collapse the routed blueprint's reach: per-combo reach through the
//! abstraction (pseudo-harmonic weights for off-tree sizes), flattened by the
//! per-path product of visit confidence `c(i) = visits/(visits+64)`.

use serde::{Deserialize, Serialize};

use cham_core::engine::Action;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriorStrats {
    /// per infoset path: distribution over actions
    pub strat: std::collections::BTreeMap<String, Vec<f64>>,
}

impl PriorStrats {
    pub fn empty() -> PriorStrats {
        PriorStrats {
            strat: std::collections::BTreeMap::new(),
        }
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

/// Leaf continuation set for turn-subgame solving (v3 §5.1 — DeepStack's
/// actual lesson, not just "solve deeper").
///
/// A single fixed leaf continuation strategy is itself exploitable, and it is
/// a DIFFERENT bug class from unvisited paths (which `flatten` already
/// handles): a *confident but wrong* leaf prior. The turn solver (A3) holds
/// 2–3 perturbed leaf variants alongside the base blueprint prior and blends
/// them with a small combinator solved *within* the subgame, so the solver is
/// robust against the leaves instead of trusting one.
///
/// Variants are semantic (identified by `Action`, not slot index, so they
/// survive ladder reorderings): `call_heavy` shifts mass onto Call (Check
/// when facing no bet — the passive direction), `fold_heavy` shifts mass
/// onto Fold (Check when no Fold slot exists — still the weak direction).
/// All variants are precomputed offline from the frozen blueprint prior: no
/// online learning, no determinism impact on the decision path.
#[derive(Clone, Debug)]
pub struct LeafSet {
    pub base: Vec<f64>,
    pub call_heavy: Vec<f64>,
    pub fold_heavy: Vec<f64>,
}

/// Mass shifted from aggressive/neutral actions onto the variant's target.
pub const LEAF_TILT: f64 = 0.25;

/// Build the leaf set for one infoset: `base` (blueprint prior) plus the two
/// perturbed variants. `actions` parallels `base` (same order as the
/// solver's slots); mismatched lengths yield `None` rather than a silent
/// misalignment.
pub fn leaf_variants(base: &[f64], actions: &[Action]) -> Option<LeafSet> {
    if base.len() != actions.len() || base.is_empty() {
        return None;
    }
    let passive_idx = actions
        .iter()
        .position(|a| matches!(a, Action::Call))
        .or_else(|| actions.iter().position(|a| matches!(a, Action::Check)));
    let fold_idx = actions
        .iter()
        .position(|a| matches!(a, Action::Fold))
        .or(passive_idx);
    let call_heavy = tilt(base, actions, passive_idx, true);
    let fold_heavy = tilt(base, actions, fold_idx, false);
    Some(LeafSet {
        base: base.to_vec(),
        call_heavy,
        fold_heavy,
    })
}

/// Shift `LEAF_TILT` of the from-set's mass onto `target`. `aggressive_from`
/// selects which side donates: Bet/Raise donors for the call-heavy variant,
/// everything-but-target for the fold-heavy variant. Exact renormalization.
fn tilt(base: &[f64], actions: &[Action], target: Option<usize>, aggressive_from: bool) -> Vec<f64> {
    let mut out = base.to_vec();
    let Some(t) = target else {
        return out; // no target slot (shouldn't happen — Check/Call always legal)
    };
    let mut movable = 0.0;
    for (i, (p, a)) in base.iter().zip(actions.iter()).enumerate() {
        if i == t {
            continue;
        }
        let donor = if aggressive_from {
            matches!(a, Action::Bet { .. } | Action::Raise { .. })
        } else {
            true
        };
        if donor {
            movable += p;
        }
    }
    let shift = movable * LEAF_TILT;
    if shift <= 0.0 {
        return out;
    }
    // Remove proportionally from donors so relative donor ratios are preserved.
    for (i, (p, a)) in base.iter().zip(actions.iter()).enumerate() {
        if i == t {
            continue;
        }
        let donor = if aggressive_from {
            matches!(a, Action::Bet { .. } | Action::Raise { .. })
        } else {
            true
        };
        if donor {
            out[i] = p - shift * (p / movable);
        }
    }
    out[t] = base[t] + shift;
    // Exact renormalization against float drift.
    let total: f64 = out.iter().sum();
    if total > 0.0 {
        for v in out.iter_mut() {
            *v /= total;
        }
    }
    out
}

impl LeafSet {
    /// Combinator blend for the in-subgame solver: `w` (normalized by the
    /// caller — the solver's mixture weights over {base, call-heavy,
    /// fold-heavy}) selects the effective leaf prior. Renormalized exactly.
    pub fn blend(&self, w: [f64; 3]) -> Vec<f64> {
        let n = self.base.len();
        let mut out = vec![0.0; n];
        for i in 0..n {
            out[i] = w[0] * self.base[i] + w[1] * self.call_heavy[i] + w[2] * self.fold_heavy[i];
        }
        let total: f64 = out.iter().sum();
        if total > 1e-12 {
            for v in out.iter_mut() {
                *v /= total;
            }
        }
        out
    }
}

/// Class collapse from weighted combos: sort by strength, bucket into `k` classes
/// with normalized weights (deterministic; card removal applied upstream — the
/// caller passes dead-card-adjusted combo weights).
pub fn collapse_to_classes(
    mut weighted: Vec<(f64 /*weight*/, f64 /*strength*/)>,
    k: usize,
) -> Vec<crate::subgame::Class> {
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
            classes.push(crate::subgame::Class {
                weight: w_sum / total,
                strength: s_sum / w_sum,
            });
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

#[cfg(test)]
mod leaf_tests {
    use super::*;

    fn facing_bet() -> (Vec<f64>, Vec<Action>) {
        (
            vec![0.1, 0.3, 0.4, 0.2], // Fold, Call, Raise, Jam-as-Raise
            vec![
                Action::Fold,
                Action::Call,
                Action::Raise { to: 400 },
                Action::Raise { to: 1000 },
            ],
        )
    }

    #[test]
    fn variants_conserve_mass_and_shift() {
        let (base, acts) = facing_bet();
        let set = leaf_variants(&base, &acts).expect("shapes match");
        for v in [&set.base, &set.call_heavy, &set.fold_heavy] {
            let t: f64 = v.iter().sum();
            assert!((t - 1.0).abs() < 1e-12, "exact renormalization");
            assert!(v.iter().all(|&p| p >= 0.0), "no negative mass");
        }
        assert!(
            set.call_heavy[1] > base[1],
            "call-heavy gains Call mass: {} > {}",
            set.call_heavy[1],
            base[1]
        );
        assert!(
            set.fold_heavy[0] > base[0],
            "fold-heavy gains Fold mass: {} > {}",
            set.fold_heavy[0],
            base[0]
        );
    }

    #[test]
    fn no_bet_facing_falls_back_to_check() {
        let base = vec![0.5, 0.3, 0.2]; // Check, Bet, Jam-as-Bet
        let acts = vec![Action::Check, Action::Bet { to: 200 }, Action::Bet { to: 1000 }];
        let set = leaf_variants(&base, &acts).expect("shapes match");
        assert!(set.call_heavy[0] > base[0], "passive target is Check");
        assert!(set.fold_heavy[0] > base[0], "weak target falls back to Check");
    }

    #[test]
    fn blend_recovers_corners_and_renormalizes() {
        let (base, acts) = facing_bet();
        let set = leaf_variants(&base, &acts).expect("shapes match");
        let b = set.blend([1.0, 0.0, 0.0]);
        for (x, y) in b.iter().zip(set.base.iter()) {
            assert!((x - y).abs() < 1e-12);
        }
        let m = set.blend([0.5, 0.25, 0.25]);
        let t: f64 = m.iter().sum();
        assert!((t - 1.0).abs() < 1e-12);
    }

    #[test]
    fn shape_mismatch_is_none() {
        assert!(leaf_variants(&[0.5, 0.5], &[Action::Check]).is_none());
        assert!(leaf_variants(&[], &[]).is_none());
    }
}
