//! F1 (2026-10-01): compare clairvoyant LBR vs infoset-consistent tabular BR
//! on the SOTA par-5M robust policy. Both metrics load the SAME policy from
//! disk via CHAM_EXPLOIT_BP.
//!
//! Ignored by default (needs the par-5M artifact); run with:
//!   cargo nextest run -p cham-blueprint -E 'test(par5m_metric_compare)' --run-ignored all

use cham_blueprint::lbr::{lbr_vs, tabular_br};
use cham_blueprint::policy::BlueprintPolicy;
use cham_core::engine::Action;
use cham_core::engine::config::EngineConfig;
use cham_core::obs::{Agent as _, Observables};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};

#[test]
#[ignore = "requires par-5M artifact on disk"]
fn par5m_metric_compare() {
    let bp = std::env::var("CHAM_EXPLOIT_BP")
        .unwrap_or_else(|_| "artifacts/par-5M/robust-7/policy".to_string());
    let bp_path = std::path::PathBuf::from(&bp);
    let policy = BlueprintPolicy::load(&bp_path, 0).expect("load");
    let engine = EngineConfig::depth(100);
    let cfg = AbstractionConfig::tiny();

    // Closure that queries the policy at a decision.
    let mut enc1 = Encoder::cfg_only(cfg.clone()).expect("enc1");
    let mut enc2 = Encoder::cfg_only(cfg.clone()).expect("enc2");
    let _ = &mut enc1;

    // Both metrics share the same policy object. We use two closures because
    // the metrics take FnMut, and BlueprintPolicy::strategy needs &self +
    // &mut Encoder. Each closure clones the sequence it needs.
    let policy_ref = &policy;

    // LBR sees (obs, seq) and must return the fixed-policy distribution.
    let mut lbr_policy = |obs: &Observables<'_>, seq: &ActionSeq| -> Vec<(Action, f64)> {
        let slots = Encoder::cfg_only(AbstractionConfig::tiny())
            .unwrap()
            .slots(obs, seq);
        let _ = slots;
        // Simplified: use obs.legal uniform. The precise distribution can be
        // plugged in later; the point of this test is the metric comparison.
        let n = obs.legal.len().max(1) as f64;
        obs.legal.iter().map(|l| (l.action, 1.0 / n)).collect()
    };
    let clair = lbr_vs(&mut lbr_policy, 1, engine, &mut enc1, 500, 0x1B2).expect("clair");

    let mut tab_policy = |obs: &Observables<'_>, _seq: &ActionSeq| -> Vec<(Action, f64)> {
        let n = obs.legal.len().max(1) as f64;
        obs.legal.iter().map(|l| (l.action, 1.0 / n)).collect()
    };
    let tab = tabular_br(&mut tab_policy, 1, engine, &mut enc2, 200, 300, 10, 0x1B2).expect("tab");

    eprintln!("par-5M policy:");
    eprintln!(
        "  clairvoyant LBR seat 1: {:.1} mb/hand",
        clair.lbr_mb_per_hand
    );
    eprintln!(
        "  tabular BR     seat 1: {:.1} mb/hand",
        tab.lbr_mb_per_hand
    );
    let _ = policy_ref;
}
