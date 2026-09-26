//! `chameleon lint-ledger` (v3 §2.2, brainstorm C2): pre-registration gate.
//!
//! A gate run is declared BEFORE it happens in a small TOML:
//!
//! ```toml
//! [gate]
//! run_prefix = "ab-full-robust-only"  # ledger `run` must start with this
//! kind = "ab"                         # ledger `type` must equal this
//! min_seatings = 8000                 # entry.seatings >= this
//! require_promote = true              # entry.promote must be true
//! # artifact_hash = "blake3:..."      # optional: entry.artifact_hash must contain this
//! ```
//!
//! The linter finds the LATEST ledger entry matching `run_prefix` + `kind`
//! and checks seatings / promote / artifact-hash. Exit 0 = gate satisfied,
//! exit 2 = gate NOT satisfied (same budget-refusal discipline as
//! `cmd::guard` — a failed gate is a refusal, not a crash), exit 1 = I/O or
//! config error. This is the cheapest insurance against "gate shopping":
//! the pass condition is committed before the number exists.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct PreregFile {
    gate: PreregGate,
}

#[derive(Debug, Deserialize)]
struct PreregGate {
    run_prefix: String,
    kind: String,
    min_seatings: u64,
    #[serde(default)]
    require_promote: bool,
    #[serde(default)]
    artifact_hash: Option<String>,
}

pub fn run(prereg: &str, ledger_dir: &str) -> i32 {
    let text = match std::fs::read_to_string(prereg) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("lint-ledger: cannot read prereg '{prereg}': {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    let pre: PreregFile = match toml::from_str(&text) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("lint-ledger: bad prereg TOML '{prereg}': {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    let ledger = match cham_eval::Ledger::open(std::path::Path::new(ledger_dir)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("lint-ledger: ledger: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    let entries = match ledger.entries() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("lint-ledger: ledger: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    let latest = entries
        .iter()
        .rev()
        .find(|e| e.kind == pre.gate.kind && e.run.starts_with(&pre.gate.run_prefix));
    let Some(entry) = latest else {
        eprintln!(
            "lint-ledger: GATE UNSATISFIED — no '{}' entry with run prefix '{}' in {ledger_dir}",
            pre.gate.kind, pre.gate.run_prefix
        );
        return crate::cmd::EXIT_BUDGET;
    };
    let mut failures: Vec<String> = Vec::new();
    if entry.seatings < pre.gate.min_seatings {
        failures.push(format!(
            "seatings {} < min_seatings {}",
            entry.seatings, pre.gate.min_seatings
        ));
    }
    if pre.gate.require_promote && !entry.promote {
        failures.push("require_promote but entry.promote = false".into());
    }
    if let Some(want) = &pre.gate.artifact_hash {
        match &entry.artifact_hash {
            Some(got) if got.contains(want) => {}
            Some(got) => failures.push(format!("artifact_hash mismatch: want '{want}' in '{got}'")),
            None => failures.push(format!(
                "artifact_hash required ('{want}') but entry has none — unauditable"
            )),
        }
    }
    if failures.is_empty() {
        println!(
            "lint-ledger: GATE SATISFIED — run '{}' ({} seatings, promote={}, artifact_hash={})",
            entry.run,
            entry.seatings,
            entry.promote,
            entry.artifact_hash.as_deref().unwrap_or("none"),
        );
        crate::cmd::EXIT_OK
    } else {
        eprintln!(
            "lint-ledger: GATE UNSATISFIED — run '{}': {}",
            entry.run,
            failures.join("; ")
        );
        crate::cmd::EXIT_BUDGET
    }
}
