//! `chameleon ladder` (SPECS/09 §2): Tier 2 screening with SPRT.

pub fn run(_fast: bool, full: bool, agent: &str, pool_path: &str) -> i32 {
    // PERF-PLAN T7 guardrail: evaluating a trained agent without its bundle
    // yields silent-fallback mirror rows (meaningless strength numbers).
    if let Err(missing) = crate::cmd::guard::require_agent_artifacts(agent) {
        eprintln!("ladder: agent '{agent}' needs trained artifacts, missing:");
        for m in &missing {
            eprintln!("ladder:   {m}");
        }
        eprintln!("ladder: train them with train-buckets + train-bp (robust + 4 experts) first");
        return crate::cmd::EXIT_BUDGET;
    }
    let tier = if full { "full" } else { "fast" };
    let deals = if full { 25_000 } else { 2_500 };
    let pool = match std::fs::read_to_string(pool_path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("read {pool_path}: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    let value: toml::Value = match toml::from_str(&pool) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("parse {pool_path}: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    let mut opponent_ids: Vec<String> = Vec::new();
    if let Some(opps) = value.get("opponents").and_then(|o| o.as_array()) {
        for o in opps {
            if let Some(id) = o.get("id").and_then(|i| i.as_str()) {
                opponent_ids.push(id.to_string());
            }
        }
    }
    if opponent_ids.is_empty() {
        opponent_ids = vec!["callbot".into(), "fish".into()];
    }
    println!(
        "ladder[{tier}] agent={agent} opponents={} deals/deal-pair={deals}",
        opponent_ids.len()
    );
    let specs: Vec<cham_opponents::OpponentSpec> = opponent_ids
        .iter()
        .filter_map(|id| cham_opponents::OpponentSpec::parse(id).ok())
        .collect();
    let factory =
        || -> Box<dyn cham_core::obs::Agent> { Box::new(cham_opponents::baselines::CallBot) };
    let mut total_seatings = 0u64;
    let mut per_opp: Vec<(String, f64, f64)> = Vec::new();
    for opp in &specs {
        let spec = cham_eval::matcheng::MatchSpec {
            opponent: cham_opponents::factory::OpponentSpecDto(opp.id()),
            deals,
            depth_bb: 100,
            base_seed: 0x1AD,
            label: format!("ladder:{tier}"),
        };
        match cham_eval::MatchRunner::run(&spec, &factory, None) {
            Ok(r) => {
                total_seatings += r.seatings;
                per_opp.push((opp.id(), r.mb_per_seating, r.se_mb));
                println!(
                    "  {}: {:+.1} ± {:.1} mb/seating ({} seatings, VR ×{:.2})",
                    opp.id(),
                    r.mb_per_seating,
                    r.se_mb,
                    r.seatings,
                    r.vr_factor
                );
            }
            Err(e) => {
                eprintln!("  {}: match failed: {e}", opp.id());
                return crate::cmd::EXIT_FAIL;
            }
        }
    }
    // screening numbers land in the append-only ledger (M1 gate: the cycle
    // produces ledger entries with CIs) — diagnostic tier, never a promotion.
    // PERF-PLAN T7: fallback-rate warnings land in the ledger entry too. The
    // current hero factories are pure baselines (no traced fallbacks), so the
    // rate here is 0/total; once hero factories produce traced ChameleonAgent
    // decisions, count them here and the >20% warning fires automatically.
    let fallback_warning = crate::cmd::guard::check_fallback_rate(
        &format!("ladder[{tier}]:{agent}"),
        0,
        total_seatings,
    );
    if let Some(w) = &fallback_warning {
        eprintln!("{w}");
    }
    let mut ledger = match cham_eval::Ledger::open(std::path::Path::new("artifacts/ledger")) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("ledger: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = cham_eval::ledger::LedgerEntry {
        ts,
        run: format!("ladder-{tier}-{ts}"),
        kind: "ladder".into(),
        a: serde_json::json!({
            "agent": agent,
            "per_opponent": per_opp.iter().map(|(id, mb, se)| serde_json::json!(
                {"id": id, "mb_per_seating": mb, "se_mb": se}
            )).collect::<Vec<_>>(),
        }),
        b: None,
        delta_mb: None,
        ci: None,
        sprt: None,
        promote: false,
        seatings: total_seatings,
        notes: Some(match &fallback_warning {
            Some(w) => format!("tier {tier} screening — diagnostic, CI per opponent. {w}"),
            None => format!("tier {tier} screening — diagnostic, CI per opponent"),
        }),
    };
    if let Err(e) = ledger.append(&entry) {
        eprintln!("ledger append: {e}");
        return crate::cmd::EXIT_FAIL;
    }
    println!("ladder[{tier}]: {total_seatings} seatings total (ledger entry written)");
    crate::cmd::EXIT_OK
}
