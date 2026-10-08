//! `chameleon train-pcs` (Phase C): public-chance-sampling DCFR trainer.
//!
//! Design: docs/plans/PHASE-C-PCS-DESIGN-2026-10-08.md. Status and known
//! perf blocker: docs/plans/PHASE-C-STATUS-2026-10-08.md.
//!
//! NOTE: the walk is currently ~1 s/iter at full range (1326 combos per
//! side), so this command REFUSES to launch a run whose estimated wall
//! clock is over the `--wall-budget-s` (default 4h). Pass `--force` only
//! for a diagnostic run you know will not finish.

use std::path::Path;

use cham_blueprint::pcs::trainer::{PcsConfig, run_pcs};
use cham_blueprint::{BlueprintPolicy, ProvenanceRecord};
use cham_core::card::{Card, Hand2};
use cham_core::engine::config::EngineConfig;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::Encoder;
use cham_engine::ladder::ActionLadder;
use cham_search::pubtree::PublicTree;

const ENGINE_CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

/// Rough per-iteration cost estimate. Based on the measured walk
/// (walk_throughput at n=16, n=64, n=256 in pcs_walk_bench.rs): the
/// per-combo-per-node cost is ~1 us, the tree has ~4124 nodes, and the
/// walk visits two players' combos so total per-iteration work scales
/// as `2 * combos * 4124 * 1us`. This is an order-of-magnitude guard,
/// not a benchmark; if it is wrong it is wrong on the safe side.
fn estimate_wall_s(iters: u64, combos: usize) -> f64 {
    let per_iter_s = 2.0 * combos as f64 * 4124.0 * 1e-6;
    iters as f64 * per_iter_s
}

/// Build disjoint hero / villain ranges from two halves of the deck.
/// Hero draws from cards 0..26, villain from 26..52. Width capped at
/// C(26,2) = 325 per side.
fn split_ranges(n: usize) -> (Vec<[u8; 2]>, Vec<[u8; 2]>) {
    assert!(n <= 325, "max 325 combos per side (C(26,2))");
    let mut hero = Vec::with_capacity(n);
    let mut vill = Vec::with_capacity(n);
    'h: for a in 0..26u8 {
        for b in (a + 1)..26u8 {
            hero.push([a, b]);
            if hero.len() == n {
                break 'h;
            }
        }
    }
    'v: for a in 26..52u8 {
        for b in (a + 1)..52u8 {
            vill.push([a, b]);
            if vill.len() == n {
                break 'v;
            }
        }
    }
    (hero, vill)
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    iters: u64,
    seed: u64,
    config_path: &str,
    buckets_path: &str,
    out: &str,
    combos: usize,
    dcfr_alpha: f64,
    dcfr_beta: f64,
    dcfr_gamma: f64,
    wall_budget_s: u64,
    log_every: u64,
    force: bool,
) -> i32 {
    // Load abstraction.
    let cfg = match std::fs::read_to_string(config_path)
        .ok()
        .and_then(|t| cham_engine::config::parse_config(&t).ok())
    {
        Some(c) => c,
        None => {
            eprintln!("train-pcs: cannot load {config_path}, using tiny defaults");
            AbstractionConfig::tiny()
        }
    };

    // Load encoder.
    let mut encoder = match Encoder::from_artifacts_dir(Path::new(buckets_path), cfg.clone()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("train-pcs: from_artifacts_dir err {e:?}; using cfg_only");
            Encoder::cfg_only(cfg.clone()).expect("cfg_only")
        }
    };

    let ladder = ActionLadder::new(&cfg);
    let tree = PublicTree::build(ENGINE_CFG, &ladder, 100_000);
    eprintln!("train-pcs: tree has {} nodes", tree.len());

    // Ranges.
    let (hero, villain) = split_ranges(combos);
    eprintln!(
        "train-pcs: {} hero / {} villain combos (from disjoint half-decks)",
        hero.len(),
        villain.len()
    );

    // Wall-budget guard.
    let est_s = estimate_wall_s(iters, combos);
    let est_h = est_s / 3600.0;
    eprintln!(
        "train-pcs: estimated wall = {est_h:.1} h ({est_s:.0} s) for {iters} iters at {} combos/side",
        combos
    );
    if !force && est_s > wall_budget_s as f64 {
        eprintln!(
            "train-pcs: REFUSING — estimate {est_h:.1} h exceeds --wall-budget-s {}",
            wall_budget_s
        );
        eprintln!("train-pcs: rerun with --force if you know what you are doing");
        return 1;
    }

    // Rank function: river_equity against the sampled board, scaled to u32.
    let rank_fn = |board: &[Card; 5], range: &[[u8; 2]]| -> Vec<u32> {
        range
            .iter()
            .map(|c| {
                let h = Hand2::new(Card(c[0]), Card(c[1]));
                (cham_engine::tables::river_equity(h, board) * 1e6) as u32
            })
            .collect()
    };

    // Train.
    let pcs_cfg = PcsConfig {
        iters,
        seed,
        dcfr_alpha,
        dcfr_beta,
        dcfr_gamma,
        log_every,
    };
    let t0 = std::time::Instant::now();
    let pcs_table = run_pcs(
        &tree,
        &ladder,
        &hero,
        &villain,
        ENGINE_CFG,
        1,
        rank_fn,
        &mut encoder,
        &pcs_cfg,
    );
    let wall_s = t0.elapsed().as_secs_f64();

    // Convert to tabular and write artifact.
    let tabular = pcs_table.to_tabular(cham_blueprint::ThreadMode::Deterministic);
    let art_dir = Path::new(out).join("robust");
    if let Err(e) = std::fs::create_dir_all(&art_dir) {
        eprintln!("train-pcs: cannot create {}: {e}", art_dir.display());
        return 1;
    }

    let prov = ProvenanceRecord {
        abstraction_hash: encoder.abstraction_hash(),
        artifact_hash: 0,
        mode: "pcs".into(),
        opponent_id: None,
        depth_bb: 100,
        iters,
        train_seed: seed,
        thread_mode: "deterministic".into(),
        threads: 1,
        parent: None,
        wall_s,
        infosets: tabular.len(),
        created_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    };
    if let Err(e) = BlueprintPolicy::build_artifact(&tabular, &prov, &art_dir) {
        eprintln!("train-pcs: build_artifact failed: {e}");
        return 1;
    }

    let policy_path = art_dir.join("policy.bin");
    let bytes = std::fs::metadata(&policy_path)
        .map(|m| m.len())
        .unwrap_or(0);
    println!(
        "train-pcs: wrote {} ({} infosets, {} bytes, {:.1}s wall)",
        policy_path.display(),
        tabular.len(),
        bytes,
        wall_s
    );
    println!();
    println!("To measure with the D1 harness, run:");
    println!("  CHAM_D1_BP={}/robust \\", out);
    println!("  CHAM_D1_CONFIG={} \\", config_path);
    println!("  CHAM_D1_BUCKETS={} \\", buckets_path);
    println!("    target/debug/deps/d1_fullgame_vbr-<hash> --ignored --nocapture");
    println!();
    println!(
        "The D2 baseline to beat is 5.77 +/- 0.55 bb (see docs/plans/D1-RESULT-2026-10-08.md)."
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_scales_linearly() {
        let a = estimate_wall_s(1000, 30);
        let b = estimate_wall_s(2000, 30);
        assert!((b / a - 2.0).abs() < 1e-9);
    }

    #[test]
    fn split_ranges_are_disjoint() {
        let (h, v) = split_ranges(10);
        assert_eq!(h.len(), 10);
        assert_eq!(v.len(), 10);
        for hc in &h {
            for vc in &v {
                assert!(
                    hc[0] != vc[0] && hc[0] != vc[1] && hc[1] != vc[0] && hc[1] != vc[1],
                    "overlap: {hc:?} vs {vc:?}"
                );
            }
        }
    }
}
