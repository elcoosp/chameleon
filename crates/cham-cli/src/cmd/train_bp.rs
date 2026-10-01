//! `chameleon train-bp` (SPECS/04): blueprint training + status.

use std::path::Path;

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let from = entry.path();
        let to = dst.join(&name);
        let ft = entry.file_type()?;
        if ft.is_dir() {
            copy_dir_all(&from, &to)?;
        } else if ft.is_file() {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

pub fn run(
    mode: &str,
    opponent: Option<&str>,
    seed: u64,
    depth: i64,
    iters: u64,
    out: &str,
    status: Option<&str>,
    threads: Option<u32>,
    thread_mode: Option<&str>,
    config: Option<&str>,
    buckets: Option<&str>,
    regret_discount: f32,
    avg_gamma: f32,
    reuse: bool,
    resume_from: Option<&str>,
    cache_dir: Option<&str>,
    checkpoint_every: u64,
    checkpoint_dir: Option<&str>,
) -> i32 {
    if let Some(run_dir) = status {
        return print_status(run_dir);
    }
    let engine_cfg = cham_core::engine::config::EngineConfig::depth(depth);
    // Load the abstraction from the file the agent loader will also parse
    // (hash parity requirement). Overridable via --config so we can train
    // against the full abstraction without editing the default.
    let config_path = config.unwrap_or("config/abstraction-tiny.toml");
    let cfg = match std::fs::read_to_string(config_path)
        .ok()
        .and_then(|t| cham_engine::config::parse_config(&t).ok())
    {
        Some(c) => c,
        None => {
            eprintln!("config: cannot load {config_path}, falling back to tiny in-code defaults");
            cham_engine::config::AbstractionConfig::tiny()
        }
    };
    let buckets_dir = buckets.unwrap_or("artifacts/buckets-tiny");
    let mut enc =
        match cham_engine::Encoder::from_artifacts_dir(Path::new(buckets_dir), cfg.clone()) {
            Ok(e) => e,
            Err(_) => cham_engine::Encoder::cfg_only(cfg.clone()).expect("enc"),
        };
    let train_mode = match mode {
        "robust" => cham_blueprint::TrainMode::Robust,
        "exploit" | "exploit-bayes" => {
            let opp_id = opponent.unwrap_or("callbot");
            let opp = match cham_opponents::OpponentSpec::parse(opp_id) {
                Ok(o) => o,
                Err(e) => {
                    eprintln!("opponent: {e}");
                    return crate::cmd::EXIT_FAIL;
                }
            };
            cham_blueprint::TrainMode::Exploit {
                opponent: opp,
                jitter_seed: seed,
                frozen: None,
            }
        }
        other => {
            eprintln!("unknown mode {other} (robust | exploit | exploit-bayes)");
            return crate::cmd::EXIT_FAIL;
        }
    };
    // F9 (2026-10-01): read CHAM_TRAIN_EPS at the CLI boundary and
    // populate the config field. The blueprint crate no longer reads it.
    let explore_eps = std::env::var("CHAM_TRAIN_EPS")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|v| (0.0..=0.5).contains(v))
        .unwrap_or(0.0);
    let tcfg = cham_blueprint::TrainerConfig {
        depth_bb: depth,
        iters,
        train_seed: seed,
        snapshot_every: iters.max(10) / 10,
        bayes_session_block: 2000,
        regret_discount,
        dcfr_alpha: 1.0,
        dcfr_beta: 1.0,
        avg_gamma,
        checkpoint_every,
        checkpoint_dir: if checkpoint_every > 0 {
            Some(
                checkpoint_dir
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| std::path::PathBuf::from(out).join("checkpoints")),
            )
        } else {
            None
        },
        explore_eps,
    };
    let runs = std::path::Path::new(out).join(format!("{mode}-{seed}"));

    let thread_mode = match thread_mode.unwrap_or("deterministic") {
        "deterministic" => cham_blueprint::ThreadMode::Deterministic,
        "hogwild" => cham_blueprint::ThreadMode::Hogwild,
        "snapbatch" => cham_blueprint::ThreadMode::Snapbatch,
        other => {
            eprintln!("unknown --thread-mode {other} (deterministic | hogwild | snapbatch)");
            return crate::cmd::EXIT_FAIL;
        }
    };
    // PERF-PLAN T5: worker count defaults to available parallelism (on Apple
    // M1 4 workers usually beats 8 for this memory-bound workload);
    // --threads overrides.
    let threads = threads.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n: std::num::NonZeroUsize| n.get() as u32)
            .unwrap_or(4)
    });

    // ---- V2 A/B training cache ----
    let cache_root = cache_dir
        .map(std::path::PathBuf::from)
        .unwrap_or_else(cham_blueprint::train_cache::default_cache_dir);
    if reuse {
        let mode_tag = match mode {
            "robust" => "Robust".to_string(),
            other => format!("Exploit:{other}"),
        };
        let opp_id = if mode == "robust" {
            None
        } else {
            opponent.map(str::to_string)
        };
        let cfg_peek = cham_blueprint::TrainerConfig {
            depth_bb: depth,
            iters,
            train_seed: seed,
            snapshot_every: iters.max(10) / 10,
            bayes_session_block: 2000,
            regret_discount,
            dcfr_alpha: 1.0,
            dcfr_beta: 1.0,
            avg_gamma,
            checkpoint_every: 0,
            checkpoint_dir: None,
            explore_eps: 0.0,
        };
        let key = cham_blueprint::train_cache::train_cache_key(
            &cfg_peek,
            &mode_tag,
            opp_id.as_deref(),
            enc.abstraction_hash(),
            &format!("{:?}", thread_mode),
            threads,
            resume_from.is_some(),
        );
        if let Some(cached) = cham_blueprint::train_cache::lookup(&cache_root, &key) {
            println!(
                "train-bp: REUSE hit {mode}-{seed} key={key} from {}",
                cached.display()
            );
            let _ = std::fs::remove_dir_all(&runs);
            if let Some(parent) = runs.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = copy_dir_all(&cached, &runs) {
                eprintln!("train-bp: reuse copy failed: {e}");
                return crate::cmd::EXIT_FAIL;
            }
            println!("train-bp: reused at {}", runs.display());
            return crate::cmd::EXIT_OK;
        }
        println!("train-bp: cache miss key={key}");
    }
    let t0 = std::time::Instant::now();
    let resume_path = resume_from.map(std::path::Path::new);
    if let Some(p) = resume_path {
        println!("train-bp: resuming from {}", p.display());
    }
    let (table, prov) = match cham_blueprint::train_with_threads(
        &tcfg,
        &train_mode,
        engine_cfg,
        &mut enc,
        thread_mode,
        threads,
        &runs,
        None,
        resume_path,
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("train: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    // build the quantized inference artifact
    let art_dir = runs.join("policy");
    let record = cham_blueprint::ProvenanceRecord {
        abstraction_hash: prov.abstraction_hash,
        artifact_hash: 0,
        mode: format!("{:?}", prov.mode),
        opponent_id: prov.opponent_id.clone(),
        depth_bb: depth,
        iters,
        train_seed: seed,
        // L-9 fix (2026-09-27): previously hardcoded "Deterministic"
        // regardless of the actual `--thread-mode`. The artifact provenance
        // is bound into the ledger; recording the wrong mode made the
        // ledger an auditability lie. Write the ACTUAL mode.
        thread_mode: format!("{thread_mode:?}"),
        threads,
        parent: None,
        wall_s: prov.wall_s,
        infosets: table.len(),
        created_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    };
    if let Err(e) = cham_blueprint::BlueprintPolicy::build_artifact(&table, &record, &art_dir) {
        eprintln!("artifact: {e}");
        return crate::cmd::EXIT_FAIL;
    }
    println!(
        "train-bp: {mode} iters={iters} depth={depth} seed={seed} infosets={} wall={:.1}s artifact={}",
        table.len(),
        t0.elapsed().as_secs_f64(),
        art_dir.display()
    );
    if let Ok(bytes) = std::fs::read(art_dir.join("policy.bin")) {
        println!(
            "policy.bin: {} bytes blake3 {}",
            bytes.len(),
            &blake3::hash(&bytes).to_string()[..16]
        );
    }
    // ---- V2 A/B training cache: store the fresh artifact ----
    // Recompute key (cheap hash) so we don't have to hoist it out of the
    // reuse branch above. Store is idempotent: first writer wins.
    if reuse {
        let mode_tag = match mode {
            "robust" => "Robust".to_string(),
            other => format!("Exploit:{other}"),
        };
        let opp_id = if mode == "robust" {
            None
        } else {
            opponent.map(str::to_string)
        };
        let cfg_peek = cham_blueprint::TrainerConfig {
            depth_bb: depth,
            iters,
            train_seed: seed,
            snapshot_every: iters.max(10) / 10,
            bayes_session_block: 2000,
            regret_discount,
            dcfr_alpha: 1.0,
            dcfr_beta: 1.0,
            avg_gamma,
            checkpoint_every: 0,
            checkpoint_dir: None,
            explore_eps: 0.0,
        };
        let key = cham_blueprint::train_cache::train_cache_key(
            &cfg_peek,
            &mode_tag,
            opp_id.as_deref(),
            enc.abstraction_hash(),
            &format!("{:?}", thread_mode),
            threads,
            resume_from.is_some(),
        );
        if let Err(e) = cham_blueprint::train_cache::store(&cache_root, &key, &runs) {
            eprintln!("train-bp: cache store skipped: {e}");
        } else {
            println!("train-bp: cached at {}/{key}", cache_root.display());
        }
    }
    crate::cmd::EXIT_OK
}

fn print_status(run_dir: &str) -> i32 {
    let prov = Path::new(run_dir).join("provenance.json");
    match std::fs::read_to_string(&prov) {
        Ok(text) => {
            println!("{text}");
            crate::cmd::EXIT_OK
        }
        Err(e) => {
            eprintln!("status: {e}");
            crate::cmd::EXIT_FAIL
        }
    }
}
