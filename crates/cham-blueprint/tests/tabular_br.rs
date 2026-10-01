//! F1 (2026-10-01): the clairvoyant LBR overestimates exploitability; the
//! infoset-consistent tabular BR is a lower bound. On the same policy,
//! tabular BR ≤ clairvoyant LBR by construction (the tabular BR is
//! restricted to one action per infoset; the clairvoyant BR can pick a
//! different action per deal).

use cham_blueprint::lbr::{lbr_vs, tabular_br};
use cham_core::engine::Action;
use cham_core::engine::config::EngineConfig;
use cham_core::obs::Observables;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};

fn uniform_policy(obs: &Observables<'_>, _seq: &ActionSeq) -> Vec<(Action, f64)> {
    let n = obs.legal.len().max(1) as f64;
    obs.legal.iter().map(|l| (l.action, 1.0 / n)).collect()
}

/// The clairvoyant BR is an UPPER bound; the tabular BR is a LOWER bound.
/// They must satisfy tabular ≤ clairvoyant on the same policy.
#[test]
fn tabular_le_clairvoyant_on_uniform() {
    let engine = EngineConfig::depth(50);
    let cfg = AbstractionConfig::tiny();

    let mut enc1 = Encoder::cfg_only(cfg.clone()).expect("enc1");
    let mut p1 = uniform_policy;
    let clair = lbr_vs(&mut p1, 1, engine, &mut enc1, 100, 0x1B2).expect("clair");

    let mut enc2 = Encoder::cfg_only(cfg).expect("enc2");
    let mut p2 = uniform_policy;
    let tab = tabular_br(&mut p2, 1, engine, &mut enc2, 100, 100, 8, 0x1B2).expect("tab");

    eprintln!(
        "uniform: clairvoyant {:.3} bb/hand vs tabular {:.3} bb/hand",
        clair.lbr_bb_per_hand, tab.lbr_bb_per_hand
    );
    assert!(
        tab.lbr_bb_per_hand <= clair.lbr_bb_per_hand + 1e-9,
        "tabular BR ({}) must be ≤ clairvoyant BR ({})",
        tab.lbr_bb_per_hand,
        clair.lbr_bb_per_hand
    );
}
