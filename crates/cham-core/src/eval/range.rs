//! `Range` — 1326 combo bits over `[u64; 21]` (SPECS/01 §3). v1's `[u64; 3]` was
//! 192 bits — wrong. Combo ids use the triangular `Hand2::combo_id` indexing.

use serde::{Deserialize, Serialize};

use crate::card::Hand2;

/// 1326 bits = 21 × u64.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Range(pub [u64; 21]);



impl Range {
    /// All 1326 combos on.
    pub fn all() -> Range {
        let mut r = Range([0u64; 21]);
        for i in 0..1326 {
            r.set(i, true);
        }
        r
    }
    pub fn set(&mut self, combo: usize, on: bool) {
        debug_assert!(combo < 1326, "combo id out of range");
        let word = combo / 64;
        let bit = combo % 64;
        if on {
            self.0[word] |= 1u64 << bit;
        } else {
            self.0[word] &= !(1u64 << bit);
        }
    }
    pub fn get(&self, combo: usize) -> bool {
        debug_assert!(combo < 1326, "combo id out of range");
        self.0[combo / 64] & (1u64 << (combo % 64)) != 0
    }
    pub fn count(&self) -> u32 {
        self.0.iter().map(|w| w.count_ones()).sum()
    }
    /// Top `n` fraction of combos by the pinned preflop class ordering
    /// (class strength heuristic: pairs > suited > offsuit, higher ranks first).
    pub fn top(n: f64) -> Range {
        Self::from_percent(n * 100.0)
    }
    /// Top `p` percent by the pinned ordering (deterministic).
    pub fn from_percent(p: f64) -> Range {
        let p = p.clamp(0.0, 100.0);
        let want = ((p / 100.0) * 1326.0).round() as usize;
        let mut order: Vec<(u8, usize)> = (0..1326).map(|c| (Hand2::from_combo(c).class_id(), c)).collect();
        order.sort_unstable(); // by class id (pinned strength order)
        let mut r = Range::default();
        for (_, c) in order.into_iter().take(want) {
            r.set(c, true);
        }
        r
    }
    /// Iterate set combo ids in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        (0..1326).filter(move |&c| self.get(c))
    }
    /// Clear every combo containing one of `dead` cards.
    pub fn remove_cards(&mut self, dead: &[crate::card::Card]) {
        for combo in 0..1326 {
            let [a, b] = Hand2::from_combo(combo).cards();
            if dead.iter().any(|d| d.idx() == a.idx() || d.idx() == b.idx()) {
                self.set(combo, false);
            }
        }
    }
}
