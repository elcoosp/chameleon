//! Event ingestion + aggregates (SPECS/08): events.jsonl → winrate tables.

use std::collections::BTreeMap;

/// Aggregate match records by opponent label.
#[derive(Clone, Debug, Default)]
pub struct IngestSummary {
    pub by_label: BTreeMap<String, (f64, f64, u64)>, // mb, se, seatings
}

/// Aggregate a set of cham-rec `match` payloads.
pub fn ingest_matches(payloads: &[serde_json::Value]) -> IngestSummary {
    let mut out = IngestSummary::default();
    for p in payloads {
        if p.get("label").is_none() || p.get("mb_per_seating").is_none() {
            continue;
        }
        let label = p["label"].as_str().unwrap_or("?").to_string();
        let mb = p["mb_per_seating"].as_f64().unwrap_or(0.0);
        let se = p["se_mb"].as_f64().unwrap_or(0.0);
        let seatings = p["seatings"].as_u64().unwrap_or(0);
        let e = out.by_label.entry(label).or_insert((0.0, 0.0, 0));
        e.0 = mb;
        e.1 = se;
        e.2 += seatings;
    }
    out
}
