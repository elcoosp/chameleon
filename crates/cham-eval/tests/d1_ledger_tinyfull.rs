//! One-shot: append the tiny-full D1 entry to the ledger.

use cham_eval::ledger::{Ledger, LedgerEntry};
use std::path::Path;

#[test]
#[ignore = "one-shot: appends tiny-full D1 entry to artifacts/ledger/ledger.jsonl"]
fn append_d1_tinyfull() {
    let repo = std::env::var("CHAM_REPO").unwrap_or_else(|_| ".".into());
    let ledger_dir = Path::new(&repo).join("artifacts/ledger");
    let artifact_hash = std::env::var("CHAM_ARTIFACT_HASH")
        .ok()
        .filter(|s| !s.is_empty());

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let vbr_bb = 7.6479_f64;
    let se_bb = 0.9237_f64;

    let entry = LedgerEntry {
        ts,
        run: "d1-fullgame-vbr/blueprints-tiny-full/robust-7".into(),
        kind: "d1-vbr".into(),
        a: serde_json::json!({
            "vbr_bb": vbr_bb,
            "se_bb": se_bb,
            "boards": 60,
            "combos_h": 30,
            "combos_v": 30,
            "policy_miss_pct": 1.06,
            "blueprint": "artifacts/blueprints-tiny-full/robust/robust-7/policy",
            "abstraction": "config/abstraction-tiny.toml",
            "vbr_kind": "full-game",
            "tabular_br_bb_for_compare": 8.19,
            "harness": "crates/cham-agent/tests/d1_fullgame_vbr.rs",
            "env_note": "bundle trained with CHAM_SLOT_BUCKET=1; D1 must run with it set (miss 99.48% without, 1.06% with)",
            "supersedes": null,
            "d2_baseline_3se": null,
        }),
        b: None,
        delta_mb: None,
        ci: Some((vbr_bb - se_bb, vbr_bb + se_bb)),
        sprt: None,
        promote: false,
        seatings: 60,
        artifact_hash,
        notes: Some(
            "Decision D1, plan-mandated second bundle (docs/reviews/CHAMELEON-SOTA-PLAN.md §Decision D1). \
             VBR ~ tabular BR (8.19) within 1 SE => outcome 1: the tiny-full abstraction really is that bad, \
             Phase C is mandatory for this bundle. Contrast with agent-honest-19dim (5.77 +/- 0.55), where \
             the tabular BR over-estimated. Both numbers are needed to interpret D1."
                .into(),
        ),
    };

    let mut l = Ledger::open(&ledger_dir).expect("open ledger");
    l.append(&entry).expect("append");
    eprintln!("appended tiny-full D1 entry");
}
