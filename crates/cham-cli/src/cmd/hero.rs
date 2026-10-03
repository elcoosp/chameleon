//! Shared hero-agent construction (BROAD-PERF-PLAN B1): the REAL agent wiring,
//! factored out of `play` so `ladder`/`ab` build exactly the same pipeline.
//!
//! Construction mirrors `cmd::play::run`: `loader::load_agent(bundle, routing,
//! depth)` → router (trained `router.bin` when present, deterministic default
//! otherwise) → `ChameleonAgent::new`. One instance plays a whole match; its
//! `on_hand_end` hook resets per-hand state deal-by-deal (see `matcheng`).

use cham_agent::modes::{AgentMode, SearchCfg};
use cham_agent::pipeline::ChameleonAgent;
use cham_router::model::SoftmaxModel;

/// CLI mode names → AgentMode routing strings (SPECS/07 §3 canonical set;
/// must stay in sync with `cmd::play::run`).
pub fn routing_for(agent: &str) -> &str {
    match agent {
        // SOTA 2026-09-28 (docs/plans/SOTA-2026-09-28.md): argmax routing
        // beats mixture on the current bundle — aggregate +6 567 vs +3 184
        // mb/seating (mixture + synthetic router: +4 388; robust-only:
        // much worse). `full` now maps to argmax so the shipped default is
        // the measured-best configuration.
        //
        // Use `full-mixture` to get the historical mixture behavior
        // (reach-weighted blend of 5 experts, sharpened-softmax weights).
        // That will become the right default again once `collect` produces
        // real (not synthetic) router training data; see
        // docs/plans/ROUTER-TRAINING-GAP-2026-09-28.md.
        "full" | "no-search" | "full-no-search" => "argmax",
        "full-argmax" | "argmax" | "no-search-argmax" => "argmax",
        "full-mixture" | "mixture" => "mixture",
        // PERF (2026-09-29): hedge on router confidence. Argmax when the
        // top weight is above CHAM_HEDGE_THRESHOLD (default 0.5), else
        // fall back to the mixture.
        "full-hedged" | "hedged" => "hedged",
        // 2026-10-01 (F7): route like argmax, but sample the expert's
        // mixed strategy instead of playing its mode. Opt-in mode for the
        // A/B against the mode-taking `argmax`.
        "sample-expert" | "full-sample-expert" => "sample-expert",
        "robust-only" => "robust-only",
        "bayes" => "bayes",
        other => other,
    }
}

/// Build a fresh hero for `agent` at `depth_bb`. Pure baselines (anything not
/// requiring trained artifacts, e.g. `callbot`) yield a `CallBot`; trained
/// modes load the `artifacts/agent` bundle or return the refusal message.
pub fn build_hero(agent: &str, depth_bb: i64) -> Result<Box<dyn cham_core::obs::Agent>, String> {
    if !crate::cmd::guard::requires_trained_artifacts(agent) {
        return Ok(Box::new(cham_opponents::baselines::CallBot));
    }
    Ok(Box::new(build_chameleon(agent, depth_bb)?))
}

/// Build a concrete `ChameleonAgent` (trained path of [`build_hero`]).
pub fn build_chameleon(agent: &str, depth_bb: i64) -> Result<ChameleonAgent, String> {
    build_chameleon_with_router(agent, depth_bb, None)
}

/// 2026-10-01 (F1): variant with an explicit search flag. Enables live
/// river solving for evaluation tools (ladder/probe). Uses `EXP-SEARCH`
/// as the auditable G4 ledger token when enabled.
pub fn build_chameleon_with_search(
    agent: &str,
    depth_bb: i64,
    search_enabled: bool,
) -> Result<ChameleonAgent, String> {
    build_chameleon_with_router_and_search(agent, depth_bb, None, search_enabled)
}

/// EXP-015: same as [`build_chameleon`] but the `full` victim's
/// `RouterRuntime` is built from override hyperparameters instead of the
/// checked-in defaults. `overrides = (temp, n0, shield_beta, shield_z)`.
pub fn build_chameleon_with_router(
    agent: &str,
    depth_bb: i64,
    overrides: Option<(f64, f64, f64, f64)>,
) -> Result<ChameleonAgent, String> {
    build_chameleon_with_router_and_search(agent, depth_bb, overrides, false)
}

/// 2026-10-01 (F1): the underlying builder accepts a `search_enabled`
/// flag that flips `AgentMode.search.enabled`. When true, the mode
/// carries `g4_ledger_ref = "EXP-SEARCH"` (the auditable opt-in token
/// per SPECS/06 §7). Existing callers pass `false` and get the
/// pre-F1 behavior bit-for-bit.
pub fn build_chameleon_with_router_and_search(
    agent: &str,
    depth_bb: i64,
    overrides: Option<(f64, f64, f64, f64)>,
    search_enabled: bool,
) -> Result<ChameleonAgent, String> {
    // Default bundle resolution:
    //   1. CHAM_AGENT_BUNDLE if set (competitive-measurement override);
    //   2. else `artifacts/agent-honest-19dim` (the promoted retrained
    //      19-dim bundle) IF present;
    //   3. else `artifacts/agent` (the tracked, router-less fallback).
    //
    // Step 2's path is gitignored: it exists on a working checkout but NOT
    // on a fresh clone. The fallback to the tracked `artifacts/agent` keeps
    // a fresh clone loadable instead of refusing outright. Set
    // CHAM_AGENT_BUNDLE to force either explicitly.
    let bundle_path = std::env::var("CHAM_AGENT_BUNDLE").unwrap_or_else(|_| {
        let promoted = std::path::Path::new("artifacts/agent-honest-19dim/robust/policy.bin");
        if promoted.exists() {
            "artifacts/agent-honest-19dim".to_string()
        } else {
            "artifacts/agent".to_string()
        }
    });
    let bundle = std::path::Path::new(&bundle_path);
    let routing = routing_for(agent);
    let loaded = cham_agent::loader::load_agent(bundle, routing, depth_bb)
        .map_err(|e| format!("artifact bundle under {bundle_path} not loadable: {e}"))?;
    // 2026-10-01 (F1): search is opt-in. The `--search` CLI flag on
    // ladder/probe/play sets `search_enabled = true`, which carries
    // `EXP-SEARCH` as the auditable G4 ledger token (SPECS/06 §7).
    let mode = AgentMode {
        routing: routing.to_string(),
        search: SearchCfg {
            enabled: search_enabled,
            solver: std::env::var("CHAM_SEARCH_SOLVER").unwrap_or_else(|_| "Rnr".into()),
            g4_ledger_ref: if search_enabled {
                "EXP-SEARCH".into()
            } else {
                String::new()
            },
        },
        fallback_mode: std::env::var("CHAM_FALLBACK_MODE").unwrap_or_else(|_| "renorm".into()),
    };
    let mut router = match std::fs::read(bundle.join("router.bin")) {
        Ok(bytes) => {
            let base = cham_router::runtime::RouterRuntime::from_model_bytes(&bytes)
                .map_err(|e| format!("router model: {e}"))?;
            match overrides {
                Some((temp, n0, beta, z)) => {
                    cham_router::runtime::RouterRuntime::new(base.model.clone(), temp, n0, beta, z)
                }
                None => base,
            }
        }
        Err(_) => {
            // Also honor CHAM_ROUTER_TEMP / CHAM_ROUTER_N0 in the fallback
            // path so a bundle without `router.bin` can still be tested at
            // different sharpening values.
            let (mut temp, mut n0, beta, z) = overrides.unwrap_or((0.7, 8.0, 0.5, -1.5));
            if let Ok(v) = std::env::var("CHAM_ROUTER_TEMP")
                .unwrap_or_default()
                .parse::<f64>()
            {
                temp = v;
            }
            if let Ok(v) = std::env::var("CHAM_ROUTER_N0")
                .unwrap_or_default()
                .parse::<f64>()
            {
                n0 = v;
            }
            cham_router::runtime::RouterRuntime::new(SoftmaxModel::new(20, 4), temp, n0, beta, z)
        }
    };
    // v7 Item 6: changepoint shield is opt-in via --router-changepoint-shield
    // (process-wide flag) so it A/B's cleanly vs fixed-N0.
    // RouterRuntime::new already enables it from the flag/env; the explicit
    // wrapper below covers routers constructed before the flag existed.
    if cham_router::runtime::CHANGEPOINT_FORCE.load(std::sync::atomic::Ordering::SeqCst)
        && !router.changepoint_enabled()
    {
        router = router.with_changepoint_shield(1.0 / 200.0);
    }
    ChameleonAgent::new(
        mode,
        loaded.encoder,
        router,
        loaded.experts,
        loaded.robust,
        loaded.bayes,
        None,
    )
    .map_err(|e| format!("agent: {e}"))
}

/// Counting wrapper: delegates every hook to the inner hero while tallying
/// decisions and `fallback_used` traces (B1 fallback-rate guardrail). The enum
/// keeps a concrete handle on `ChameleonAgent` for trace reads — no downcast,
/// no pointers, no `unsafe` (all forbidden here).
pub enum CountingHero {
    Baseline {
        inner: Box<dyn cham_core::obs::Agent>,
        decisions: u64,
    },
    Chameleon {
        bot: Box<ChameleonAgent>,
        decisions: u64,
        fallbacks: u64,
    },
}

impl CountingHero {
    /// Wrap a hero built by [`build_hero`] (baseline path: no trace reads).
    pub fn new(inner: Box<dyn cham_core::obs::Agent>) -> CountingHero {
        CountingHero::Baseline {
            inner,
            decisions: 0,
        }
    }

    /// Wrap a concrete `ChameleonAgent` with fallback accounting.
    pub fn chameleon(bot: ChameleonAgent) -> CountingHero {
        CountingHero::Chameleon {
            bot: Box::new(bot),
            decisions: 0,
            fallbacks: 0,
        }
    }

    pub fn decisions(&self) -> u64 {
        match self {
            CountingHero::Baseline { decisions, .. } => *decisions,
            CountingHero::Chameleon { decisions, .. } => *decisions,
        }
    }

    pub fn fallbacks(&self) -> u64 {
        match self {
            CountingHero::Baseline { .. } => 0,
            CountingHero::Chameleon { fallbacks, .. } => *fallbacks,
        }
    }
}

impl cham_core::obs::Agent for CountingHero {
    fn name(&self) -> &str {
        match self {
            CountingHero::Baseline { inner, .. } => inner.name(),
            CountingHero::Chameleon { bot, .. } => bot.name(),
        }
    }
    fn act(
        &mut self,
        obs: &cham_core::obs::Observables<'_>,
        rng: &mut cham_core::rng::Rng,
    ) -> cham_core::engine::Action {
        match self {
            CountingHero::Baseline { inner, decisions } => {
                *decisions += 1;
                inner.act(obs, rng)
            }
            CountingHero::Chameleon {
                bot,
                decisions,
                fallbacks,
            } => {
                let a = bot.act(obs, rng);
                *decisions += 1;
                if bot.last_trace.as_ref().is_some_and(|t| t.fallback_used) {
                    *fallbacks += 1;
                }
                a
            }
        }
    }
    fn on_hand_end(&mut self, ph: &cham_core::engine::PublicHistory, hero_net: i64) {
        match self {
            CountingHero::Baseline { inner, .. } => inner.on_hand_end(ph, hero_net),
            CountingHero::Chameleon { bot, .. } => bot.on_hand_end(ph, hero_net),
        }
    }
    fn on_public_action(
        &mut self,
        obs: &cham_core::obs::Observables<'_>,
        player: cham_core::obs::Player,
        action: cham_core::engine::Action,
    ) {
        match self {
            CountingHero::Baseline { inner, .. } => inner.on_public_action(obs, player, action),
            CountingHero::Chameleon { bot, .. } => bot.on_public_action(obs, player, action),
        }
    }
}
