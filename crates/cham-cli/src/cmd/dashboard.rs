//! `chameleon dashboard` (SPECS/09 §2): the trimmed 4-section static dashboard
//! (SPECS/08 §8): headline/ledger, per-opponent winrate table, frontier chart,
//! ledger table. Static HTML + inline SVG; no JS deps, no animation.

pub fn run(out: &str, last: usize) -> i32 {
    let ledger_dir = std::path::Path::new("artifacts/ledger");
    let entries = match cham_eval::Ledger::open(ledger_dir).and_then(|l| l.entries()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("dashboard: no usable ledger under artifacts/ledger: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    if entries.is_empty() {
        eprintln!("dashboard: ledger is empty — run ladder/ab first");
        return crate::cmd::EXIT_FAIL;
    }
    // aggregate the most recent `last` entries into the winrate table
    let recent: Vec<&cham_eval::ledger::LedgerEntry> = entries.iter().rev().take(last).collect();
    let payloads: Vec<serde_json::Value> = recent
        .iter()
        .map(|e| {
            serde_json::json!({
                "label": e.run,
                "mb_per_seating": e.delta_mb.unwrap_or(0.0),
                "se_mb": 0.0,
                "seatings": e.seatings,
            })
        })
        .collect();
    let summary = cham_eval::ingest::ingest_matches(&payloads);
    // frontier points: (exploitability lbr_mb, achieved delta_mb) where both exist
    let frontier: Vec<(f64, f64)> = entries
        .iter()
        .filter_map(|e| {
            let lbr = e.a.get("lbr_mb").and_then(|v| v.as_f64())?;
            let delta = e.delta_mb?;
            Some((lbr, delta))
        })
        .collect();
    let ledger_rows: Vec<(String, String, f64)> = recent
        .iter()
        .map(|e| (e.run.clone(), e.kind.clone(), e.delta_mb.unwrap_or(0.0)))
        .collect();
    let html = cham_eval::dashboard::render(&summary, &frontier, &ledger_rows);
    let out_path = std::path::Path::new(out);
    if let Some(parent) = out_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("dashboard: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    }
    match std::fs::write(out_path, html) {
        Ok(()) => {
            println!(
                "dashboard: {} ledger entries → {out} (4 sections: headline, winrates, frontier, ledger)",
                entries.len()
            );
            crate::cmd::EXIT_OK
        }
        Err(e) => {
            eprintln!("dashboard: {e}");
            crate::cmd::EXIT_FAIL
        }
    }
}
