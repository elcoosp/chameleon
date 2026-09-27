//! Artifact loader (SPECS/07 §6): integrity (blake3 per artifact, abstraction hash
//! agreement) and DEPTH FLEXIBILITY — every blueprint's `depth_bb` must equal the
//! requested play depth (mixed-depth set → hard error).

use std::path::Path;

use serde::{Deserialize, Serialize};

use cham_blueprint::policy::BlueprintPolicy;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::Encoder;

use crate::AgentError;

/// agent_load record payload (SPECS/12 §3 kind `agent_load`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentLoadRecord {
    pub mode: String,
    pub artifact_hashes: std::collections::BTreeMap<String, String>,
    pub depth_bb: i64,
    pub experts: Vec<String>,
    pub abstraction_hash: String,
}

/// A loaded, integrity-checked agent bundle.
pub struct LoadedAgent {
    pub encoder: Encoder,
    pub experts: Vec<BlueprintPolicy>, // 4 specialists
    pub robust: BlueprintPolicy,
    pub bayes: Option<BlueprintPolicy>,
    pub record: AgentLoadRecord,
}

/// Default pre-load memory budget in MB (B8; 16 GB machines stay safe with
/// headroom for the OS + search working set). Override for tests via
/// `CHAMELEON_MEMORY_BUDGET_MB`.
pub const DEFAULT_MEMORY_BUDGET_MB: u64 = 8192;

/// Effective memory budget in MB.
pub fn memory_budget_mb() -> u64 {
    std::env::var("CHAMELEON_MEMORY_BUDGET_MB")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_MEMORY_BUDGET_MB)
}

/// Estimate a bundle's resident bytes BEFORE allocating (B8): sum of the
/// `policy.bin` file sizes plus the bucket-table files, from metadata alone.
/// Returns `(total_bytes, largest_contributor_path)`.
pub fn estimate_bundle_bytes(dir: &Path) -> Result<(u64, String), AgentError> {
    let mut total = 0u64;
    let mut largest = (0u64, String::new());
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for i in 0..4 {
        paths.push(dir.join(format!("experts/{i}/policy.bin")));
    }
    paths.push(dir.join("robust/policy.bin"));
    paths.push(dir.join("bayes/policy.bin")); // optional: missing files count 0
    let buckets = dir.join("buckets");
    if buckets.is_dir() {
        if let Ok(rd) = std::fs::read_dir(&buckets) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_file() {
                    paths.push(p);
                }
            }
        }
    }
    for p in &paths {
        let n = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        total += n;
        if n > largest.0 {
            largest = (n, p.display().to_string());
        }
    }
    Ok((total, largest.1))
}

/// Refuse a bundle whose estimate exceeds `budget_mb` (B8): names the largest
/// contributor so the operator knows what to shrink. Pure check — no I/O
/// beyond file sizes.
pub fn check_budget(dir: &Path, budget_mb: u64) -> Result<u64, AgentError> {
    let (total, largest) = estimate_bundle_bytes(dir)?;
    let budget_bytes = budget_mb.saturating_mul(1024 * 1024);
    if total > budget_bytes {
        return Err(AgentError::Loader(format!(
            "bundle under {} refuses: estimated {} MB > budget {} MB (largest: {} at {} bytes)",
            dir.display(),
            total / (1024 * 1024),
            budget_mb,
            if largest.is_empty() { "?" } else { &largest },
            total,
        )));
    }
    Ok(total)
}

/// Load an agent bundle from a directory:
/// `abstraction.toml` (+ buckets), `experts/{0..3}/policy.bin`, `robust/policy.bin`,
/// optional `bayes/policy.bin`.
pub fn load_agent(dir: &Path, routing: &str, depth_bb: i64) -> Result<LoadedAgent, AgentError> {
    load_agent_with_budget(dir, routing, depth_bb, memory_budget_mb())
}

/// Load with an explicit memory budget in MB (the B8 guard runs first).
pub fn load_agent_with_budget(
    dir: &Path,
    routing: &str,
    depth_bb: i64,
    budget_mb: u64,
) -> Result<LoadedAgent, AgentError> {
    check_budget(dir, budget_mb)?;
    let toml_path = dir.join("abstraction.toml");
    let toml_text = std::fs::read_to_string(&toml_path)?;
    let cfg: AbstractionConfig =
        toml::from_str(&toml_text).map_err(|e| AgentError::Loader(format!("toml: {e}")))?;
    cfg.validate()
        .map_err(|e| AgentError::Loader(format!("{e}")))?;
    let encoder = Encoder::from_config(cfg, &dir.join("buckets"))
        .map_err(|e| AgentError::Loader(format!("encoder: {e}")))?;
    let ab_hash = encoder.abstraction_hash();

    let mut experts = Vec::new();
    let mut hashes = std::collections::BTreeMap::new();
    for i in 0..4 {
        let p = dir.join(format!("experts/{i}/policy.bin"));
        let expected_ab = if std::env::var("CHAM_IGNORE_ABSTRACTION_HASH").as_deref() == Ok("1") {
            eprintln!(
                "cham-agent: WARNING — CHAM_IGNORE_ABSTRACTION_HASH=1 is set; \
                 skipping abstraction-hash verification for diagnostics. The \
                 loaded bundle's keys may not match its trained abstraction; \
                 never use this in a gate."
            );
            0 // 0 = "skip the check" per BlueprintPolicy::load
        } else {
            ab_hash
        };
        let bp = BlueprintPolicy::load(p.parent().expect("dir"), expected_ab)
            .map_err(|e| AgentError::Loader(format!("expert {i}: {e}")))?;
        if bp.provenance().depth_bb != depth_bb {
            return Err(AgentError::Loader(format!(
                "expert {} depth {} ≠ requested {depth_bb} (mixed-depth set)",
                i,
                bp.provenance().depth_bb
            )));
        }
        hashes.insert(format!("expert{i}"), format!("{:x}", bp.artifact_hash()));
        experts.push(bp);
    }
    let robust_ab = if std::env::var("CHAM_IGNORE_ABSTRACTION_HASH").as_deref() == Ok("1") {
        0
    } else {
        ab_hash
    };
    let robust = BlueprintPolicy::load(&dir.join("robust"), robust_ab)
        .map_err(|e| AgentError::Loader(format!("robust: {e}")))?;
    if robust.provenance().depth_bb != depth_bb {
        return Err(AgentError::Loader(format!(
            "robust depth {} ≠ requested {depth_bb}",
            robust.provenance().depth_bb
        )));
    }
    hashes.insert("robust".into(), format!("{:x}", robust.artifact_hash()));
    // M-10 fix (2026-09-27): a MISSING bayes artifact is legitimate
    // ("this bundle has no bayes expert") — but a CORRUPT one is a hard
    // error. The previous `Err(_) => None` made the two indistinguishable:
    // a tampered/truncated bayes/policy.bin loaded "successfully" with
    // bayes=None, and bayes routing silently played uniform. We now
    // pre-check the file's existence: if it doesn't exist → None; if it
    // exists but fails to load → propagate the error.
    let bayes_dir = dir.join("bayes");
    let bayes_bin = bayes_dir.join("policy.bin");
    let bayes = if !bayes_bin.exists() {
        None
    } else {
        let bayes_ab = if std::env::var("CHAM_IGNORE_ABSTRACTION_HASH").as_deref() == Ok("1") {
            0
        } else {
            ab_hash
        };
        let bp = BlueprintPolicy::load(&bayes_dir, bayes_ab).map_err(|e| {
            AgentError::Loader(format!(
                "bayes/policy.bin exists but failed to load (corrupt or wrong \
                 abstraction): {e}"
            ))
        })?;
        if bp.provenance().depth_bb != depth_bb {
            return Err(AgentError::Loader(format!(
                "bayes depth {} ≠ requested {depth_bb}",
                bp.provenance().depth_bb
            )));
        }
        hashes.insert("bayes".into(), format!("{:x}", bp.artifact_hash()));
        Some(bp)
    };

    // M-10 second half: 'bayes' routing requires a bayes expert. A bundle
    // that declares routing=bayes but ships no bayes artifact is an
    // inconsistency the operator should see NOW, not at the first
    // inference call (which would silently use uniform-greedy).
    if routing == "bayes" && bayes.is_none() {
        return Err(AgentError::Loader(
            "routing='bayes' but no bayes/policy.bin in the bundle".into(),
        ));
    }

    let record = AgentLoadRecord {
        mode: routing.to_string(),
        artifact_hashes: hashes,
        depth_bb,
        experts: experts
            .iter()
            .map(|e| format!("{:x}", e.artifact_hash()))
            .collect(),
        abstraction_hash: format!("{ab_hash:x}"),
    };
    Ok(LoadedAgent {
        encoder,
        experts,
        robust,
        bayes,
        record,
    })
}
