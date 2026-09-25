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

/// Load an agent bundle from a directory:
/// `abstraction.toml` (+ buckets), `experts/{0..3}/policy.bin`, `robust/policy.bin`,
/// optional `bayes/policy.bin`.
pub fn load_agent(dir: &Path, routing: &str, depth_bb: i64) -> Result<LoadedAgent, AgentError> {
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
        let bp = BlueprintPolicy::load(p.parent().expect("dir"), ab_hash)
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
    let robust = BlueprintPolicy::load(&dir.join("robust"), ab_hash)
        .map_err(|e| AgentError::Loader(format!("robust: {e}")))?;
    if robust.provenance().depth_bb != depth_bb {
        return Err(AgentError::Loader(format!(
            "robust depth {} ≠ requested {depth_bb}",
            robust.provenance().depth_bb
        )));
    }
    hashes.insert("robust".into(), format!("{:x}", robust.artifact_hash()));
    let bayes = match BlueprintPolicy::load(&dir.join("bayes"), ab_hash) {
        Ok(bp) => {
            if bp.provenance().depth_bb != depth_bb {
                return Err(AgentError::Loader("bayes depth mismatch".into()));
            }
            hashes.insert("bayes".into(), format!("{:x}", bp.artifact_hash()));
            Some(bp)
        }
        Err(_) => None,
    };
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
