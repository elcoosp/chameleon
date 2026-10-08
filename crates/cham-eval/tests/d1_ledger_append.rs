//! One-shot: append the D1 result to the ledger via the real API.
//!
//! The artifact hash is computed in the shell (`b3sum` or `openssl`) and
//! passed via `CHAM_ARTIFACT_HASH`, so this test does not need a direct
//! blake3 dependency.

use cham_eval::ledger::{Ledger, LedgerEntry};
use std::path::Path;

#[test]
#[ignore = "one-shot: appends D1 entry to artifacts/ledger/ledger.jsonl"]
fn append_d1_entry() {
    let repo = std::env::var("CHAM_REPO").unwrap_or_else(|_| ".".into());
    let ledger_dir = Path::new(&repo).join("artifacts/ledger");
    let artifact_hash = std::env::var("CHAM_ARTIFACT_HASH")
        .ok()
        .filter(|s| !s.is_empty());

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let vbr_bb = 6.4119_f64;
    let se_bb = 1.4953_f64;

    let entry = LedgerEntry {
        ts,
        run: "d1-fullgame-vbr/agent-honest-19dim".into(),
        kind: "d1-vbr".into(),
        a: serde_json::json!({
            "vbr_bb": vbr_bb,
            "se_bb": se_bb,
            "boards": 20,
            "combos_h": 30,
            "combos_v": 30,
            "policy_miss_pct": 0.49,
            "blueprint": "artifacts/agent-honest-19dim/robust",
            "abstraction": "artifacts/agent-honest-19dim/abstraction.toml",
            "vbr_kind": "full-game",
            "river_only_bb_for_compare": 5.890,
            "tabular_br_bb_for_compare": 8.19,
            "harness": "crates/cham-agent/tests/d1_fullgame_vbr.rs",
        }),
        b: None,
        delta_mb: None,
        ci: Some((vbr_bb - se_bb, vbr_bb + se_bb)),
        sprt: None,
        promote: false,
        seatings: 20,
        artifact_hash,
        notes: Some(
            "Decision D1: full-game VBR vs shipped blueprint (honest ruler). \
             Positive = perfect BR beats blueprint. Earlier streets add ~0.5 bb \
             over river-only; tabular BR (8.19) was a metric artifact. \
             Hash over policy.bin only (labelled sha256 if blake3 unavailable)."
                .into(),
        ),
    };

    let mut l = Ledger::open(&ledger_dir).expect("open ledger");
    l.append(&entry).expect("append");
    eprintln!("appended D1 entry to {}", ledger_dir.display());
}
