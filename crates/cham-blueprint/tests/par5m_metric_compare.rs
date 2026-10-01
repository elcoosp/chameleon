//! F1 (2026-10-01): the clairvoyant LBR vs the infoset-consistent tabular BR
//! on the shipped par-5M robust policy. Both metrics query the SAME loaded
//! policy through a closure that maintains its own encoder.
//!
//! Run with the par-5M artifact present:
//!   CHAM_EXPLOIT_BP=$PWD/artifacts/par-5M/robust-7/policy \
//!     cargo nextest run -p cham-blueprint -E 'test(par5m_metric_compare)' --run-ignored all --no-capture
//!
//! Expected: tabular BR < clairvoyant LBR by a large factor. On a uniform
//! policy the report's kernel measured a 6x reduction; a trained policy
//! should be similar or larger.

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
    // The closure owns its encoder — strategy() needs an &mut Encoder for
    // per-decision caches. Fresh instance because lbr_vs/tabular_br also hold
    // their own encoders.
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
                // No row, or shape mismatch: uniform over legal.
                let n = obs.legal.len().max(1) as f64;
                obs.legal.iter().map(|l| (l.action, 1.0 / n)).collect()
            }
        }
    }
}

#[test]
#[ignore = "requires par-5M artifact on disk"]
fn par5m_metric_compare() {
    let bp_dir = std::env::var("CHAM_EXPLOIT_BP")
        .unwrap_or_else(|_| "artifacts/par-5M/robust-7/policy".to_string());
    let cfg_path = std::env::var("CHAM_EXPLOIT_CONFIG")
        .unwrap_or_else(|_| "config/abstraction-tiny.toml".to_string());

    // Load the abstraction config the policy was trained against.
    let cfg = std::fs::read_to_string(&cfg_path)
        .ok()
        .and_then(|t| cham_engine::config::parse_config(&t).ok())
        .unwrap_or_else(AbstractionConfig::tiny);

    let policy = BlueprintPolicy::load(std::path::Path::new(&bp_dir), 0).expect("load policy");
    let engine = EngineConfig::depth(100);

    // Two independent encoders so the metrics do not share caches.
    let mut enc_lbr = Encoder::cfg_only(cfg.clone()).expect("enc_lbr");
    let mut enc_tab = Encoder::cfg_only(cfg.clone()).expect("enc_tab");

    // Clairvoyant LBR seat 1.
    let mut lbr_policy = make_policy_closure(&policy, cfg.clone());
    let clair = lbr_vs(&mut lbr_policy, 1, engine, &mut enc_lbr, 500, 0x1B2).expect("clair");

    // Tabular BR seat 1.
    let mut tab_policy = make_policy_closure(&policy, cfg);
    let tab = tabular_br(
        &mut tab_policy,
        1,
        engine,
        &mut enc_tab,
        300,
        200,
        12,
        0x1B2,
    )
    .expect("tab");

    eprintln!();
    eprintln!("=== par-5M robust: clairvoyant vs tabular (seat 1) ===");
    eprintln!(
        "  clairvoyant LBR:  {:>8.1} mb/hand ({:.3} bb/hand)",
        clair.lbr_mb_per_hand, clair.lbr_bb_per_hand
    );
    eprintln!(
        "  tabular BR:       {:>8.1} mb/hand ({:.3} bb/hand)",
        tab.lbr_mb_per_hand, tab.lbr_bb_per_hand
    );
    eprintln!(
        "  clairvoyant / tabular ratio: {:.2}x",
        clair.lbr_mb_per_hand / tab.lbr_mb_per_hand.max(1e-9)
    );
    eprintln!();

    assert!(
        tab.lbr_mb_per_hand <= clair.lbr_mb_per_hand + 1.0,
        "tabular BR must be ≤ clairvoyant LBR on the same policy"
    );
}
