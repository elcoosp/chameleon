//! Robust warm-start (SPECS/04 §7): key-exact transfer from the robust training
//! table — `regret_dst = robust_regret` (unscaled; shared game), `strat_sum_dst =
//! robust_strat × 0.1` (prior weight), `visits_dst = robust_visits`.
//!
//! The depth ladder (10→20→40→80→200 bb) is CUT from the default recipe
//! (EXP-006 is the pre-registered experiment that may revive it).

use crate::table::RegretTable;

/// Transfer every robust row whose key exists in `dst`'s key space — actually the
/// reverse direction per spec: keys are exact, so every robust key is inserted into
/// `dst` if missing (strategy stays close to robust at t=0; tested by
/// `warmstart_exact_keys`).
pub fn warmstart_from_robust(src: &RegretTable, dst: &mut RegretTable) {
    let entries: Vec<(u64, u32, usize)> = src.iter().map(|(k, off)| (k, off, src.row_width(off))).collect();
    for (key, src_off, w) in entries {
        let (dst_off, _) = dst.entry_or_insert(key, w);
        for a in 0..w {
            let r = src.regret(src_off, w, a);
            dst.regret_add(dst_off, a, r);
        }
        for a in 0..w {
            let s = src.strat(src_off, w, a) * 0.1; // 0.1 prior weight
            dst.strat_add(dst_off, w, a, s);
        }
        let wgt = src.avg_weight(src_off, w) * 0.1;
        dst.add_weight(dst_off, w, wgt);
        for _ in 0..src.visits(src_off, w) {
            dst.add_visit(dst_off, w);
        }
    }
}
