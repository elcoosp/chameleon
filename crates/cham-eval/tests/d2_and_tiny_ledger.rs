//! One-shot: ledger D2 result + tiny-full 180-board D1.

use cham_eval::ledger::{Ledger, LedgerEntry};
use std::path::Path;

#[test]
#[ignore = "one-shot"]
fn append_d2_and_tiny() {
    let repo = std::env::var("CHAM_REPO").unwrap_or_else(|_| ".".into());
    let ledger_dir = Path::new(&repo).join("artifacts/ledger");
    let mut l = Ledger::open(&ledger_dir).expect("open");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // D2 result.
    l.append(&LedgerEntry {
        ts,
        run: "d2/pcs-tiny-50k".into(),
        kind: "d1-vbr".into(),
        a: serde_json::json!({
            "vbr_bb": 9.0977,
            "se_bb": 0.6707,
            "boards": 180,
            "iters": 50000,
            "wall_s": 3721.8,
            "rows": 81354,
            "policy_miss_pct": 0.58,
            "blueprint": "artifacts/pcs-d2-4h/robust",
            "vbr_kind": "full-game",
            "range_construction": "shuffled",
            "baseline_vbr_bb": 8.5196,
            "d2_threshold_bb": 6.657,
            "verdict": "FAIL - plateau (z=0.64 vs shipped)",
        }),
        b: None,
        delta_mb: None,
        ci: Some((9.0977 - 0.6707, 9.0977 + 0.6707)),
        sprt: None,
        promote: false,
        seatings: 180,
        artifact_hash: None,
        notes: Some("Decision D2: PCS 50k iters plateaus at the shipped blueprint's level. Plan's pivot: skip to W2 (abstraction floor).".into()),
    }).expect("append d2");

    // tiny-full 180-board.
    l.append(&LedgerEntry {
        ts,
        run: "d1-fullgame-vbr/blueprints-tiny-full/robust-7/shuffled-180".into(),
        kind: "d1-vbr".into(),
        a: serde_json::json!({
            "vbr_bb": 10.7319,
            "se_bb": 0.6206,
            "boards": 180,
            "policy_miss_pct": 1.31,
            "blueprint": "artifacts/blueprints-tiny-full/robust/robust-7/policy",
            "vbr_kind": "full-game",
            "range_construction": "shuffled",
        }),
        b: None,
        delta_mb: None,
        ci: Some((10.7319 - 0.6206, 10.7319 + 0.6206)),
        sprt: None,
        promote: false,
        seatings: 180,
        artifact_hash: None,
        notes: Some("tiny-full 180-board shuffled D1; completes the D1 pair at the authoritative range width.".into()),
    }).expect("append tiny");
    eprintln!("appended 2 entries");
}
