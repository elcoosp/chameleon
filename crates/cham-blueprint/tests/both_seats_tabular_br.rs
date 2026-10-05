//! 2026-10-01 late: measure the corrected tabular BR on BOTH seats.
//!
//! The F1 metric doc (`F1-CORRECTED-METRIC-2026-10-01.md`) only reports
//! seat 1. The addendum §7 observed that seat 1's tabular BR turns
//! NEGATIVE on the F3+F4+F6a trainer's artifacts (500k: -1.6 bb/hand,
//! 5M: -3.3 bb/hand). A best response can be negative if the abstraction
//! game value is negative for that seat, but the magnitude is worth
//! contextualizing against seat 0.
//!
//! If seat 0 is strongly positive and seat 1 strongly negative, the
//! abstraction is structurally unbalanced for the two seats — which is
//! a real property of the tiny ladder (cap=1, no 3-bets below jam) and
//! NOT a bug in `tabular_br`. If both are near zero, the numbers are
//! converging on a small game value and the F1 fix is doing its job.
//!
//! Run:
//!   CHAM_EXPLOIT_BP=$PWD/artifacts/par-f5-tiny-5000000/robust-7/policy \
//!   CHAM_EXPLOIT_BUCKETS=$PWD/artifacts/buckets-tiny \
//!   CHAM_EXPLOIT_CONFIG=$PWD/config/abstraction-tiny.toml \
//!   CHAM_EXPLOIT_LABEL=par-f5-tiny-5000000 \
//!     cargo nextest run -p cham-blueprint \
//!       -E 'test(both_seats_tabular_br)' \
//!       --run-ignored all --no-capture

use cham_blueprint::lbr::{lbr_vs, tabular_br};
use cham_blueprint::policy::BlueprintPolicy;
use cham_core::engine::Action;
use cham_core::engine::config::EngineConfig;
use cham_core::obs::Observables;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};

fn make_policy_closure(
    policy: &BlueprintPolicy,
    cfg: AbstractionConfig,
) -> impl FnMut(&Observables<'_>, &ActionSeq) -> Vec<(Action, f64)> + '_ {
    let mut my_enc = Encoder::cfg_only(cfg).expect("policy encoder");
    move |obs: &Observables<'_>, seq: &ActionSeq| -> Vec<(Action, f64)> {
        let slots = my_enc.slots(obs, seq);
        match policy.strategy(obs, &mut my_enc, seq) {
            Some(s) if s.len() == slots.len() => slots
                .iter()
                .zip(s.iter())
                .map(|(slot, p)| (slot.action, *p))
                .collect(),
            _ => {
                let n = obs.legal.len().max(1) as f64;
                obs.legal.iter().map(|l| (l.action, 1.0 / n)).collect()
            }
        }
    }
}

#[test]
#[ignore = "requires a trained policy artifact on disk"]
fn both_seats_tabular_br() {
    let bp_dir = std::env::var("CHAM_EXPLOIT_BP")
        .unwrap_or_else(|_| "artifacts/par-5M/robust-7/policy".to_string());
    let cfg_path = std::env::var("CHAM_EXPLOIT_CONFIG")
        .unwrap_or_else(|_| "config/abstraction-tiny.toml".to_string());
    let label =
        std::env::var("CHAM_EXPLOIT_LABEL").unwrap_or_else(|_| "policy under test".to_string());
    let train_deals: u32 = std::env::var("CHAM_TBR_TRAIN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let test_deals: u32 = std::env::var("CHAM_TBR_TEST")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let sweeps: u32 = std::env::var("CHAM_TBR_SWEEPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    eprintln!("[both_seats] train_deals={train_deals} test_deals={test_deals} sweeps={sweeps}");

    let cfg = std::fs::read_to_string(&cfg_path)
        .ok()
        .and_then(|t| cham_engine::config::parse_config(&t).ok())
        .unwrap_or_else(AbstractionConfig::tiny);

    let policy = BlueprintPolicy::load(std::path::Path::new(&bp_dir), 0).expect("load policy");
    let engine = EngineConfig::depth(100);

    eprintln!();
    eprintln!("=== {label}: BOTH seats, clairvoyant vs tabular ===");
    eprintln!();

    let mut total_tab = 0.0_f64;

    for seat in [0usize, 1usize] {
        let mut enc_lbr = Encoder::cfg_only(cfg.clone()).expect("enc_lbr");
        let mut lbr_policy = make_policy_closure(&policy, cfg.clone());
        let clair = lbr_vs(&mut lbr_policy, seat, engine, &mut enc_lbr, 500, 0x1B2).expect("clair");

        let mut enc_tab = Encoder::cfg_only(cfg.clone()).expect("enc_tab");
        let mut tab_policy = make_policy_closure(&policy, cfg.clone());
        let tab = tabular_br(
            &mut tab_policy,
            seat,
            engine,
            &mut enc_tab,
            train_deals,
            test_deals,
            sweeps,
            0x1B2,
        )
        .expect("tab");

        eprintln!(
            "  seat {seat}: clairvoyant {:>9.1} mb ({:>6.3} bb) | tabular {:>9.1} mb ({:>6.3} +/- {:.3} bb)",
            clair.lbr_mb_per_hand,
            clair.lbr_bb_per_hand,
            tab.lbr_mb_per_hand,
            tab.lbr_bb_per_hand,
            tab.se_bb,
        );

        total_tab += tab.lbr_bb_per_hand;
    }

    eprintln!();
    eprintln!("  (per-seat +/- is the held-out SE; sum SE is the quadrature)",);
    eprintln!(
        "  total corrected (BR(0) + BR(1)): {:.3} bb/hand",
        total_tab
    );
    eprintln!(
        "  (sum < 0 would mean seat-unbalanced policy; sum > 0 means the abstraction favors the trainer)"
    );
    eprintln!();
}
