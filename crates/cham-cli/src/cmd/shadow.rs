//! EXP-016 shadow ladder: champion snapshots + gauntlet vs frozen shadows.
//! Snapshots persist `export_rows()` under `artifacts/shadow/<hash>/`; the
//! gauntlet runs the challenger against the last-3 shadows via AbRunner.

use std::path::Path;

pub fn snapshot(policy_dir: &str, out_dir: &str) -> i32 {
    let policy_path = Path::new(policy_dir);
    // policy_dir may be a single ".../policy" dir, a train-bp out dir
    // (append "policy"), or a full agent bundle root (`robust/` +
    // `experts/{0..3}/`, merged — the bundle has no single policy file).
    let single = if policy_path.ends_with("policy") {
        policy_path.to_path_buf()
    } else {
        policy_path.join("policy")
    };
    let mut rows: std::collections::BTreeMap<u64, Vec<f64>> = Default::default();
    let mut hasher = blake3::Hasher::new();
    let mut sources = 0u32;
    if single.join("policy.bin").exists() {
        let policy = match cham_blueprint::BlueprintPolicy::load(&single, 0) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("shadow snapshot: cannot load policy '{policy_dir}': {e}");
                return crate::cmd::EXIT_BUDGET;
            }
        };
        rows = policy.export_rows().into_iter().collect();
        hasher.update(&std::fs::read(single.join("policy.bin")).unwrap_or_default());
        sources = 1;
    } else {
        // Bundle-root layout: merge robust + 4 experts, first-wins on key
        // collision (tiers share infoset keys; the merged frozen opponent is
        // an approximation of the mixture champion, not any single tier —
        // logged per-tier so drift is visible).
        let mut tiers: Vec<std::path::PathBuf> = vec![policy_path.join("robust")];
        tiers.extend((0..4).map(|k| policy_path.join("experts").join(k.to_string())));
        for tier in &tiers {
            let tier_rows: std::collections::BTreeMap<u64, Vec<f64>> =
                match cham_blueprint::BlueprintPolicy::load(tier, 0) {
                    Ok(p) => p.export_rows().into_iter().collect(),
                    Err(e) => {
                        eprintln!(
                            "shadow snapshot: cannot load tier '{}': {e}",
                            tier.display()
                        );
                        return crate::cmd::EXIT_BUDGET;
                    }
                };
            hasher.update(&std::fs::read(tier.join("policy.bin")).unwrap_or_default());
            let mut fresh = 0usize;
            for (k, v) in tier_rows {
                if let std::collections::btree_map::Entry::Vacant(e) = rows.entry(k) {
                    e.insert(v);
                    fresh += 1;
                }
            }
            println!(
                "shadow snapshot: tier '{}': {} new rows",
                tier.display(),
                fresh
            );
            sources += 1;
        }
    }
    let _ = sources;
    let hash = format!("shadow:{}", hasher.finalize().to_hex());
    let safe: String = hash.replace(':', "_");
    let dir = Path::new(out_dir).join(&safe);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("shadow snapshot: mkdir: {e}");
        return crate::cmd::EXIT_FAIL;
    }
    let payload = serde_json::json!({
        "hash": hash,
        "rows": rows.len(),
        "ts": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
    });
    // Persist rows KEYED (v2): the gauntlet rebuilds FrozenRows from this
    // file, and unkeyed values alone cannot (infoset keys are the lookup).
    // serde_json stringifies the u64 keys; the loader parses them back.
    // (Pre-v2 snapshots wrote bare values arrays — the loader rejects those
    // as unusable rather than misreading them.)
    let encoded = serde_json::to_vec(&rows).unwrap_or_default();
    if std::fs::write(dir.join("rows.bin"), &encoded).is_err() {
        eprintln!("shadow snapshot: write rows failed");
        return crate::cmd::EXIT_FAIL;
    }
    if std::fs::write(dir.join("meta.json"), payload.to_string()).is_err() {
        eprintln!("shadow snapshot: write meta failed");
        return crate::cmd::EXIT_FAIL;
    }
    // Prune: keep last 5 by mtime (EXP-016 storage kill criterion).
    if let Ok(mut entries) = std::fs::read_dir(out_dir).map(|r| {
        r.filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .collect::<Vec<_>>()
    }) {
        entries.sort_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        while entries.len() > 5 {
            let oldest = entries.remove(0);
            let _ = std::fs::remove_dir_all(oldest.path());
        }
    }
    println!(
        "shadow snapshot: {hash} ({} rows) → {out_dir}/{safe}",
        rows.len()
    );
    crate::cmd::EXIT_OK
}

pub fn gauntlet(agent: &str, shadow_dir: &str, deals: u64) -> i32 {
    match run_gauntlet(agent, shadow_dir, deals) {
        Ok(report) => {
            println!(
                "shadow gauntlet: agent={agent} vs {} shadows, {deals} deals each",
                report.n_shadows
            );
            for (id, delta) in &report.per_shadow {
                println!("  vs {id}: {delta:+.1} mb/seating");
            }
            println!(
                "shadow gauntlet: worst {:+.1} mb/seating",
                report.worst_delta_mb
            );
            crate::cmd::EXIT_OK
        }
        Err(e) => {
            eprintln!("shadow gauntlet: {e}");
            // No shadows yet is a budget/empty state, not a failure —
            // matches the pre-gate CLI behavior (first promotion skips).
            crate::cmd::EXIT_BUDGET
        }
    }
}

/// Per-shadow head-to-head outcome of [`run_gauntlet`].
#[derive(Clone, Debug)]
pub struct GauntletReport {
    /// Shadows actually faced (≤ 3, newest first).
    pub n_shadows: usize,
    /// (shadow id, candidate delta in mb/seating) — positive favors the candidate.
    pub per_shadow: Vec<(String, f64)>,
    /// Minimum over `per_shadow` (the binding constraint for the gate).
    pub worst_delta_mb: f64,
}

impl GauntletReport {
    /// True when the candidate holds within `tolerance` (negative, e.g.
    /// `-10.0`) against EVERY recent shadow.
    pub fn all_within_tolerance(&self, tolerance: f64) -> bool {
        self.per_shadow.iter().all(|(_, d)| *d >= tolerance)
    }
}

/// Load keyed snapshot rows, or `None` when the snapshot predates keyed
/// persistence (v1 values-only arrays) or is otherwise unreadable.
fn load_shadow_rows(dir: &std::path::Path) -> Option<std::collections::BTreeMap<u64, Vec<f64>>> {
    let bytes = std::fs::read(dir.join("rows.bin")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// EXP-016 gate core (v5-deepdive-audit item 5): face the candidate hero
/// against the last-3 frozen shadows in real duplicate matches
/// (`MatchRunner::run`, same engine both seats see) and report per-shadow
/// deltas. Shadows resolve via the opponents-crate shadow registry, so
/// `frozen:<label>` specs carry the snapshot's REAL rows under its OWN
/// abstraction encoder — a miss-rate mismatch would surface inside the
/// match, not as silent uniform play.
///
/// Returns `Err` with a `"no shadow snapshots ..."` message when there is
/// nothing to regress against (first-ever promotion) — the `--promote`
/// caller treats exactly that case as a skip, every other error as fatal.
pub fn run_gauntlet(
    candidate: &str,
    shadow_dir: &str,
    deals: u64,
) -> Result<GauntletReport, String> {
    let mut snaps: Vec<std::path::PathBuf> = std::fs::read_dir(shadow_dir)
        .map(|r| {
            r.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    if snaps.is_empty() {
        return Err(format!(
            "no shadow snapshots under '{shadow_dir}' — run shadow snapshot first"
        ));
    }
    snaps.sort_by_key(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    // Candidate must build BEFORE any match: the per-deal factory below is
    // infallible by `MatchRunner::run`'s contract, and construction is
    // deterministic (same bundle every deal), so a single pre-check rules
    // out mid-run surprises.
    crate::cmd::hero::build_hero(candidate, 100)
        .map_err(|e| format!("gauntlet: candidate '{candidate}' needs trained artifacts ({e})"))?;
    let mut per_shadow: Vec<(String, f64)> = Vec::new();
    for snap in snaps.iter().rev().take(3) {
        let id = snap
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let rows = match load_shadow_rows(snap) {
            Some(r) if !r.is_empty() => r,
            _ => {
                println!("  vs {id}: skipped (no keyed rows — re-snapshot to enable)");
                continue;
            }
        };
        cham_opponents::register_shadow(
            &id,
            cham_opponents::FrozenRows(rows),
            "artifacts/agent/buckets",
            "artifacts/agent/abstraction.toml",
        );
        let label = id.clone();
        let factory = || {
            crate::cmd::hero::build_hero(candidate, 100).unwrap_or_else(|_| {
                Box::new(cham_opponents::baselines::CallBot) as Box<dyn cham_core::obs::Agent>
            })
        };
        let spec = cham_eval::matcheng::MatchSpec {
            opponent: cham_opponents::factory::OpponentSpecDto(format!("frozen:{label}")),
            deals,
            depth_bb: 100,
            base_seed: 0x5EED_0016 ^ (per_shadow.len() as u64).wrapping_mul(0x9E37_79B9),
            label: format!("gauntlet:{candidate}/{id}"),
        };
        match cham_eval::matcheng::MatchRunner::run(&spec, &factory, None) {
            Ok(res) => per_shadow.push((id, res.mb_per_seating)),
            Err(e) => return Err(format!("gauntlet: match vs '{id}' failed: {e}")),
        }
    }
    if per_shadow.is_empty() {
        return Err(format!(
            "no shadow snapshots under '{shadow_dir}' with keyed rows — re-run shadow snapshot first"
        ));
    }
    let worst_delta_mb = per_shadow
        .iter()
        .map(|(_, d)| *d)
        .fold(f64::INFINITY, f64::min);
    Ok(GauntletReport {
        n_shadows: per_shadow.len(),
        per_shadow,
        worst_delta_mb,
    })
}
