//! `chameleon trace` (SPECS/09 §2): textual decision traces — **no animation**
//! (v1's replay animation is cut). Reads an instrumented run's events.jsonl,
//! validates it against the SPECS/12 registry, and prints a summary plus the
//! top-N decision records ranked by the requested metric.

pub fn run(run: &str, top: usize, by: &str) -> i32 {
    let path = match locate_run(run) {
        Some(p) => p,
        None => {
            eprintln!("trace: no events.jsonl for run '{run}' under artifacts/runs/");
            return crate::cmd::EXIT_FAIL;
        }
    };
    // schema gate first: an invalid record stream stops the command (SPECS/12)
    if let Err(e) = cham_rec::validate::validate_file(&path) {
        eprintln!("trace: record stream invalid: {e}");
        return crate::cmd::EXIT_FAIL;
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("trace: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    let mut decisions: Vec<(u64, serde_json::Value)> = Vec::new();
    let mut kinds: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for (lineno, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("trace: line {} unparseable: {e}", lineno + 1);
                return crate::cmd::EXIT_FAIL;
            }
        };
        let kind = v["kind"].as_str().unwrap_or("?").to_string();
        *kinds.entry(kind).or_insert(0) += 1;
        if v["kind"] == "decision" {
            decisions.push((v["seq"].as_u64().unwrap_or(0), v["data"].clone()));
        }
    }
    let fallbacks = decisions
        .iter()
        .filter(|(_, d)| d["fallback_used"].as_bool() == Some(true))
        .count();
    let searched = decisions
        .iter()
        .filter(|(_, d)| {
            d["search"]
                .as_object()
                .map(|s| s["triggered"].as_bool() == Some(true))
                .unwrap_or(false)
        })
        .count();
    println!(
        "trace {run}: {} records, kinds {:?}",
        text.lines().count(),
        kinds
    );
    println!(
        "trace {run}: {} decisions, {fallbacks} fallback_used, {searched} searches triggered",
        decisions.len()
    );

    let metric = |d: &serde_json::Value| -> f64 {
        match by {
            // fallback: rank decisions where the mixture fell back highest
            "fallback" => {
                if d["fallback_used"].as_bool() == Some(true) {
                    1.0
                } else {
                    0.0
                }
            }
            // search: rank by solver work invested
            "search" => d["search"]["iters"].as_f64().unwrap_or(0.0),
            // lbr: rank by the solver's local best-response gap
            "lbr" => d["search"]["lbr_gap_ours"].as_f64().unwrap_or(0.0).abs(),
            _ => 0.0,
        }
    };
    decisions.sort_by(|a, b| {
        metric(&b.1)
            .partial_cmp(&metric(&a.1))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    println!("top {top} by {by}:");
    for (seq, d) in decisions.iter().take(top) {
        let action = d["action"].as_str().unwrap_or("?");
        let street = d["street"].as_u64().unwrap_or(0);
        let hand = d["hand_idx"].as_u64().unwrap_or(0);
        let fb = d["fallback_used"].as_bool() == Some(true);
        let search = d["search"]["solver"].as_str().unwrap_or("-");
        println!(
            "  seq{seq:>5} hand {hand:>4} street {street} action {action:<10} search {search:<8} fallback {fb}"
        );
    }
    crate::cmd::EXIT_OK
}

/// Resolve a run id (exact dir name, or `latest`) to its events file.
fn locate_run(run: &str) -> Option<std::path::PathBuf> {
    let runs = std::path::Path::new("artifacts/runs");
    if run == "latest" {
        let mut best: Option<(String, std::path::PathBuf)> = None;
        if let Ok(rd) = std::fs::read_dir(runs) {
            for e in rd.flatten() {
                let p = e.path().join("events.jsonl");
                if p.exists() {
                    let name = e.file_name().to_string_lossy().to_string();
                    if best.as_ref().is_none_or(|(n, _)| name > *n) {
                        best = Some((name, p));
                    }
                }
            }
        }
        return best.map(|(_, p)| p);
    }
    let direct = runs.join(run).join("events.jsonl");
    direct.exists().then_some(direct)
}
