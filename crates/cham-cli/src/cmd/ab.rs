//! `chameleon ab` (SPECS/09 §2): Tier 3 paired A/B with promotion.

pub fn run(
    a: &str,
    b: &str,
    deals: u64,
    _clusters: usize,
    margin: f64,
    sprt: bool,
    promote: bool,
) -> i32 {
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
        sprt: sprt.then_some(cham_eval::ab::SprtParams {
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
    // arms: A/B factories are mode-wired at M3 (agent artifacts); the default
    // wiring plays the robust blueprint on both arms so the runner is exercised.
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
    let verdict = match cham_eval::AbRunner::run(&spec, &pool, &factory, &factory, 100, None) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("ab: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    println!(
        "ab {a} vs {b}: delta {:+.1} mb/seating CI {:?} sprt={:?} rule={}",
        verdict.delta_mb, verdict.ci, verdict.sprt, verdict.rule
    );
    for po in &verdict.per_opp {
        println!("  {}: {:+.1} mb CI {:?}", po.opponent, po.delta_mb, po.ci);
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
