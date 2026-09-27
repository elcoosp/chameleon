//! Event ingestion + aggregates (SPECS/08): events.jsonl → winrate tables.

use std::collections::BTreeMap;

/// Aggregate match records by opponent label.
#[derive(Clone, Debug, Default)]
pub struct IngestSummary {
    /// label → (mean mb/seating, SE of the mean, total seatings)
    pub by_label: BTreeMap<String, (f64, f64, u64)>,
}

/// Aggregate a set of cham-rec `match` payloads.
///
/// L-12 fix (2026-09-27): the previous code OVERWROTE a label's mean/se with
/// the LAST record while SUMMING the seatings — a dashboard over chunked
/// runs showed "mean from the last chunk, se from the last chunk, seatings
/// over all chunks", an internally inconsistent pair. Now we accumulate
/// seatings-weighted first and second moments locally and derive the
/// combined mean and SE over the total seatings.
pub fn ingest_matches(payloads: &[serde_json::Value]) -> IngestSummary {
    // local per-label accumulation:
    //   (Σ x·s,  Σ (x² + se²)·s,  Σ s)   where s = seatings, x = mb_per_seating
    // E[X²] uses the record's own `se` to reconstruct the within-record
    // second moment; that is exactly the minimum-variance combination when
    // the records report (mean, se, N).
    let mut moments: BTreeMap<String, (f64, f64, u64)> = BTreeMap::new();
    for p in payloads {
        if p.get("label").is_none() || p.get("mb_per_seating").is_none() {
            continue;
        }
        let label = p["label"].as_str().unwrap_or("?").to_string();
        let mb = p["mb_per_seating"].as_f64().unwrap_or(0.0);
        let se = p["se_mb"].as_f64().unwrap_or(0.0);
        let seatings = p["seatings"].as_u64().unwrap_or(0);
        let s = seatings.max(1) as f64;
        let entry = moments.entry(label).or_insert((0.0, 0.0, 0));
        entry.0 += mb * s;
        // E[X²] ≈ x² + se² when the record's SE reflects its own sampling
        // variance around x. Weighted by seatings.
        entry.1 += (mb * mb + se * se) * s;
        entry.2 += seatings;
    }

    let mut by_label = BTreeMap::new();
    for (label, (sum_mb_s, sum_sq_s, n)) in moments {
        if n == 0 {
            by_label.insert(label, (0.0, 0.0, 0u64));
            continue;
        }
        let mean = sum_mb_s / n as f64;
        let ex2 = sum_sq_s / n as f64;
        let var = (ex2 - mean * mean).max(0.0);
        let se = (var / n as f64).sqrt();
        by_label.insert(label, (mean, se, n));
    }
    IngestSummary { by_label }
}
