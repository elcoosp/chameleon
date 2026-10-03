//! §3.3(3): same-abstraction BR vs fine-information BR.
//! Ratio same:fine ~ 1 => coarse BR honest; >> 1 => "~0" is an artifact.
use cham_blueprint::lbr::{tabular_br, tabular_br_fine};
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
#[ignore = "requires CHAM_EXPLOIT_BP"]
fn fine_br() {
    let bp = std::env::var("CHAM_EXPLOIT_BP")
        .unwrap_or_else(|_| "artifacts/par-5M/robust-7/policy".into());
    let cfgp = std::env::var("CHAM_EXPLOIT_CONFIG")
        .unwrap_or_else(|_| "config/abstraction-tiny.toml".into());
    let label = std::env::var("CHAM_EXPLOIT_LABEL").unwrap_or_else(|_| "policy".into());
    let tr: u32 = std::env::var("CHAM_TBR_TRAIN").ok().and_then(|v| v.parse().ok()).unwrap_or(5000);
    let te: u32 = std::env::var("CHAM_TBR_TEST").ok().and_then(|v| v.parse().ok()).unwrap_or(500);
    let sw: u32 = std::env::var("CHAM_TBR_SWEEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(30);
    let cfg = std::fs::read_to_string(&cfgp).ok()
        .and_then(|t| cham_engine::config::parse_config(&t).ok())
        .unwrap_or_else(AbstractionConfig::tiny);
    let mut fine_cfg = cfg.clone();
    fine_cfg.buckets.flop_k = 300;
    fine_cfg.buckets.turn_k = 200;
    fine_cfg.buckets.river_eq_bins = 64;
    fine_cfg.buckets.river_texture_classes = 8;
    let policy = BlueprintPolicy::load(std::path::Path::new(&bp), 0).expect("load");
    let eng = EngineConfig::depth(100);
    eprintln!("\n=== {label}: same-abstraction vs fine-key BR (fine=300/200/64/8 proxy) ===");
    let mut ssum = 0.0f64;
    let mut fsum = 0.0f64;
    for seat in [0usize, 1usize] {
        let mut e1 = Encoder::cfg_only(cfg.clone()).expect("e1");
        let mut p1 = make_policy_closure(&policy, cfg.clone());
        let s = tabular_br(&mut p1, seat, eng, &mut e1, tr, te, sw, 0x1B2).expect("same");
        let mut e2 = Encoder::cfg_only(cfg.clone()).expect("e2");
        let mut ke = Encoder::cfg_only(fine_cfg.clone()).expect("ke");
        let mut p2 = make_policy_closure(&policy, cfg.clone());
        let f = tabular_br_fine(&mut p2, seat, eng, &mut e2, &mut ke, tr, te, sw, 0x1B2).expect("fine");
        eprintln!("  seat {seat}: same {:>8.3} +/- {:.3} | fine {:>8.3} +/- {:.3}",
            s.lbr_bb_per_hand, s.se_bb, f.lbr_bb_per_hand, f.se_bb);
        ssum += s.lbr_bb_per_hand;
        fsum += f.lbr_bb_per_hand;
    }
    eprintln!("  SUM same {ssum:.3} bb | fine {fsum:.3} bb");
    eprintln!();
}
