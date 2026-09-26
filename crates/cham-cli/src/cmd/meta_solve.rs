//! EXP-018 meta-solve: Nash mixture over the agent zoo from ledger A/B rows.

pub fn run(modes_csv: &str, ledger_path: &str) -> i32 {
    let modes: Vec<&str> = modes_csv.split(',').map(str::trim).collect();
    let path = std::path::Path::new(ledger_path);
    let txt = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("meta-solve: cannot read ledger '{ledger_path}': {e}");
            return crate::cmd::EXIT_BUDGET;
        }
    };
    // (a_mode, b_mode, delta_mb) triples for kind == "ab".
    let mut sum = vec![vec![0.0; modes.len()]; modes.len()];
    let mut count = vec![vec![0u32; modes.len()]; modes.len()];
    for line in txt.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("type").and_then(|k| k.as_str()) != Some("ab") {
            continue;
        }
        let delta = match v.get("delta_mb").and_then(|d| d.as_f64()) {
            Some(d) => d,
            None => continue,
        };
        let mode_of = |x: &serde_json::Value| -> Option<String> {
            if let Some(s) = x.as_str() {
                return Some(s.to_string());
            }
            x.get("agent")
                .or_else(|| x.get("mode"))
                .and_then(|m| m.as_str())
                .map(String::from)
        };
        let (Some(a), Some(b)) = (
            v.get("a").and_then(mode_of),
            v.get("b").and_then(mode_of),
        ) else {
            continue;
        };
        if let (Some(i), Some(j)) = (
            modes.iter().position(|&m| m == a),
            modes.iter().position(|&m| m == b),
        ) {
            sum[i][j] += delta;
            sum[j][i] -= delta;
            count[i][j] += 1;
            count[j][i] += 1;
        }
    }
    let n = modes.len();
    let m: Vec<Vec<f64>> = (0..n)
        .map(|i| {
            (0..n)
                .map(|j| {
                    if count[i][j] > 0 {
                        sum[i][j] / count[i][j] as f64
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect();
    let covered = count
        .iter()
        .flatten()
        .filter(|&&c| c > 0)
        .count();
    println!("meta-solve: {} modes, {covered} covered cells of {}", modes.join(","), n * n);
    match cham_search::oracle::solve_matrix(&m) {
        Some((v, row, _col)) => {
            let mix: Vec<String> = modes
                .iter()
                .zip(row.iter())
                .map(|(m, w)| format!("{m}: {w:.2}"))
                .collect();
            println!("meta-solve Nash value {v:+.1} mb/seating; mixture [{mix}]", mix = mix.join(", "));
            crate::cmd::EXIT_OK
        }
        None => {
            eprintln!("meta-solve: solve_matrix refused (>5x5 or degenerate) — restrict to ≤5 modes");
            crate::cmd::EXIT_FAIL
        }
    }
}
