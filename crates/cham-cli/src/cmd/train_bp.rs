//! `chameleon train-bp` (SPECS/04): blueprint training + status.

use std::path::Path;

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
            }
        }
        other => {
            eprintln!("unknown mode {other} (robust | exploit | exploit-bayes)");
            return crate::cmd::EXIT_FAIL;
        }
    };
    let tcfg = cham_blueprint::TrainerConfig {
        depth_bb: depth,
        iters,
        train_seed: seed,
        snapshot_every: iters.max(10) / 10,
        bayes_session_block: 2000,
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
    let t0 = std::time::Instant::now();
    let (table, prov) = match cham_blueprint::train_with_threads(
        &tcfg,
        &train_mode,
        engine_cfg,
        &mut enc,
        thread_mode,
        threads,
        &runs,
        None,
        None,
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
        thread_mode: "Deterministic".into(),
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
