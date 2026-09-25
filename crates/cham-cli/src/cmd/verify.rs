//! `chameleon verify` (SPECS/09 §2): Tier 0.

/// Workspace invariant greps (SPECS/00 §11).
fn grep_invariants() -> Result<(), String> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let root = std::path::Path::new(manifest).parent().expect("workspace root");
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
            println!("proof {}: {} — {}", r.id, if r.passed { "PASS" } else { "FAIL" }, r.detail);
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

    // 4. perf gates live in criterion benches (`just bench`; SPECS/00 §6)
    if perf {
        println!("perf gates: run `just bench` and compare against SPECS/00 §6");
        println!("  P1 eval ≥ 100M/s (M1)  P2 apply ≥ 10M/s  P3a/P3b encode  P4 mccfr  P5 solve  P6 match");
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
    use cham_core::engine::config::EngineConfig;
    use cham_core::engine::State;
    use cham_core::obs::Player;
    use cham_core::rng::child;
    use cham_engine::config::AbstractionConfig;
    use cham_engine::encoder::{ActionSeq, Encoder};
    use rustc_hash::FxHashSet;

    let cfg = AbstractionConfig::tiny();
    let mut enc = match Encoder::from_artifacts_dir(std::path::Path::new("artifacts/buckets-tiny"), cfg) {
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
