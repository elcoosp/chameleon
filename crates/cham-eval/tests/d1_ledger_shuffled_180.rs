//! One-shot: append the 180-board shuffled D1 entry.

use cham_eval::ledger::{Ledger, LedgerEntry};
use std::path::Path;

#[test]
#[ignore = "one-shot"]
fn append_d1_shuffled_180() {
    let repo = std::env::var("CHAM_REPO").unwrap_or_else(|_| ".".into());
    let ledger_dir = Path::new(&repo).join("artifacts/ledger");
    let mut l = Ledger::open(&ledger_dir).expect("open");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = LedgerEntry {
        ts,
        run: "d1-fullgame-vbr/agent-honest-19dim/shuffled-180".into(),
        kind: "d1-vbr".into(),
        a: serde_json::json!({
            "vbr_bb": 8.5196,
            "se_bb": 0.6208,
            "boards": 180,
            "combos_h": 30,
            "combos_v": 30,
            "policy_miss_pct": 1.52,
            "blueprint": "artifacts/agent-honest-19dim/robust",
            "vbr_kind": "full-game",
            "range_construction": "shuffled (post af1b635)",
            "d2_baseline_3se": 8.5196 - 3.0 * 0.6208,
        }),
        b: None,
        delta_mb: None,
        ci: Some((8.5196 - 0.6208, 8.5196 + 0.6208)),
        sprt: None,
        promote: false,
        seatings: 180,
        artifact_hash: None,
        notes: Some("180-board shuffled D1; settles the magnitude question against the lexicographic 5.77 +/- 0.55".into()),
    };
    l.append(&entry).expect("append");
    eprintln!("appended");
}
