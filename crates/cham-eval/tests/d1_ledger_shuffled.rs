//! One-shot: append the shuffled-range D1 entries.

use cham_eval::ledger::{Ledger, LedgerEntry};
use std::path::Path;

#[test]
#[ignore = "one-shot"]
fn append_d1_shuffled() {
    let repo = std::env::var("CHAM_REPO").unwrap_or_else(|_| ".".into());
    let ledger_dir = Path::new(&repo).join("artifacts/ledger");
    let mut l = Ledger::open(&ledger_dir).expect("open");

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let entries = [
        (
            "d1-fullgame-vbr/agent-honest-19dim/shuffled",
            "artifacts/agent-honest-19dim/robust",
            7.5720_f64,
            1.5362_f64,
            0.57_f64,
            "supersedes d1-fullgame-vbr/agent-honest-19dim/tight (5.77 lexicographic); higher by ~1.1 sigma",
        ),
        (
            "d1-fullgame-vbr/blueprints-tiny-full/robust-7/shuffled",
            "artifacts/blueprints-tiny-full/robust/robust-7/policy",
            9.7523_f64,
            1.5768_f64,
            0.37_f64,
            "supersedes d1-fullgame-vbr/blueprints-tiny-full/robust-7 (7.65 lexicographic); higher by ~1.3 sigma",
        ),
    ];

    for (run, bp, vbr, se, miss, note) in entries {
        let entry = LedgerEntry {
            ts,
            run: run.into(),
            kind: "d1-vbr".into(),
            a: serde_json::json!({
                "vbr_bb": vbr,
                "se_bb": se,
                "boards": 20,
                "combos_h": 30,
                "combos_v": 30,
                "policy_miss_pct": miss,
                "blueprint": bp,
                "vbr_kind": "full-game",
                "range_construction": "shuffled (post af1b635)",
            }),
            b: None,
            delta_mb: None,
            ci: Some((vbr - se, vbr + se)),
            sprt: None,
            promote: false,
            seatings: 20,
            artifact_hash: None,
            notes: Some(note.into()),
        };
        l.append(&entry).expect("append");
    }
    eprintln!("appended shuffled D1 entries");
}
