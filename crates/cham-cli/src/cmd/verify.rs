//! `chameleon verify` (SPECS/09 §2): Tier 0.

/// Workspace invariant greps (SPECS/00 §11).
fn grep_invariants() -> Result<(), String> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let root = std::path::Path::new(manifest)
        .parent()
        .expect("workspace root");
    let forbidden: Vec<(&str, &str)> = vec![
        ("thread_rng", "SPECS/00 §11: thread_rng is forbidden"),
        ("SmallRng", "SPECS/00 §11: SmallRng is forbidden"),
        ("StdRng", "SPECS/00 §11: StdRng is forbidden"),
        ("soft_bucket", "soft buckets are CUT (review)"),
        ("depth_bands", "depth bands replaced by SPR bands"),
    ];
    let crates_dir = root.join("crates");
    let mut hits = Vec::new();
    for entry in walk_sources(&crates_dir) {
        let text = std::fs::read_to_string(&entry).unwrap_or_default();
        for (pat, why) in &forbidden {
            if text.contains(pat) {
                hits.push(format!("{}: {pat} — {why}", entry.display()));
            }
        }
    }
    if hits.is_empty() {
        Ok(())
    } else {
        Err(hits.join("\n"))
    }
}

fn walk_sources(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "target" || name == "tests" || name == "benches" {
                    continue;
                }
                out.extend(walk_sources(&p));
            } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                out.push(p);
            }
        }
    }
    out
}

pub fn run(perf: bool, count_infosets: bool, proofs: bool) -> i32 {
    let mut failures = Vec::new();

    // 1. workspace greps
    if let Err(e) = grep_invariants() {
        failures.push(format!("invariant greps:\n{e}"));
    }

    // 2. proofs (M-1 gate)
    if proofs {
        let results: Vec<cham_proofs::ProofResult> = cham_proofs::run_all(30_000);
        for r in &results {
            println!(
                "proof {}: {} — {}",
                r.id,
                if r.passed { "PASS" } else { "FAIL" },
                r.detail
            );
            if !r.passed {
                failures.push(format!("proof {} failed", r.id));
            }
        }
    }

    // 3. infoset estimate (sampling-based; M-1 spike caveat — SPECS/02 §7)
    if count_infosets {
        let estimate = estimate_infosets(500);
        println!(
            "infoset estimate (500-deal sample, extrapolated): ~{estimate:.0} distinct keys on the tiny abstraction"
        );
    }

    // 4. perf gates are ENFORCED from criterion estimates (SPECS/00 §6).
    if perf {
        check_perf_gates(&mut failures);
    }

    if failures.is_empty() {
        println!("verify: GREEN");
        crate::cmd::EXIT_OK
    } else {
        for f in &failures {
            eprintln!("verify FAIL: {f}");
        }
        crate::cmd::EXIT_FAIL
    }
}

/// Sampling-based infoset estimator: distinct keys over N random deals.
fn estimate_infosets(deals: u64) -> f64 {
    use cham_core::card::Deck;
    use cham_core::engine::State;
    use cham_core::engine::config::EngineConfig;
    use cham_core::obs::Player;
    use cham_core::rng::child;
    use cham_engine::config::AbstractionConfig;
    use cham_engine::encoder::{ActionSeq, Encoder};
    use rustc_hash::FxHashSet;

    let cfg = AbstractionConfig::tiny();
    let mut enc =
        match Encoder::from_artifacts_dir(std::path::Path::new("artifacts/buckets-tiny"), cfg) {
            Ok(e) => e,
            Err(_) => Encoder::cfg_only(AbstractionConfig::tiny()).expect("enc"),
        };
    let mut seen = FxHashSet::default();
    let engine = EngineConfig::depth(100);
    for d in 0..deals {
        let rng = &mut child(0x1A5A, &format!("deal{d}"));
        let mut s = match State::new(engine, Deck::shuffled(rng)) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut seq = ActionSeq::default();
        let mut guard = 0;
        while !s.is_terminal() && guard < 400 {
            guard += 1;
            let obs = cham_core::obs::Observables::view(&s, Player::from_usize(s.to_act()));
            let k = enc.key(&obs, &seq);
            seen.insert(k.0);
            let legals: Vec<_> = obs.legal.iter().map(|l| l.action).collect();
            if legals.is_empty() {
                break;
            }
            let a = legals[0];
            enc.record(&obs, Player::from_usize(s.to_act()), a, &mut seq);
            if s.apply(a).is_err() {
                break;
            }
        }
    }
    seen.len() as f64 * (2000.0 / deals.max(1) as f64) // crude extrapolation to 2000-deal sessions
}

/// One enforced perf gate: criterion bench mean vs. threshold (SPECS/00 §6).
struct PerfGate {
    gate: &'static str,
    bench: &'static str,
    /// threshold on the bench-sample mean, nanoseconds
    threshold_ns: f64,
    meaning: &'static str,
}

/// Gates (PERF-PLAN T6): thresholds are per-sample means at our committed
/// bench sizes, derived from the SPECS/00 §6 rates.
fn perf_gates() -> Vec<PerfGate> {
    vec![
        PerfGate {
            gate: "P1",
            bench: "eval_evaluate7",
            threshold_ns: 10_000.0,
            meaning: "1000 evals ≥ 100M/s",
        },
        PerfGate {
            gate: "P2",
            bench: "engine_apply",
            threshold_ns: 200_000.0,
            meaning: "2000 steps ≥ 10M/s",
        },
        PerfGate {
            gate: "P3a",
            bench: "encode_flop",
            threshold_ns: 100_000.0,
            meaning: "100 keys ≥ 1M/s",
        },
        PerfGate {
            gate: "P3b",
            bench: "encode_river",
            threshold_ns: 100_000.0,
            meaning: "10 keys ≥ 100k/s",
        },
        PerfGate {
            gate: "P4",
            bench: "mccfr_iter_200bb_tiny",
            threshold_ns: 2_000_000.0,
            meaning: "20 traversals",
        },
        PerfGate {
            gate: "P5",
            bench: "solve_rnr_400",
            threshold_ns: 7_000_000.0,
            meaning: "400-iter river solve",
        },
        PerfGate {
            gate: "P6",
            bench: "match_20_deals",
            threshold_ns: 30_000.0,
            meaning: "match throughput",
        },
        // B10.1/10.2: hero decision latency becomes a measured gate.
        PerfGate {
            gate: "P7",
            bench: "decision_latency",
            threshold_ns: 1_000_000.0,
            meaning: "hero decision p99 < 1 ms (search off)",
        },
        PerfGate {
            gate: "P8",
            bench: "decision_latency_search",
            threshold_ns: 50_000_000.0,
            meaning: "hero decision p99 < 50 ms (search on)",
        },
    ]
}

/// Read a bench-sample mean (ns) from criterion's estimates.json.
fn read_bench_mean(criterion_dir: &std::path::Path, bench: &str) -> Option<f64> {
    let p = criterion_dir.join(bench).join("new").join("estimates.json");
    let text = std::fs::read_to_string(p).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("mean")?.get("point_estimate")?.as_f64()
}

fn fmt_ns(ns: f64) -> String {
    if ns >= 1_000_000.0 {
        format!("{:.2} ms", ns / 1_000_000.0)
    } else if ns >= 1_000.0 {
        format!("{:.2} µs", ns / 1_000.0)
    } else {
        format!("{ns:.0} ns")
    }
}

/// Enforce perf gates (PERF-PLAN T6): read
/// `target/criterion/<bench>/new/estimates.json` for each gate bench, print a
/// `gate / threshold / measured / PASS|FAIL` table, and push failures so the
/// exit code is nonzero. Missing estimates → "run `just bench` first", nonzero
/// (never a silent pass).
fn check_perf_gates(failures: &mut Vec<String>) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let root = std::path::Path::new(manifest);
    let workspace_root = root
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root");
    let criterion_dir = workspace_root.join("target").join("criterion");
    let gates = perf_gates();
    let mut missing = Vec::new();
    let mut rows: Vec<(String, String, String, bool)> = Vec::new();
    for g in &gates {
        match read_bench_mean(&criterion_dir, g.bench) {
            Some(mean) => {
                let pass = mean <= g.threshold_ns;
                if !pass {
                    failures.push(format!(
                        "perf gate {} ({}) FAILED: measured {} > threshold {}",
                        g.gate,
                        g.bench,
                        fmt_ns(mean),
                        fmt_ns(g.threshold_ns)
                    ));
                }
                rows.push((
                    format!("{} ({})", g.gate, g.meaning),
                    fmt_ns(g.threshold_ns),
                    fmt_ns(mean),
                    pass,
                ));
            }
            None => missing.push(g.bench.to_string()),
        }
    }
    if !missing.is_empty() {
        println!(
            "perf gates: missing criterion estimates for: {}",
            missing.join(", ")
        );
        println!("perf gates: run `just bench` first");
        failures.push(format!(
            "perf gates: missing estimates for {}",
            missing.join(", ")
        ));
        return;
    }
    println!("perf gates (SPECS/00 §6; criterion means):");
    println!(
        "  {:<28} {:>12} {:>12}  verdict",
        "gate", "threshold", "measured"
    );
    for (gate, threshold, measured, pass) in rows {
        println!(
            "  {gate:<28} {threshold:>12} {measured:>12}  {}",
            if pass { "PASS" } else { "FAIL" }
        );
    }
}
