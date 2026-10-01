//! Content-keyed blueprint training cache (V2 A/B speedup).
//!
//! A trained blueprint is a pure function of its inputs, all of which are
//! captured in `RunProvenance`. Reusing an artifact across A/B runs that
//! leave the blueprint unchanged turns a 30-60 s train into a file read.
//!
//! Cache layout:
//!   artifacts/blueprints-cache/<key>/
//!     policy/policy.bin
//!     policy/provenance.json
//!     provenance.json
//!     table.snap
//!
//! `<key>` = hex of blake3 over `TRAIN_CACHE_VERSION` and everything that
//! feeds into the trained policy: abstraction hash, mode tag, opponent id,
//! depth, iters, seed, thread mode, threads, regret discount. It does NOT
//! include wall-clock, host, or path — the same inputs on a different day
//! hit the same key by construction.
//!
//! Version bump rules: any change to trainer math, serializer, or the
//! provenance schema requires a bump of `TRAIN_CACHE_VERSION`.

use std::io;
use std::path::{Path, PathBuf};

use crate::TrainerConfig;

/// Bump on any change to trainer semantics or cache layout.
///
/// M-5 fix (2026-09-27): v1 → v2. The v1 key omitted `bayes_session_block`,
/// the four environment overrides that silently change training
/// (`CHAM_RBP_THETA0`, `CHAM_EXPLORE_EPS`, `CHAM_FORCE_SEAT`,
/// `CHAM_AVG_UNIFORM`), and whether the run was a `--resume-from` (a resumed
/// table differs from a fresh one yet was stored under the fresh-train key).
/// Same key → silently reused wrong artifact in A/B flows. Any older
/// cache entry is now orphaned; re-train once.
pub const TRAIN_CACHE_VERSION: u32 = 2;

/// Default cache root (git-ignored).
pub fn default_cache_dir() -> PathBuf {
    PathBuf::from("artifacts/blueprints-cache")
}

/// Compute the cache key for one training run.
///
/// `mode_tag`, `opponent_id`, `abstraction_hash`, `regret_discount`,
/// `thread_mode`, and `threads` are the inputs that change the trained
/// artifact but are not in `TrainerConfig`. `cfg` contributes depth, iters,
/// seed.
/// Compute the cache key for one training run.
///
/// M-5 fix (2026-09-27): the key now also covers
///   * `cfg.bayes_session_block` — it changes belief-bin sampling cadence
///     and (via the H-8 fix) the concrete family agent rebuild rate;
///   * the four training-relevant env overrides that change the produced
///     artifact but were silently invisible to the cache:
///     `CHAM_RBP_THETA0`, `CHAM_EXPLORE_EPS`, `CHAM_FORCE_SEAT`,
///     `CHAM_AVG_UNIFORM`;
///   * whether the run was a `--resume-from` (a resumed table is a
///     different function of its inputs than a fresh one — same
///     abstraction/mode/seed but a different starting point).
#[allow(clippy::too_many_arguments)]
pub fn train_cache_key(
    cfg: &TrainerConfig,
    mode_tag: &str,
    opponent_id: Option<&str>,
    abstraction_hash: u64,
    thread_mode: &str,
    threads: u32,
    is_resume: bool,
) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = rustc_hash::FxHasher::default();
    TRAIN_CACHE_VERSION.hash(&mut h);
    cfg.depth_bb.hash(&mut h);
    cfg.iters.hash(&mut h);
    cfg.train_seed.hash(&mut h);
    cfg.regret_discount.to_bits().hash(&mut h);
    cfg.avg_gamma.to_bits().hash(&mut h);
    // M-5: previously omitted.
    cfg.bayes_session_block.hash(&mut h);
    cfg.snapshot_every.hash(&mut h);
    cfg.checkpoint_every.hash(&mut h);
    mode_tag.hash(&mut h);
    opponent_id.unwrap_or("").hash(&mut h);
    abstraction_hash.hash(&mut h);
    thread_mode.hash(&mut h);
    threads.hash(&mut h);
    // M-5: resume is a distinct training path.
    is_resume.hash(&mut h);
    // M-5: environment overrides that change the trained artifact. Hash the
    // raw string (empty when unset), never the parsed number — a malformed
    // value that falls back to a default is a distinct input from unset.
    for var in [
        "CHAM_RBP_THETA0",
        "CHAM_EXPLORE_EPS",
        "CHAM_FORCE_SEAT",
        "CHAM_AVG_UNIFORM",
    ] {
        var.hash(&mut h);
        std::env::var(var).unwrap_or_default().hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

/// Path to the cache entry for a given key.
pub fn cache_entry_dir(cache_root: &Path, key: &str) -> PathBuf {
    cache_root.join(key)
}

/// A cache entry is considered present when both `provenance.json` and
/// `policy/policy.bin` exist (an interrupted write leaves one missing).
pub fn lookup(cache_root: &Path, key: &str) -> Option<PathBuf> {
    let dir = cache_entry_dir(cache_root, key);
    let prov = dir.join("provenance.json");
    let policy = dir.join("policy").join("policy.bin");
    if prov.is_file() && policy.is_file() {
        Some(dir)
    } else {
        None
    }
}

/// Copy a freshly-trained run directory into the cache (atomic).
///
/// `run_dir` is the directory `train_with_threads` wrote (contains
/// `provenance.json`, `policy/`, `table.snap`). We copy its contents into
/// `<cache_root>/<key>/` via a `.tmp` rename so a concurrent reader never
/// sees a half-written entry.
pub fn store(cache_root: &Path, key: &str, run_dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(cache_root)?;
    let final_dir = cache_entry_dir(cache_root, key);
    if final_dir.is_dir() {
        return Ok(()); // already present, first-writer wins
    }
    let tmp_dir = cache_root.join(format!("{key}.tmp.{}", std::process::id()));
    if tmp_dir.exists() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
    std::fs::create_dir_all(&tmp_dir)?;
    copy_dir_all(run_dir, &tmp_dir)?;
    // rename is atomic on the same filesystem
    match std::fs::rename(&tmp_dir, &final_dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            Err(e)
        }
    }
}

/// Copy a training run directory (shallow recursive; the layout is fixed:
/// top-level files + `policy/` subdir).
fn copy_dir_all(src: &Path, dst: &Path) -> io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let from = entry.path();
        let to = dst.join(&name);
        let ft = entry.file_type()?;
        if ft.is_dir() {
            std::fs::create_dir_all(&to)?;
            copy_dir_all(&from, &to)?;
        } else if ft.is_file() {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(iters: u64) -> TrainerConfig {
        TrainerConfig {
            depth_bb: 100,
            iters,
            train_seed: 7,
            snapshot_every: 10,
            bayes_session_block: 2000,
            regret_discount: 1.0,
            dcfr_alpha: 1.0,
            dcfr_beta: 1.0,
            avg_gamma: 0.9,
            checkpoint_every: 0,
            checkpoint_dir: None,
            explore_eps: 0.0,
        }
    }

    #[test]
    fn same_inputs_same_key() {
        let a = train_cache_key(
            &cfg(1000),
            "Robust",
            None,
            0xCAFE,
            "Deterministic",
            4,
            false,
        );
        let b = train_cache_key(
            &cfg(1000),
            "Robust",
            None,
            0xCAFE,
            "Deterministic",
            4,
            false,
        );
        assert_eq!(a, b);
    }

    #[test]
    fn different_iters_different_key() {
        let a = train_cache_key(
            &cfg(1000),
            "Robust",
            None,
            0xCAFE,
            "Deterministic",
            4,
            false,
        );
        let b = train_cache_key(
            &cfg(2000),
            "Robust",
            None,
            0xCAFE,
            "Deterministic",
            4,
            false,
        );
        assert_ne!(a, b);
    }

    #[test]
    fn different_discount_different_key() {
        let mut c1 = cfg(1000);
        let mut c2 = cfg(1000);
        c1.regret_discount = 0.9;
        c2.regret_discount = 1.0;
        let a = train_cache_key(&c1, "Robust", None, 0xCAFE, "Deterministic", 4, false);
        let b = train_cache_key(&c2, "Robust", None, 0xCAFE, "Deterministic", 4, false);
        assert_ne!(a, b);
    }

    /// M-5 fix (2026-09-27): these cases were the actual bug. Each of the
    /// inputs below changes the trained artifact but was absent from the v1
    /// cache key, so two DIFFERENT runs were stored under the same key and
    /// the second silently reused the first.
    #[test]
    fn m5_semantic_inputs_change_key() {
        let base = cfg(1000);
        let k = |c: &TrainerConfig, resume: bool| {
            train_cache_key(c, "Robust", None, 0xCAFE, "Deterministic", 4, resume)
        };
        // bayes_session_block
        let mut other = base.clone();
        other.bayes_session_block = base.bayes_session_block * 2;
        assert_ne!(
            k(&base, false),
            k(&other, false),
            "bayes_session_block must be keyed"
        );
        // resume flag
        assert_ne!(
            k(&base, false),
            k(&base, true),
            "resume-vs-fresh must be keyed"
        );
    }

    #[test]
    fn roundtrip_store_lookup() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let run = dir.path().join("run");
        std::fs::create_dir_all(run.join("policy")).unwrap();
        std::fs::write(run.join("provenance.json"), b"{}").unwrap();
        std::fs::write(run.join("policy").join("policy.bin"), b"x").unwrap();
        std::fs::write(run.join("policy").join("provenance.json"), b"{}").unwrap();

        let key = "deadbeef";
        assert!(lookup(root, key).is_none());
        store(root, key, &run).unwrap();
        let got = lookup(root, key).expect("hit");
        assert!(got.join("policy/policy.bin").is_file());
        assert!(got.join("provenance.json").is_file());
    }
}
