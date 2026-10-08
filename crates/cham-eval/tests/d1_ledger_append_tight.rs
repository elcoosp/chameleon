//! One-shot: append the tighter (180-board) D1 to the ledger.

use cham_eval::ledger::{Ledger, LedgerEntry};
use std::path::Path;

#[test]
#[ignore = "one-shot: appends tighter D1 entry to artifacts/ledger/ledger.jsonl"]
fn append_d1_tight() {
    let repo = std::env::var("CHAM_REPO").unwrap_or_else(|_| ".".into());
    let ledger_dir = Path::new(&repo).join("artifacts/ledger");
    let artifact_hash = std::env::var("CHAM_ARTIFACT_HASH")
        .ok()
        .filter(|s| !s.is_empty());

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let vbr_bb = 5.7748_f64;
    let se_bb = 0.5469_f64;

    let entry = LedgerEntry {
        ts,
        run: "d1-fullgame-vbr/agent-honest-19dim/tight".into(),
        kind: "d1-vbr".into(),
        a: serde_json::json!({
            "vbr_bb": vbr_bb,
            "se_bb": se_bb,
            "boards": 180,
            "combos_h": 30,
            "combos_v": 30,
            "policy_miss_pct": 1.46,
            "blueprint": "artifacts/agent-honest-19dim/robust",
            "abstraction": "artifacts/agent-honest-19dim/abstraction.toml",
            "vbr_kind": "full-game",
            "river_only_bb_for_compare": 5.890,
            "tabular_br_bb_for_compare": 8.19,
            "harness": "crates/cham-agent/tests/d1_fullgame_vbr.rs",
            "supersedes": "d1-fullgame-vbr/agent-honest-19dim (20 boards, 6.41 +/- 1.50)",
            "d2_baseline_3se": vbr_bb - 3.0 * se_bb,
        }),
        b: None,
        delta_mb: None,
        ci: Some((vbr_bb - se_bb, vbr_bb + se_bb)),
        sprt: None,
        promote: false,
        seatings: 180,
        artifact_hash,
        notes: Some(
            "Decision D1, tightened: 180-board full-game VBR vs shipped blueprint. \
             Supersedes the 20-board 6.41 +/- 1.50. D2 threshold: trained policy \
             must beat 5.77 - 3*0.55 = 4.12 bb."
                .into(),
        ),
    };

    let mut l = Ledger::open(&ledger_dir).expect("open ledger");
    l.append(&entry).expect("append");
    eprintln!("appended tighter D1 entry to {}", ledger_dir.display());
}
