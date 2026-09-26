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

pub fn run(perf: bool, count_infosets: bool, proofs: bool, gpu: bool, tables: Option<&str>) -> i32 {
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

    // 5. GPU-track gates (GPU-PLAN G3.0): P7 bit-exactness + P8 throughput.
    //    Reads pre-built tables under --tables; does NOT build. On non-macOS
    //    or feature-off, prints SKIP and returns — the CI path.
    if gpu {
        check_gpu_gates(tables, &mut failures);
        check_bucket_artifacts(&mut failures);
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

/// `verify --gpu` (GPU-PLAN G3.0). Reads pre-built tables under `tables`
/// (default `artifacts/gpu-tables`) and enforces:
///
/// - **P7 GPU-CONSISTENCY**: N random (board, hole) per complete table
///   compared to the CPU reference `ehs_reference`. Runs on **any** host
///   (no GPU needed — verification is a CPU re-derivation of GPU output).
/// - **P8 BUILD-THROUGHPUT**: manifest's recorded `throughput_evals_per_s`
///   must clear a sanity floor.
/// - **P9 INTEGRATION**: with G2.x SKIP'd, zero consumers exist.
///
/// Never fails on a missing/empty tables dir: prints "no tables built" and
/// exits clean. Partial builds (manifest absent or stale) are reported but
/// do not fail — the operator is expected to be running a build.
fn check_gpu_gates(tables: Option<&str>, failures: &mut Vec<String>) {
    let dir = tables.unwrap_or("artifacts/gpu-tables");
    println!("verify --gpu: tables dir = {dir}");

    let dir_path = std::path::Path::new(dir);
    if !dir_path.is_dir() {
        println!("verify --gpu: no tables dir at {dir} — nothing to check (OK)");
        return;
    }

    let mut manifests: Vec<(String, std::path::PathBuf)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir_path) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("json")
                && let Some(stem) = p.file_stem().and_then(|s| s.to_str())
            {
                manifests.push((stem.to_string(), p));
            }
        }
    }
    if manifests.is_empty() {
        println!("verify --gpu: no manifests under {dir} — nothing to check (OK)");
        return;
    }
    manifests.sort_by(|a, b| a.0.cmp(&b.0));

    for (kind, manifest_path) in &manifests {
        let raw = match std::fs::read_to_string(manifest_path) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!(
                    "verify --gpu: cannot read {}: {e}",
                    manifest_path.display()
                ));
                continue;
            }
        };
        let m: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!(
                    "verify --gpu: {} is not JSON: {e}",
                    manifest_path.display()
                ));
                continue;
            }
        };
        let boards = m.get("boards").and_then(|v| v.as_u64()).unwrap_or(0);
        let holes = m.get("holes").and_then(|v| v.as_u64()).unwrap_or(0);
        let denom = m.get("denom").and_then(|v| v.as_u64()).unwrap_or(0);
        let bytes = m.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);
        let rate = m
            .get("throughput_evals_per_s")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let blake3_str = m
            .get("blake3")
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>");
        let complete = m.get("complete").and_then(|v| v.as_bool()).unwrap_or(false);
        println!(
            "  table {kind:<8} boards={boards} holes={holes} denom={denom} bytes={bytes} complete={complete}"
        );
        println!("    blake3={}", &blake3_str[..blake3_str.len().min(16)]);
        println!("    manifest throughput = {rate:.2e} evals/s");

        let bin_path = dir_path.join(format!("{kind}.bin"));
        if !bin_path.is_file() {
            if complete {
                failures.push(format!(
                    "verify --gpu: manifest {kind}.json present but {kind}.bin missing"
                ));
            } else {
                println!("    bin absent (build incomplete) — skipped");
            }
            continue;
        }
        let bin_size = std::fs::metadata(&bin_path).map(|m| m.len()).unwrap_or(0);
        if complete && bin_size != bytes {
            failures.push(format!(
                "verify --gpu: {kind}.bin size {} != manifest bytes {}",
                bin_size, bytes
            ));
            continue;
        }
        if !complete && bin_size > bytes {
            println!(
                "    bin size {} > manifest bytes {} (mid-build; manifest is stale)",
                bin_size, bytes
            );
        }

        // P7 resample: on ANY host (verification is CPU-side). Supports
        // turn (4-card boards) and flop (3-card boards).
        let n_sample: usize = 24;
        if (kind == "turn" || kind == "flop") && complete {
            use cham_core::card::Card;
            use cham_gpu::reference::{EhsDenom, ehs_reference};
            use std::io::{Read, Seek, SeekFrom};

            let mut f = match std::fs::File::open(&bin_path) {
                Ok(f) => f,
                Err(e) => {
                    failures.push(format!("verify --gpu: open {kind}.bin: {e}"));
                    continue;
                }
            };
            let per_board = (holes * 4).max(1);
            let mut rng: u64 = 0x9E37_79B9_7F4A_7C15;
            let mut checked = 0usize;
            let mut bad: Vec<(u64, u64, u32, u64)> = Vec::new();
            let mut attempts = 0;
            while checked < n_sample && attempts < n_sample * 4 {
                attempts += 1;
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                let board_i = rng % boards.max(1);
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                let hole_i = (rng % holes.max(1)) as u16;
                let (lo, hi) = match hole2_lo_hi(hole_i) {
                    Some(v) => v,
                    None => continue,
                };
                let board: Vec<cham_core::card::Card> = if kind == "flop" {
                    nth_board3(board_i).to_vec()
                } else {
                    nth_board4(board_i).to_vec()
                };
                if board.iter().any(|c| c.0 == lo || c.0 == hi) {
                    continue;
                }
                let off = board_i * per_board + (hole_i as u64) * 4;
                if f.seek(SeekFrom::Start(off)).is_err() {
                    continue;
                }
                let mut buf = [0u8; 4];
                if f.read_exact(&mut buf).is_err() {
                    continue;
                }
                let gpu_val = u32::from_le_bytes(buf);
                let hole = [Card(lo), Card(hi)];
                let street = if kind == "flop" {
                    EhsDenom::Flop
                } else {
                    EhsDenom::Turn
                };
                let cpu_val = ehs_reference(&board[..], hole, street) as u64;
                checked += 1;
                if gpu_val as u64 != cpu_val {
                    bad.push((board_i, hole_i as u64, gpu_val, cpu_val));
                }
            }
            if bad.is_empty() {
                println!("    P7 resample: {checked}/{checked} bit-equal");
            } else {
                for (b, h, g, c) in &bad {
                    eprintln!("verify --gpu: P7 mismatch board={b} hole={h} gpu={g} cpu={c}");
                }
                failures.push(format!(
                    "verify --gpu: {}/{} resample mismatches on {kind}",
                    bad.len(),
                    checked
                ));
            }
        } else if !complete {
            println!("    P7 resample: SKIP (table incomplete)");
        } else {
            println!("    P7 resample: SKIP (kind={kind} resample not implemented)");
        }

        // P8: rate floor. 1e8 is 30x below real builds and 40x above the
        // old boards-only formula's off-by-1326x value — a real build
        // clears, a bogus one fails loudly.
        const FLOOR_RATE: f64 = 1.0e8;
        if complete {
            if rate >= FLOOR_RATE {
                println!("    P8 rate:     {rate:.2e} >= {FLOOR_RATE:.1e} (floor)");
            } else {
                failures.push(format!(
                    "verify --gpu: {kind} manifest rate {rate:.2e} below floor {FLOOR_RATE:.1e}"
                ));
            }
        } else {
            println!("    P8 rate:     SKIP (table incomplete)");
        }
    }

    println!("  P9: 0 shipped consumers (G2.0 skip) — informational");

    let gpu_failures = failures
        .iter()
        .filter(|f| f.starts_with("verify --gpu"))
        .count();
    println!(
        "verify --gpu: {} table(s) checked — {}",
        manifests.len(),
        if gpu_failures == 0 {
            "all gates green".to_string()
        } else {
            format!("{gpu_failures} FAILURES")
        }
    );
}

/// Reconstruct the (lo, hi) card ids of a `hole2_index` value, or `None` if
/// the index is not a valid unordered pair (lo < hi).
fn hole2_lo_hi(idx: u16) -> Option<(u8, u8)> {
    let mut hi: u16 = 1;
    while ((hi as u32) * (hi as u32 - 1)) / 2 <= idx as u32 && hi < 52 {
        hi += 1;
    }
    hi -= 1;
    let lo = idx - (hi * (hi - 1)) / 2;
    if lo >= hi {
        return None;
    }
    Some((lo as u8, hi as u8))
}

/// The i-th 4-card board in ascending lexicographic order over card ids.
/// Matches the ordering produced by `cham-gpu/src/bin/gpu-build.rs`.
fn nth_board3(i: u64) -> [cham_core::card::Card; 3] {
    use cham_core::card::Card;
    let mut count = 0u64;
    for a in 0u8..52 {
        for b in (a + 1)..52 {
            for c in (b + 1)..52 {
                if count == i {
                    return [Card(a), Card(b), Card(c)];
                }
                count += 1;
            }
        }
    }
    [Card(0), Card(1), Card(2)]
}

fn nth_board4(i: u64) -> [cham_core::card::Card; 4] {
    use cham_core::card::Card;
    let mut count = 0u64;
    for a in 0u8..52 {
        for b in (a + 1)..52 {
            for c in (b + 1)..52 {
                for d in (c + 1)..52 {
                    if count == i {
                        return [Card(a), Card(b), Card(c), Card(d)];
                    }
                    count += 1;
                }
            }
        }
    }
    [Card(0), Card(1), Card(2), Card(3)]
}

/// `verify --gpu` extension: structural sanity on bucket artifacts.
///
/// Scans `artifacts/buckets*/meta.json`, validates the shared schema
/// (version 2, `river_eq_edges` non-empty, per-street `k`/`orbits` fields),
/// and confirms the companion `.bin` files exist and are non-trivial.
///
/// During a full build, `turn` may be `null` in meta.json — that is a valid
/// "still in progress" state and reported as INFO, not a failure. Once the
/// build writes turn.bin + updates meta.json, the field is present.
///
/// Never asserts exact byte counts: the row width is not part of the
/// public contract, and pinning it here would break the moment the format
/// legitimately changes.
fn check_bucket_artifacts(failures: &mut Vec<String>) {
    let root = std::path::Path::new("artifacts");
    if !root.is_dir() {
        return;
    }
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if p.is_dir() && name.starts_with("buckets") {
                dirs.push(p);
            }
        }
    }
    if dirs.is_empty() {
        println!("verify --gpu: no buckets* dirs under artifacts/ (OK)");
        return;
    }
    dirs.sort();

    for dir in &dirs {
        let label = dir.file_name().and_then(|s| s.to_str()).unwrap_or("?");
        let meta_path = dir.join("meta.json");
        if !meta_path.is_file() {
            failures.push(format!("verify --gpu: {label}/meta.json missing"));
            continue;
        }
        let raw = match std::fs::read_to_string(&meta_path) {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("verify --gpu: {label}/meta.json unreadable: {e}"));
                continue;
            }
        };
        let m: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!("verify --gpu: {label}/meta.json not JSON: {e}"));
                continue;
            }
        };
        let version = m.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
        if version != 2 {
            failures.push(format!("verify --gpu: {label} version {version} != 2"));
        }
        let edges = m
            .get("river_eq_edges")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        if edges < 2 {
            failures.push(format!(
                "verify --gpu: {label} river_eq_edges len {edges} < 2"
            ));
        }

        // Per-street checks
        let mut per_street_ok = true;
        for street in ["flop", "turn"] {
            let field = m.get(street);
            match field {
                None | Some(serde_json::Value::Null) => {
                    if street == "turn" {
                        println!("  buckets {label:<12} turn: still building (meta.json null)");
                    } else {
                        failures.push(format!("verify --gpu: {label} has no {street} block"));
                        per_street_ok = false;
                    }
                }
                Some(v) => {
                    let k = v.get("k").and_then(|x| x.as_u64()).unwrap_or(0);
                    let orbits = v.get("orbits").and_then(|x| x.as_u64()).unwrap_or(0);
                    if k == 0 {
                        failures.push(format!("verify --gpu: {label} {street}.k == 0"));
                        per_street_ok = false;
                    }
                    if orbits == 0 {
                        failures.push(format!("verify --gpu: {label} {street}.orbits == 0"));
                        per_street_ok = false;
                    }
                    let bin = dir.join(format!("{street}.bin"));
                    if !bin.is_file() {
                        if street == "turn" {
                            // Freshly-resumed build may not have written turn.bin yet.
                            println!("  buckets {label:<12} {street}: meta present, .bin not yet");
                        } else {
                            failures.push(format!("verify --gpu: {label}/{street}.bin missing"));
                            per_street_ok = false;
                        }
                    } else {
                        let sz = std::fs::metadata(&bin).map(|x| x.len()).unwrap_or(0);
                        if sz < 1024 {
                            failures.push(format!(
                                "verify --gpu: {label}/{street}.bin suspiciously small ({sz} B)"
                            ));
                            per_street_ok = false;
                        }
                    }
                    if per_street_ok {
                        println!("  buckets {label:<12} {street}: k={k} orbits={orbits}");
                    }
                }
            }
        }
    }
}
