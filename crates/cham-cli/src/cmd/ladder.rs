//! `chameleon ladder` (SPECS/09 §2): Tier 2 screening with SPRT.
//!
//! BROAD-PERF-PLAN wiring:
//! - B1: the REAL hero agent (same construction as `play`) via `cmd::hero` —
//!   one `CountingHero` per opponent match (agents own per-hand tracker state,
//!   never shared across matches). Missing `artifacts/agent` refuses (guard).
//! - B2: one thread per opponent (`std::thread::scope`); per-opponent seeds
//!   derived as `base_seed ^ ((index as u64) << 32)` (same scheme as
//!   `MatchRunner::run_pool`) so parallel runs re-derive the sequential
//!   streams. Output printed in pool order (deterministic).
//! - B3: chunked SPRT early-stop (250-deal chunks, global-index seeds, so a
//!   full-budget run is seed-identical to the old single-shot run).
//! - B4: the printed `±` is the VR-adjusted (duplicate) SE with `VR ×` factor.

use crate::cmd::hero::{CountingHero, build_chameleon, build_hero};

/// Deals per SPRT chunk (B3).
const CHUNK_DEALS: u64 = 250;

struct OppCfg {
    id: String,
    sprt_delta_mb: f64,
}

struct SprtCfg {
    delta0_mb: f64,
    default_delta1_mb: f64,
    alpha: f64,
    beta: f64,
}

fn parse_pool(pool_path: &str) -> Result<(Vec<OppCfg>, SprtCfg), String> {
    let pool = std::fs::read_to_string(pool_path).map_err(|e| format!("read {pool_path}: {e}"))?;
    let value: toml::Value =
        toml::from_str(&pool).map_err(|e| format!("parse {pool_path}: {e}"))?;
    let sprt = SprtCfg {
        delta0_mb: value
            .get("sprt")
            .and_then(|s| s.get("delta0_mb"))
            .and_then(|v| v.as_float())
            .unwrap_or(0.0),
        default_delta1_mb: value
            .get("sprt")
            .and_then(|s| s.get("delta1_mb"))
            .and_then(|v| v.as_float())
            .unwrap_or(25.0),
        alpha: value
            .get("sprt")
            .and_then(|s| s.get("alpha"))
            .and_then(|v| v.as_float())
            .unwrap_or(0.05),
        beta: value
            .get("sprt")
            .and_then(|s| s.get("beta"))
            .and_then(|v| v.as_float())
            .unwrap_or(0.10),
    };
    let mut opps: Vec<OppCfg> = Vec::new();
    if let Some(arr) = value.get("opponents").and_then(|o| o.as_array()) {
        for o in arr {
            if let Some(id) = o.get("id").and_then(|i| i.as_str()) {
                // per-opponent SPRT target (B3; default 25.0 mb/seating)
                let d1 = o
                    .get("sprt_delta_mb")
                    .and_then(|v| v.as_float())
                    .unwrap_or(sprt.default_delta1_mb);
                opps.push(OppCfg {
                    id: id.to_string(),
                    sprt_delta_mb: d1,
                });
            }
        }
    }
    if opps.is_empty() {
        for id in ["callbot", "fish"] {
            opps.push(OppCfg {
                id: id.into(),
                sprt_delta_mb: sprt.default_delta1_mb,
            });
        }
    }
    Ok((opps, sprt))
}

struct OppOutcome {
    id: String,
    mb: f64,
    se: f64,
    vr: f64,
    seatings: u64,
    deals_run: u64,
    deals_budget: u64,
    sprt_stop: Option<String>,
    decisions: u64,
    fallbacks: u64,
    error: Option<String>,
}

/// Run one opponent match: chunked shared-hero run with SPRT early-stop.
fn run_opponent(
    index: usize,
    opp: &OppCfg,
    agent: &str,
    deals: u64,
    tier: &str,
    sprt: &SprtCfg,
) -> OppOutcome {
    // SEED RULE (B2): derived, never shared — same scheme as run_pool.
    let base_seed: u64 = 0x1AD ^ ((index as u64) << 32);
    let spec = |deals: u64| cham_eval::matcheng::MatchSpec {
        opponent: cham_opponents::factory::OpponentSpecDto(opp.id.clone()),
        deals,
        depth_bb: 100,
        base_seed,
        label: format!("ladder:{tier}"),
    };
    // B1: the REAL hero — trained modes build ChameleonAgent (counted);
    // pure baselines keep the CallBot factory path.
    let trained = crate::cmd::guard::requires_trained_artifacts(agent);
    let mut hero: CountingHero = if trained {
        match build_chameleon(agent, 100) {
            Ok(bot) => CountingHero::chameleon(bot),
            Err(e) => {
                return OppOutcome {
                    id: opp.id.clone(),
                    mb: 0.0,
                    se: 0.0,
                    vr: 1.0,
                    seatings: 0,
                    deals_run: 0,
                    deals_budget: deals,
                    sprt_stop: None,
                    decisions: 0,
                    fallbacks: 0,
                    error: Some(e),
                };
            }
        }
    } else {
        CountingHero::new(
            build_hero(agent, 100).unwrap_or_else(|_| Box::new(cham_opponents::baselines::CallBot)),
        )
    };

    let mut cum_profits: Vec<f64> = Vec::with_capacity(deals as usize);
    let (mut mb, mut se, mut vr) = (0.0, 0.0, 1.0);
    let mut deals_run = 0u64;
    let mut sprt_stop: Option<String> = None;
    let mut done = 0u64;
    while done < deals {
        let take = (deals - done).min(CHUNK_DEALS);
        let chunk_spec = spec(deals);
        match cham_eval::MatchRunner::run_shared_range(
            &chunk_spec,
            &mut hero,
            None,
            done..done + take,
        ) {
            Ok(r) => {
                if let Some(p) = r.per_deal_profits {
                    cum_profits.extend(p);
                }
                vr = r.vr_factor;
                deals_run += take;
                done += take;
                // M-12 fix (2026-09-27): the printed ± and the ledger CI must
                // cover the SAME data as `mb`. The previous code took
                // `se = r.se_mb` (the CURRENT CHUNK's SE — 250 deals) while
                // `mb = mean(cum_profits)` covered every chunk so far. For a
                // `--full` run (25k deals) the reported ± was ~10× too wide.
                // Recompute both from the accumulated series.
                mb = cham_eval::mean(&cum_profits);
                se = cham_eval::se(&cum_profits);
                if done < deals {
                    match cham_eval::sprrt(
                        &cum_profits,
                        sprt.delta0_mb,
                        opp.sprt_delta_mb,
                        sprt.alpha,
                        sprt.beta,
                    ) {
                        Ok(cham_eval::SprtState::AcceptH1) => {
                            sprt_stop = Some("AcceptH1".into());
                            break;
                        }
                        Ok(cham_eval::SprtState::AcceptH0) => {
                            sprt_stop = Some("AcceptH0".into());
                            break;
                        }
                        Ok(cham_eval::SprtState::Continue) => {}
                        Err(e) => {
                            return OppOutcome {
                                id: opp.id.clone(),
                                mb,
                                se,
                                vr,
                                seatings: deals_run * 2,
                                deals_run,
                                deals_budget: deals,
                                sprt_stop: None,
                                decisions: hero.decisions(),
                                fallbacks: hero.fallbacks(),
                                error: Some(format!("sprt: {e}")),
                            };
                        }
                    }
                }
            }
            Err(e) => {
                return OppOutcome {
                    id: opp.id.clone(),
                    mb,
                    se,
                    vr,
                    seatings: deals_run * 2,
                    deals_run,
                    deals_budget: deals,
                    sprt_stop: None,
                    decisions: hero.decisions(),
                    fallbacks: hero.fallbacks(),
                    error: Some(format!("{e}")),
                };
            }
        }
    }
    OppOutcome {
        id: opp.id.clone(),
        mb,
        se,
        vr,
        seatings: deals_run * 2,
        deals_run,
        deals_budget: deals,
        sprt_stop,
        decisions: hero.decisions(),
        fallbacks: hero.fallbacks(),
        error: None,
    }
}

pub fn run(_fast: bool, full: bool, agent: &str, pool_path: &str) -> i32 {
    // B-2: hydrate the persistent river-subgame cache. A no-op today
    // (ladder does not enable search), but cheap and future-proof.
    let _cache_guard =
        crate::cmd::cache_guard::CachePersist::hydrate("ladder", "artifacts/river-cache.bin");
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
    let (opps, sprt) = match parse_pool(pool_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("ladder: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    println!(
        "ladder[{tier}] agent={agent} opponents={} deals/deal-pair={deals}",
        opps.len()
    );
    // B2: one thread per opponent; results collected in pool order so output
    // stays deterministic (never completion order).
    let sprt_ref = &sprt;
    let outcomes: Vec<OppOutcome> = std::thread::scope(|s| {
        let handles: Vec<_> = opps
            .iter()
            .enumerate()
            .map(|(i, opp)| s.spawn(move || run_opponent(i, opp, agent, deals, tier, sprt_ref)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("ladder thread"))
            .collect()
    });
    let mut total_seatings = 0u64;
    let mut total_decisions = 0u64;
    let mut total_fallbacks = 0u64;
    let mut total_saved = 0u64;
    let mut per_opp: Vec<(String, f64, f64)> = Vec::new();
    let mut sprt_notes: Vec<serde_json::Value> = Vec::new();
    for o in &outcomes {
        if let Some(e) = &o.error {
            eprintln!("  {}: match failed: {e}", o.id);
            return crate::cmd::EXIT_FAIL;
        }
        total_seatings += o.seatings;
        total_decisions += o.decisions;
        total_fallbacks += o.fallbacks;
        total_saved += (o.deals_budget - o.deals_run) * 2;
        per_opp.push((o.id.clone(), o.mb, o.se));
        let stop_mark = o
            .sprt_stop
            .as_ref()
            .map(|s| format!(" sprt-stop({s})"))
            .unwrap_or_default();
        println!(
            "  {}: {:+.1} ± {:.1} mb/seating ({} seatings, VR ×{:.2}){stop_mark}",
            o.id, o.mb, o.se, o.seatings, o.vr
        );
        sprt_notes.push(serde_json::json!({
            "id": o.id,
            "mb_per_seating": o.mb,
            "se_mb": o.se,
            "vr_factor": o.vr,
            "deals_run": o.deals_run,
            "deals_budget": o.deals_budget,
            "sprt_stop": o.sprt_stop,
        }));
    }
    // B1: fallback-rate guardrail over REAL counted decisions (baselines report
    // 0/total → None, same as before).
    let fallback_warning = crate::cmd::guard::check_fallback_rate(
        &format!("ladder[{tier}]:{agent}"),
        total_fallbacks,
        total_decisions,
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
            "per_opponent": sprt_notes,
            "seatings_saved_by_sprt": total_saved,
        }),
        b: None,
        delta_mb: None,
        ci: None,
        sprt: if total_saved > 0 {
            Some(format!("sprt-stop saved {total_saved} seatings"))
        } else {
            None
        },
        promote: false,
        seatings: total_seatings,
        // v3 §2.2: every ledger number carries its bound artifact identity.
        artifact_hash: crate::cmd::guard::artifact_identity(agent),
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
