//! Eval guardrails (PERF-PLAN T7): refuse to ladder/probe/ab with untrained
//! artifacts, and surface fallback-rate warnings into the ledger.
//!
//! Symptom this fixes: `ladder --fast` rows `callbot +0.0 ± 0.0` that are
//! exact mirror matches — the `full` agent silently fell back (uniform policy)
//! and strength numbers were meaningless. Strength numbers from an unevaluated
//! (fallback) pipeline look like bot bugs; refusing loudly is the fix.

/// Agent names that require the trained `artifacts/agent` bundle (mirrors
/// `play`'s routing set in `cmd::play`: every one of these loads blueprints).
const TRAINED_AGENTS: [&str; 14] = [
    "full",
    "no-search",
    "full-no-search",
    "argmax",
    "full-argmax",
    "no-search-argmax",
    "full-mixture",
    "full-hedged",
    "hedged",
    "sample-expert",
    "full-sample-expert",
    "robust-only",
    "bayes",
    "mixture",
];

/// True when `agent` needs trained policy/router artifacts to mean anything.
pub fn requires_trained_artifacts(agent: &str) -> bool {
    TRAINED_AGENTS.contains(&agent)
}

/// The full list of routable agent names. Used by `ladder`/`probe`/`ab` to
/// refuse unknown names with a clear error rather than silently falling
/// through to a baseline (see STALE-BINARY-GOTCHA-2026-10-01.md).
pub fn trained_agents() -> &'static [&'static str] {
    &TRAINED_AGENTS
}

/// The pure-baseline agent names accepted by `ladder`/`probe`/`ab` that do
/// NOT require a trained bundle. Kept intentionally small.
pub const BASELINE_AGENTS: &[&str] =
    &["callbot", "raisebot", "jamfix", "random", "fish", "uniform"];

/// True when `agent` is a recognized name for evaluation tools — either a
/// trained routing mode or a known pure baseline. A `false` result means a
/// typo, a stale binary, or an agent name that hasn't been wired up yet;
/// callers should refuse LOUDLY rather than fall through to CallBot.
pub fn is_known_agent(agent: &str) -> bool {
    requires_trained_artifacts(agent) || BASELINE_AGENTS.contains(&agent)
}

/// Required bundle files, mirroring `cham_agent::loader::load_agent`
/// (`abstraction.toml` + buckets + 4 experts + robust).
fn required_bundle_files() -> Vec<std::path::PathBuf> {
    // Respect the CHAM_AGENT_BUNDLE override (see hero.rs) so the guard
    // agrees with the loader about which bundle we are about to use.
    let base: std::path::PathBuf = std::env::var("CHAM_AGENT_BUNDLE")
        .unwrap_or_else(|_| "artifacts/agent".to_string())
        .into();
    let base = base.as_path();
    let mut out = vec![base.join("abstraction.toml"), base.join("buckets")];
    for i in 0..4 {
        out.push(base.join(format!("experts/{i}/policy.bin")));
    }
    out.push(base.join("robust/policy.bin"));
    out
}

/// Missing bundle files (empty when the bundle is loadable).
pub fn missing_artifact_files() -> Vec<String> {
    required_bundle_files()
        .into_iter()
        .filter(|p| !p.exists())
        .map(|p| p.display().to_string())
        .collect()
}

/// Refuse evaluation with an untrained bundle: `Ok(())` when the agent is a
/// pure baseline or its bundle exists; `Err(missing)` listing the absent
/// files otherwise (caller prints them and exits nonzero).
pub fn require_agent_artifacts(agent: &str) -> Result<(), Vec<String>> {
    if !requires_trained_artifacts(agent) {
        return Ok(());
    }
    let missing = missing_artifact_files();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}

/// Fallback-rate warning threshold: above 20% fallback decisions the run's
/// strength numbers are meaningless (uniform-policy contamination).
pub const FALLBACK_WARN_RATE: f64 = 0.20;

/// Content identity of the artifact bundle bound to `agent` (v3 §2.2: fills
/// `LedgerEntry::artifact_hash` so every gate number is auditable).
///
/// * Pure baselines (no trained artifacts): `baseline:<mode>` — deterministic
///   by construction (the code IS the artifact, pinned by the git revision
///   the ledger consumer records separately).
/// * Trained modes: `blake3:<hex>` over the concatenated bytes of every file
///   in [`required_bundle_files`] (missing files → `None`, so callers that
///   already passed `require_agent_artifacts` never see `None`, and callers
///   that didn't get a loud signal instead of a silent unaudited number).
pub fn artifact_identity(agent: &str) -> Option<String> {
    if !requires_trained_artifacts(agent) {
        return Some(format!("baseline:{agent}"));
    }
    let mut h = blake3::Hasher::new();
    for p in required_bundle_files() {
        let bytes = std::fs::read(p).ok()?;
        h.update(&bytes);
    }
    Some(format!("blake3:{}", h.finalize().to_hex()))
}

/// Check a run's fallback rate. Returns the prominent warning when
/// `fallback / total` exceeds [`FALLBACK_WARN_RATE`]; the caller prints it
/// and writes it into the ledger entry. `total == 0` (no traced decisions,
/// e.g. pure-baseline factories) yields `None` — nothing to warn about.
pub fn check_fallback_rate(label: &str, fallback: u64, total: u64) -> Option<String> {
    if total == 0 {
        return None;
    }
    let rate = fallback as f64 / total as f64;
    if rate > FALLBACK_WARN_RATE {
        Some(format!(
            "WARNING: {label} fell back on {fallback}/{total} decisions ({:.1}%) — artifacts missing or stale, strength numbers are meaningless",
            rate * 100.0,
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trained_agent_set_matches_play_routing() {
        for a in [
            "full",
            "argmax",
            "robust-only",
            "bayes",
            "mixture",
            "no-search",
            "full-hedged",
            "hedged",
        ] {
            assert!(requires_trained_artifacts(a), "{a} needs artifacts");
        }
        for a in ["callbot", "fish", "arch:tag", "random"] {
            assert!(!requires_trained_artifacts(a), "{a} is a pure baseline");
        }
    }

    /// Anti-regression (2026-09-30): every agent name accepted by
    /// `hero::routing_for` (i.e. every mode the CLI can route to) must
    /// require trained artifacts, or the ladder/probe silently falls
    /// through to CallBot. This bug was found after `full-hedged` was
    /// missing from TRAINED_AGENTS: four ladder runs at different
    /// thresholds produced identical outputs because all four were
    /// actually measuring CallBot.
    #[test]
    fn every_routable_agent_requires_artifacts() {
        // The full set of aliases that hero::routing_for recognizes.
        for a in [
            "full",
            "no-search",
            "full-no-search",
            "full-argmax",
            "argmax",
            "no-search-argmax",
            "full-mixture",
            "mixture",
            "full-hedged",
            "hedged",
            "robust-only",
            "bayes",
        ] {
            assert!(
                requires_trained_artifacts(a),
                "{a} is a routable agent but not in TRAINED_AGENTS — \
                 ladder/probe will silently use CallBot"
            );
        }
    }

    #[test]
    fn fallback_warning_threshold() {
        assert!(
            check_fallback_rate("x", 0, 0).is_none(),
            "no decisions → no warning"
        );
        assert!(check_fallback_rate("x", 0, 100).is_none());
        assert!(
            check_fallback_rate("x", 20, 100).is_none(),
            "exactly 20% is not above"
        );
        let w = check_fallback_rate("x", 21, 100).expect("21% must warn");
        assert!(
            w.contains("WARNING") && w.contains("21"),
            "prominent warning: {w}"
        );
    }
}
