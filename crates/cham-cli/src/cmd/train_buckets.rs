//! `chameleon train-buckets` (SPECS/02 §3): OFFLINE bucket-table builder.

pub fn run(config: &str, out: &str, profile: &str) -> i32 {
    let text = match std::fs::read_to_string(config) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("read {config}: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    let cfg: cham_engine::config::AbstractionConfig = match toml::from_str(&text) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("parse {config}: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    let params = match profile {
        "full" => cham_engine::build::BuildParams::full(),
        _ => cham_engine::build::BuildParams::tiny(),
    };
    let out_dir = std::path::Path::new(out);
    println!("train-buckets: flop (k={}, sampled={})…", cfg.buckets.flop_k, params.sample_orbits);
    if let Err(e) = cham_engine::build::build_street(&cfg, out_dir, "flop", params) {
        eprintln!("flop build: {e}");
        return crate::cmd::EXIT_FAIL;
    }
    println!("train-buckets: turn (k={})…", cfg.buckets.turn_k);
    if let Err(e) = cham_engine::build::build_street(&cfg, out_dir, "turn", params) {
        eprintln!("turn build: {e}");
        return crate::cmd::EXIT_FAIL;
    }
    if let Err(e) = cham_engine::build::finalize_meta(&cfg, out_dir, params) {
        eprintln!("meta: {e}");
        return crate::cmd::EXIT_FAIL;
    }
    // print artifact hashes (blake3; every artifact-consuming command prints them)
    for f in ["flop.bin", "turn.bin", "meta.json"] {
        let p = out_dir.join(f);
        if let Ok(bytes) = std::fs::read(&p) {
            println!("{f}: {} bytes blake3 {}", bytes.len(), &blake3_hash(&bytes)[..16]);
        }
    }
    println!("train-buckets: done → {out}");
    crate::cmd::EXIT_OK
}

fn blake3_hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_string()
}
