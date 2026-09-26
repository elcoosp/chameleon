//! `chameleon ab` (SPECS/09 §2): Tier 3 paired A/B with promotion.

pub fn run(
    a: &str,
    b: &str,
    deals: u64,
    _clusters: usize,
    margin: f64,
    no_sprt: bool,
    promote: bool,
) -> i32 {
    // B-2: same cache guard as play/ladder. No-op today; cheap.
    let _cache_guard =
        crate::cmd::cache_guard::CachePersist::hydrate("ab", "artifacts/river-cache.bin");
    // PERF-PLAN T7 guardrail on both arms: A/B-ing a trained agent without
    // its bundle compares two silent fallbacks (delta ≈ 0, meaningless).
    for arm in [a, b] {
        if let Err(missing) = crate::cmd::guard::require_agent_artifacts(arm) {
            eprintln!("ab: agent '{arm}' needs trained artifacts, missing:");
            for m in &missing {
                eprintln!("ab:   {m}");
            }
            eprintln!("ab: train them with train-buckets + train-bp (robust + 4 experts) first");
            return crate::cmd::EXIT_BUDGET;
        }
    }
    let spec = cham_eval::ab::AbSpec {
        a: a.into(),
        b: b.into(),
        deals_per_opp: deals,
        seeds: vec![1, 2, 3],
        conf: 0.95,
        margin_mb: margin,
        sprt: (!no_sprt).then_some(cham_eval::ab::SprtParams {
            delta0_mb: 0.0,
            delta1_mb: 25.0,
            alpha: 0.05,
            beta: 0.10,
        }),
    };
    let pool: Vec<cham_opponents::OpponentSpec> =
        ["arch:nit", "arch:tag", "arch:lag", "arch:station"]
            .iter()
            .filter_map(|id| cham_opponents::OpponentSpec::parse(id).ok())
            .collect();
    // B1: arms wire the REAL hero (same construction as `play`) — one shared
    // instance per arm. Pure-baseline arms keep the CallBot path; the
    // factory-based runner below is used only when NEITHER arm needs trained
    // artifacts (identical behavior for stateless heroes either way).
    // fn items (not closures) so both `&F` args share ONE type — the generic
    // `AbRunner::run` requires `hero_factory_a: &F, hero_factory_b: &F`.
    fn factory() -> Box<dyn cham_core::obs::Agent> {
        Box::new(cham_opponents::baselines::CallBot)
    }
    let ledger_dir = std::path::Path::new("artifacts/ledger");
    if let Err(e) = cham_eval::Ledger::open(ledger_dir) {
        eprintln!("ledger: {e}");
        return crate::cmd::EXIT_FAIL;
    }
    let needs_real = crate::cmd::guard::requires_trained_artifacts(a)
        || crate::cmd::guard::requires_trained_artifacts(b);
    let verdict = if needs_real {
        // v3 §1.1: pass hero FACTORIES (one fresh, session-isolated hero per
        // opponent per arm, built on its own thread) instead of two shared
        // instances. Blueprint artifacts are mmap-loaded (~µs/load), so N
        // independent instances cost microseconds, not seconds.
        let factory_a = || crate::cmd::hero::build_hero(a, 100);
        let factory_b = || crate::cmd::hero::build_hero(b, 100);
        // Fail fast if either arm can't build (preserves the old error path).
        if let Err(e) = factory_a() {
            eprintln!("ab: arm '{a}': {e}");
            return crate::cmd::EXIT_BUDGET;
        }
        if let Err(e) = factory_b() {
            eprintln!("ab: arm '{b}': {e}");
            return crate::cmd::EXIT_BUDGET;
        }
        match cham_eval::AbRunner::run_shared(
            &spec, &pool, &factory_a, &factory_b, 100, None,
        ) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("ab: {e}");
                return crate::cmd::EXIT_FAIL;
            }
        }
    } else {
        match cham_eval::AbRunner::run(&spec, &pool, &factory, &factory, 100, None) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("ab: {e}");
                return crate::cmd::EXIT_FAIL;
            }
        }
    };
    // v3 §2.1 step 1: vr_factor is first-class on every gate printout —
    // "how much did variance reduction already buy us" before deciding
    // whether full AIVAT is worth building. Cache hit-rate alongside it is
    // the §1.2 LRU kill-criterion telemetry (before/after across sweeps).
    let (chits, cmiss) = cham_search::cache::cache_stats();
    let chit_rate = if chits + cmiss > 0 {
        chits as f64 / (chits + cmiss) as f64
    } else {
        0.0
    };
    println!(
        "ab {a} vs {b}: delta {:+.1} mb/seating CI {:?} sprt={:?} vr_factor={:.2} cache_hit_rate={:.2} rule={}",
        verdict.delta_mb,
        verdict.ci,
        verdict.sprt,
        verdict.vr_factor,
        chit_rate,
        verdict.rule
    );
    for po in &verdict.per_opp {
        println!(
            "  {}: {:+.1} mb CI {:?} vr={:.2}",
            po.opponent, po.delta_mb, po.ci, po.vr_factor
        );
    }
    if verdict.promote && promote {
        let mut ledger = match cham_eval::Ledger::open(ledger_dir) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("ledger: {e}");
                return crate::cmd::EXIT_FAIL;
            }
        };
        let entry = cham_eval::ledger::LedgerEntry {
            ts: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            run: format!("ab-{a}-{b}"),
            kind: "ab".into(),
            a: serde_json::json!({"mode": a}),
            b: Some(serde_json::json!({"mode": b})),
            delta_mb: Some(verdict.delta_mb),
            ci: Some(verdict.ci),
            sprt: verdict.sprt.map(|s| format!("{s:?}")),
            promote: true,
            seatings: pool.len() as u64 * deals * 2,
            // v3 §2.2: bind both arms' artifact identities (auditable gate).
            artifact_hash: Some(format!(
                "a={} b={}",
                crate::cmd::guard::artifact_identity(a).unwrap_or("missing".into()),
                crate::cmd::guard::artifact_identity(b).unwrap_or("missing".into()),
            )),
            notes: Some("promotion".into()),
        };
        if let Err(e) = ledger.append(&entry) {
            eprintln!("ledger append: {e}");
            return crate::cmd::EXIT_FAIL;
        }
        println!("ab: PROMOTED → artifacts/ledger/ledger.jsonl");
    }
    crate::cmd::EXIT_OK
}
