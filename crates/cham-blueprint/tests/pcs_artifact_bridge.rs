//! Artifact bridge round-trip: train a small PCS model, convert to the
//! tabular RegretTable, write via `BlueprintPolicy::build_artifact`, load
//! back, and assert the recovered strategies match the PCS averages
//! within quantization tolerance (u16, 1e-4 units).
//!
//! This is the "B path" verification: if it passes, the CLI can drive
//! the PCS trainer end-to-end via the existing artifact writer.

use cham_blueprint::pcs::sampling::sample_board;
use cham_blueprint::pcs::table::RegretTable as PcsTable;
use cham_blueprint::pcs::walk::PcsIteration;
use cham_blueprint::{BlueprintPolicy, ProvenanceRecord, ThreadMode};
use cham_core::engine::config::EngineConfig;
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::Encoder;
use cham_engine::ladder::ActionLadder;
use cham_search::pubtree::PublicTree;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

#[test]
fn pcs_roundtrips_through_artifact() {
    let cfg = AbstractionConfig::tiny();
    let ladder = ActionLadder::new(&cfg);
    let tree = PublicTree::build(CFG, &ladder, 100_000);

    // Two disjoint 4-combo ranges, small enough to train quickly.
    let hero: Vec<[u8; 2]> = (0..4).map(|k| [(2 * k) as u8, (2 * k + 1) as u8]).collect();
    let vill: Vec<[u8; 2]> = (0..4)
        .map(|k| [(26 + 2 * k) as u8, (26 + 2 * k + 1) as u8])
        .collect();
    let hero_rank: Vec<u32> = hero
        .iter()
        .map(|c| (c[0] as u32) << 8 | c[1] as u32)
        .collect();
    let vill_rank: Vec<u32> = vill
        .iter()
        .map(|c| (c[0] as u32) << 8 | c[1] as u32)
        .collect();

    let mut encoder = Encoder::cfg_only(cfg.clone()).expect("enc");
    let mut pcs_table = PcsTable::new();

    let iter = PcsIteration {
        tree: &tree,
        ladder: &ladder,
        hero_range: &hero,
        hero_rank: &hero_rank,
        villain_range: &vill,
        villain_rank: &vill_rank,
        cfg: CFG,
        hero_seat: 1,
    };

    let mut rng = rng_from_seed(0x55);
    for t in 1..=20u64 {
        let board = sample_board(&mut rng);
        iter.run(&mut encoder, &mut pcs_table, board, t, 1.5, 0.0, 2.0);
    }
    eprintln!("pcs_table rows: {}", pcs_table.len());
    assert!(!pcs_table.is_empty(), "PCS table stayed empty");

    // Capture PCS averages before conversion.
    let mut pcs_avgs: Vec<(u64, Vec<f64>)> = Vec::new();
    for (k, row) in pcs_table.iter() {
        let total: f64 = row.strategy_sum.iter().sum();
        let avg: Vec<f64> = if total > 0.0 {
            row.strategy_sum.iter().map(|&s| s / total).collect()
        } else {
            let w = row.strategy_sum.len();
            vec![1.0 / w as f64; w]
        };
        pcs_avgs.push((k, avg));
    }

    // Convert to tabular and write.
    let tabular = pcs_table.to_tabular(ThreadMode::Deterministic);

    let art_dir = tempfile::tempdir().expect("tmpdir");
    let prov = ProvenanceRecord {
        abstraction_hash: 0,
        artifact_hash: 0,
        mode: "pcs-test".to_string(),
        opponent_id: None,
        depth_bb: 100,
        iters: 20,
        train_seed: 0x55,
        thread_mode: "deterministic".to_string(),
        threads: 1,
        parent: None,
        wall_s: 0.0,
        infosets: tabular.len(),
        created_unix: 0,
    };
    BlueprintPolicy::build_artifact(&tabular, &prov, art_dir.path()).expect("build_artifact");

    // Load back and compare to the PCS averages.
    let loaded = BlueprintPolicy::load(art_dir.path(), 0).expect("load");
    let recovered = loaded.export_rows();
    eprintln!("recovered rows: {}", recovered.len());

    // Every PCS key must appear in the loaded artifact.
    let recovered_map: std::collections::HashMap<u64, Vec<f64>> = recovered.into_iter().collect();
    let mut compared = 0usize;
    let mut worst_err = 0.0f64;
    for (k, pcs_avg) in &pcs_avgs {
        let Some(rec) = recovered_map.get(k) else {
            panic!("key {k:#x} from PCS table not in loaded artifact");
        };
        assert_eq!(rec.len(), pcs_avg.len(), "width mismatch at {k:#x}");
        for (a, (&got, &want)) in rec.iter().zip(pcs_avg.iter()).enumerate() {
            let err = (got - want).abs();
            if err > worst_err {
                worst_err = err;
            }
            assert!(
                err < 5e-3,
                "key {k:#x} action {a}: got {got}, want {want}, err {err}"
            );
        }
        compared += 1;
    }
    eprintln!("compared {compared} rows, worst |err| = {worst_err:.6}");
    assert!(compared > 0, "no rows compared");
}
