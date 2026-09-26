//! V2 Phase 1.3 — memory probe for `TrainMode::ExploitBayes` (SPECS/04 §5).
//!
//! ExploitBayes trains ONE policy whose infoset key carries a quantized
//! belief bin (13 bins, `modes::N_BINS`). The trained regret table size is
//! therefore bounded by (families × bins), unlike Robust self-play or the
//! per-expert Exploit modes. This probe instantiates ExploitBayes at the
//! tiny-abstraction scale, measures bytes-per-infoset and the process RSS
//! delta across the run, and asserts the total stays under 512 MB.
//!
//! Per plan (V2 §1.3): if the assert fires, record the measured number in
//! the message, tag the test `#[ignore]` with that number, and note
//! "EXP-blocked". Do NOT redesign ExploitBayes to make this pass.

use cham_blueprint::{BeliefBins, ThreadMode, TrainMode, TrainerConfig, train};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::Encoder;

/// Resident-set size of the current process, in bytes, or `None` if it
/// cannot be determined. Linux: `VmRSS` from `/proc/self/status`. macOS:
/// `ps -o rss= -p <pid>` (KB on both platforms).
fn rss_bytes() -> Option<u64> {
    if let Ok(s) = std::fs::read_to_string("/proc/self/status") {
        for line in s.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
                return Some(kb * 1024);
            }
        }
    }
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let kb: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(kb * 1024)
}

#[test]
fn exploit_bayes_memory_probe() {
    const BUDGET_BYTES: u64 = 512 * 1024 * 1024;

    let cfg = AbstractionConfig::tiny();
    let mut enc = Encoder::cfg_only(cfg).expect("encoder");
    let engine_cfg = cham_core::engine::config::EngineConfig::depth(100);

    // 4 families × `BeliefBins::new(4)` exercises the per-session pick logic.
    // Using CallBot 4× keeps the probe hermetic — the memory footprint is a
    // function of the infoset key (families count × bins), not of the
    // opponent's behavior.
    let families = vec![
        cham_opponents::OpponentSpec::CallBot,
        cham_opponents::OpponentSpec::CallBot,
        cham_opponents::OpponentSpec::CallBot,
        cham_opponents::OpponentSpec::CallBot,
    ];
    let mode = TrainMode::ExploitBayes {
        families,
        obs_noise: 0.05,
        bins: BeliefBins::new(4),
    };
    let tcfg = TrainerConfig {
        depth_bb: 100,
        iters: 1_000,
        train_seed: 7,
        snapshot_every: 10_000,
        bayes_session_block: 100,
        regret_discount: 1.0,
    };

    let rss_before = rss_bytes();
    let dir = tempfile::tempdir().expect("tempdir");
    let (table, prov) = train(
        &tcfg,
        &mode,
        engine_cfg,
        &mut enc,
        ThreadMode::Deterministic,
        dir.path(),
        None,
        None,
    )
    .expect("ExploitBayes train");
    let rss_after = rss_bytes();

    let snapshot_bytes = table.snapshot().len() as u64;
    let infosets = (table.len() as u64).max(1);
    let per_infoset = snapshot_bytes / infosets;
    let rss_delta = match (rss_before, rss_after) {
        (Some(b), Some(a)) => a.saturating_sub(b),
        _ => 0,
    };

    eprintln!(
        "ExploitBayes memory probe (V2 Phase 1.3): \
         infosets={infosets} (prov={}), snapshot_bytes={snapshot_bytes}, \
         bytes_per_infoset={per_infoset}, rss_before={rss_before:?}, \
         rss_after={rss_after:?}, rss_delta={rss_delta}",
        prov.infosets,
    );

    // Conservative upper bound: the larger of the process RSS delta and the
    // serialized table size. The RSS delta can be noisy (allocator retention,
    // test-harness allocations); the snapshot bytes are exact.
    let total = rss_delta.max(snapshot_bytes);
    assert!(
        total < BUDGET_BYTES,
        "ExploitBayes memory probe exceeds budget at tiny scale: \
         total={total} bytes ({} MB) > budget={} MB; \
         bytes_per_infoset={per_infoset}, infosets={infosets}, \
         rss_delta={rss_delta}, snapshot_bytes={snapshot_bytes}. \
         Mark this test #[ignore] with the measured number and note \
         'EXP-blocked' — do NOT redesign ExploitBayes to pass.",
        total / (1024 * 1024),
        BUDGET_BYTES / (1024 * 1024),
    );
}
