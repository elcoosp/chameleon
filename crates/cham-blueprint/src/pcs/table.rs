//! Regret and strategy-sum storage for the PCS trainer.
//!
//! Keyed by `InfoSetKey` (the encoder's `u64` key, high bit set). One
//! `Row` per infoset per player. Row width is the node's action count
//! (2..4 for the tiny ladder). The `HashMap` is `std`'s for now —
//! the tabular trainer uses a specialized table; the PCS one can be
//! swapped in once profiling shows it matters.

use rustc_hash::FxHashMap;

/// Per-infoset regret vector and strategy sum.
#[derive(Clone, Debug)]
pub struct Row {
    /// Non-negative regret per action (regret matching operates on `R^+`).
    pub regret: Vec<f64>,
    /// Cumulative strategy sum, discounted by DCFR gamma.
    pub strategy_sum: Vec<f64>,
    /// Number of visits (for diagnostics; not used by the update).
    pub visits: u64,
}

impl Row {
    pub fn new(width: usize) -> Self {
        Row {
            regret: vec![0.0; width],
            strategy_sum: vec![0.0; width],
            visits: 0,
        }
    }

    /// Regret matching: `sigma[a] = R^+[a] / sum(R^+)`, uniform if sum is 0.
    pub fn current_strategy(&self) -> Vec<f64> {
        let w = self.regret.len();
        if w == 0 {
            return Vec::new();
        }
        let sum: f64 = self.regret.iter().copied().filter(|&r| r > 0.0).sum();
        if sum <= 0.0 {
            return vec![1.0 / w as f64; w];
        }
        self.regret
            .iter()
            .map(|&r| if r > 0.0 { r / sum } else { 0.0 })
            .collect()
    }

    /// Write the current strategy into `out` without allocating. The
    /// caller supplies a reused buffer; `out` is truncated to `w` if
    /// it is longer, and its length must be >= `w`.
    pub fn current_strategy_into(&self, out: &mut [f64]) -> usize {
        let w = self.regret.len();
        debug_assert!(out.len() >= w, "buffer too short: {} < {w}", out.len());
        let sum: f64 = self.regret.iter().copied().filter(|&r| r > 0.0).sum();
        if sum <= 0.0 {
            let u = 1.0 / w as f64;
            for v in out[..w].iter_mut() {
                *v = u;
            }
        } else {
            for i in 0..w {
                out[i] = if self.regret[i] > 0.0 {
                    self.regret[i] / sum
                } else {
                    0.0
                };
            }
        }
        w
    }
}

/// Storage for all infosets across a training run.
#[derive(Debug, Default)]
pub struct RegretTable {
    rows: FxHashMap<u64, Row>,
}

impl RegretTable {
    pub fn new() -> Self {
        RegretTable {
            rows: FxHashMap::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Get or insert a row of width `w`. If the key exists with a
    /// different width, this is a caller bug (the same infoset must
    /// always have the same action count).
    pub fn row_mut(&mut self, key: u64, w: usize) -> &mut Row {
        self.rows.entry(key).or_insert_with(|| Row::new(w))
    }

    pub fn row(&self, key: u64) -> Option<&Row> {
        self.rows.get(&key)
    }

    /// Iterate over every (key, row) pair. Used by the artifact bridge.
    pub fn iter(&self) -> impl Iterator<Item = (u64, &Row)> + '_ {
        self.rows.iter().map(|(&k, r)| (k, r))
    }

    /// Convert to the tabular `crate::table::RegretTable` so the artifact
    /// writer (`BlueprintPolicy::build_artifact`) can consume it.
    ///
    /// The tabular table stores (regret, strat_sum, avg_weight, visits)
    /// per row and normalizes `strat_sum` in `avg_strategy`. We populate
    /// it with the PCS row's *normalized average strategy* as the strat
    /// sum: since `avg_strategy` divides by the total (which is exactly
    /// 1.0 here), the loaded distribution round-trips unchanged. Visits
    /// are set from the PCS row's visit count, capped at u16::MAX by
    /// `build_artifact` itself.
    ///
    /// Rows with width outside [1, 12] are dropped (`entry_or_insert`
    /// debug-asserts that range). The tiny ladder produces widths 2-4,
    /// so this only matters for hand-built toy trees.
    pub fn to_tabular(&self, mode: crate::table::ThreadMode) -> crate::table::RegretTable {
        let mut dst = crate::table::RegretTable::new(mode);
        for (key, row) in self.iter() {
            let w = row.strategy_sum.len();
            if w == 0 || w > 12 {
                continue;
            }
            let total: f64 = row.strategy_sum.iter().sum();
            let avg: Vec<f64> = if total > 0.0 {
                row.strategy_sum.iter().map(|&s| s / total).collect()
            } else {
                vec![1.0 / w as f64; w]
            };
            let (off, w_ret) = dst.entry_or_insert(key, w);
            debug_assert_eq!(w, w_ret);
            for (a, &p) in avg.iter().enumerate() {
                dst.strat_add(off, w, a, p);
            }
            let v = row.visits.max(1).min(u32::MAX as u64) as u32;
            dst.add_visits(off, w, v);
        }
        dst
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_strategy_on_zero_regret() {
        let r = Row::new(3);
        let s = r.current_strategy();
        assert_eq!(s.len(), 3);
        for x in s {
            assert!((x - 1.0 / 3.0).abs() < 1e-12);
        }
    }

    #[test]
    fn regret_matching_proportional() {
        let mut r = Row::new(3);
        r.regret = vec![1.0, 2.0, 0.0];
        let s = r.current_strategy();
        assert!((s[0] - 1.0 / 3.0).abs() < 1e-12);
        assert!((s[1] - 2.0 / 3.0).abs() < 1e-12);
        assert!((s[2]).abs() < 1e-12);
    }

    #[test]
    fn negative_regret_is_clamped() {
        let mut r = Row::new(2);
        r.regret = vec![-1.0, 2.0];
        let s = r.current_strategy();
        assert!((s[0]).abs() < 1e-12);
        assert!((s[1] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn table_inserts_and_looks_up() {
        let mut t = RegretTable::new();
        t.row_mut(0x8000_0000_0000_0001, 3);
        t.row_mut(0x8000_0000_0000_0002, 4);
        assert_eq!(t.len(), 2);
        assert_eq!(t.row(0x8000_0000_0000_0001).unwrap().regret.len(), 3);
        assert_eq!(t.row(0x8000_0000_0000_0002).unwrap().regret.len(), 4);
        assert!(t.row(0x8000_0000_0000_0003).is_none());
    }
}
