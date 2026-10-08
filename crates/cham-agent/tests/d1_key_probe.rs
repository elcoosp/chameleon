//! Key probe: dump a policy's stored keys and the key the encoder
//! computes at a known root villain node, to diagnose a strategy() miss.

use cham_blueprint::policy::BlueprintPolicy;
use cham_core::card::{Card, Deck};
use cham_core::engine::State;
use cham_core::engine::config::EngineConfig;
use cham_core::obs::{Observables, Player};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn d1_key_probe() {
    let bp_dir = std::env::var("CHAM_D1_BP")
        .unwrap_or_else(|_| "artifacts/agent-honest-19dim/robust".into());
    let policy = BlueprintPolicy::load(std::path::Path::new(&bp_dir), 0).expect("load");
    let bundle = std::path::Path::new(&bp_dir).parent().unwrap();

    let cfg_path = std::env::var("CHAM_D1_CONFIG")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| bundle.join("abstraction.toml"));
    let cfg = std::fs::read_to_string(&cfg_path)
        .ok()
        .and_then(|t| cham_engine::config::parse_config(&t).ok())
        .unwrap_or_else(AbstractionConfig::tiny);

    eprintln!();
    eprintln!("=== config / env ===");
    eprintln!("  cfg_path:                 {}", cfg_path.display());
    eprintln!("  cfg.version:              {}", cfg.version);
    eprintln!("  cfg.ladder.slot_bucket:   {}", cfg.ladder.slot_bucket);
    eprintln!("  cfg.compress_history:     {}", cfg.compress_history);
    eprintln!(
        "  env CHAM_SLOT_BUCKET:     {:?}",
        std::env::var("CHAM_SLOT_BUCKET").ok()
    );
    eprintln!(
        "  env CHAM_COMPRESS_HISTORY:{:?}",
        std::env::var("CHAM_COMPRESS_HISTORY").ok()
    );

    let bk = std::env::var("CHAM_D1_BUCKETS")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| bundle.join("buckets"));
    let mut enc = match Encoder::from_artifacts_dir(&bk, cfg.clone()) {
        Ok(e) => {
            eprintln!("  encoder: from_artifacts_dir OK");
            e
        }
        Err(e) => {
            eprintln!("  encoder: from_artifacts_dir err {e:?} -> cfg_only");
            Encoder::cfg_only(cfg.clone()).unwrap()
        }
    };
    eprintln!(
        "  enc.abstraction_hash:     {:#018x}",
        enc.abstraction_hash()
    );

    let rows = policy.export_rows();
    eprintln!();
    eprintln!("=== policy stored keys ===");
    eprintln!("  row count: {}", rows.len());
    let mut sorted = true;
    for i in 1..rows.len() {
        if rows[i].0 <= rows[i - 1].0 {
            sorted = false;
            break;
        }
    }
    eprintln!("  strictly sorted: {}", sorted);
    eprintln!("  first 5:");
    for (k, d) in rows.iter().take(5) {
        eprintln!("    {:#018x}  w={}", k, d.len());
    }
    eprintln!("  last 3:");
    for (k, d) in rows.iter().rev().take(3) {
        eprintln!("    {:#018x}  w={}", k, d.len());
    }

    // Root villain node, preflop. Board irrelevant preflop.
    let board = [Card(20), Card(21), Card(22), Card(23), Card(24)];
    let vh = [0u8, 1];
    let dh = [2u8, 3];
    let prefix = [
        Card(vh[0]),
        Card(dh[0]),
        Card(vh[1]),
        Card(dh[1]),
        board[0],
        board[1],
        board[2],
        board[3],
        board[4],
    ];
    let st = State::new(CFG, Deck::with_prefix(&prefix)).unwrap();
    let obs = Observables::view(&st, Player::from_usize(0));
    let seq = ActionSeq::default();
    let key = enc.key(&obs, &seq);
    eprintln!();
    eprintln!("=== probe root key (villain preflop, no actions) ===");
    eprintln!("  key:      {:#018x}", key.0);
    eprintln!("  street:   {:?}", obs.street);
    eprintln!("  player:   {:?}", obs.player);
    eprintln!("  pot:      {}", obs.pot);
    eprintln!("  to_call:  {}", obs.to_call);
    eprintln!("  stack:    {}", obs.stack);

    let got = policy.strategy(&obs, &mut enc, &seq);
    eprintln!(
        "  strategy: {:?}",
        got.as_ref().map(|v| (v.len(), v.iter().sum::<f64>()))
    );

    // Same node, but with an artificial belief_bin=1 (tests whether
    // belief is what shifts the key).
    let mut enc2 = Encoder::from_artifacts_dir(&bk, cfg.clone()).unwrap();
    enc2.set_belief_bin(1);
    let obs2 = Observables::view(&st, Player::from_usize(0));
    let key2 = enc2.key(&obs2, &seq);
    eprintln!("  belief_bin=1 key: {:#018x}", key2.0);
}
