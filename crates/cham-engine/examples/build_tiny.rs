//! Build the committed M1-tiny bucket artifacts into artifacts/buckets-tiny/.
//! Run: cargo run -p cham-engine --example build_tiny --release

use std::path::Path;

use cham_engine::build::{build_street, finalize_meta, BuildParams};
use cham_engine::config::AbstractionConfig;

fn main() {
    let cfg = AbstractionConfig::tiny();
    cfg.validate().expect("cfg");
    let out = Path::new("artifacts/buckets-tiny");
    std::fs::create_dir_all(out).expect("dir");
    let params = BuildParams::tiny();
    let t0 = std::time::Instant::now();
    let flop = build_street(&cfg, out, "flop", params).expect("flop");
    println!("flop: {:?} ({:?})", flop, t0.elapsed());
    let turn = build_street(&cfg, out, "turn", params).expect("turn");
    println!("turn: {:?} ({:?})", turn, t0.elapsed());
    finalize_meta(&cfg, out, params).expect("meta");
    println!("meta committed; total {:?}", t0.elapsed());
    for f in ["flop.bin", "turn.bin", "meta.json"] {
        let p = out.join(f);
        println!("{f}: {} bytes", std::fs::metadata(&p).expect("meta").len());
    }
}
