//! Diagnostic: train a fresh tiny robust, then inspect the internals at the
//! SB root — regret / RM+ current strategy / avg_strategy — for one hand.
//! Separate "RM+ produces pure" from "averaging is broken".
use cham_blueprint::table::ThreadMode;
use cham_blueprint::{TrainMode, TrainerConfig};
use cham_core::card::Deck;
use cham_core::engine::State;
use cham_core::engine::config::EngineConfig;
use cham_core::obs::{Observables, Player};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};

#[test]
fn sb_root_internals() {
    let iters: u64 = std::env::var("SB_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300_000);
    let cfg = AbstractionConfig::tiny();
    let mut enc = Encoder::cfg_only(cfg).expect("enc");
    let tcfg = TrainerConfig {
        depth_bb: 100,
        iters,
        train_seed: 7,
        snapshot_every: iters,
        bayes_session_block: 2000,
        regret_discount: 1.0,
        avg_gamma: 0.9,
            checkpoint_every: 0,
            checkpoint_dir: None,
    };
    let engine_cfg = EngineConfig::depth(100);
    let dir = tempfile::tempdir().unwrap();
    let (table, _prov) = cham_blueprint::train(
        &tcfg,
        &TrainMode::Robust,
        engine_cfg,
        &mut enc,
        ThreadMode::Deterministic,
        dir.path(),
        None,
        None,
    )
    .expect("train");

    // SB root infoset for the first deal.
    let deck = Deck::shuffled(&mut cham_core::rng::rng_from_seed(0xC0FFEE));
    let state = State::new(engine_cfg, deck).unwrap();
    let obs = Observables::view(&state, Player::from_usize(0));
    let seq = ActionSeq::default();
    let slots = enc.slots(&obs, &seq);
    let key = enc.key(&obs, &seq);
    let w = slots.len();
    eprintln!("itertotal={iters}");
    eprintln!("key={key:?} w={w}");
    if let Some(off) = table.find(key.0) {
        let regrets: Vec<f32> = (0..w).map(|a| table.regret(off, w, a)).collect();
        let sig = table.sigma_rms(off, w);
        let avg = table.avg_strategy(off, w);
        let visits = table.visits(off, w);
        eprintln!("visits = {visits}");
        eprintln!("regrets = {:?}", regrets);
        eprintln!("sigma_rm_rms = {:?}", sig);
        eprintln!("avg_strategy = {:?}", avg);
    } else {
        eprintln!("key not present in table");
    }
    // Try a handful of SB root hands, in case the specific seed is weird.
    for seed in [0xAAu64, 0xBB, 0xCC, 0xDD] {
        let deck = Deck::shuffled(&mut cham_core::rng::rng_from_seed(seed));
        let state = State::new(engine_cfg, deck).unwrap();
        let obs = Observables::view(&state, Player::from_usize(0));
        let seq = ActionSeq::default();
        let slots = enc.slots(&obs, &seq);
        let key = enc.key(&obs, &seq);
        let w = slots.len();
        if let Some(off) = table.find(key.0) {
            let sig = table.sigma_rms(off, w);
            let avg = table.avg_strategy(off, w);
            let vis = table.visits(off, w);
            eprintln!(
                "seed {seed:x} hand={:?} visits={vis} sigma={:?} avg={:?}",
                obs.hole, sig, avg
            );
        } else {
            eprintln!("seed {seed:x} hand={:?} UNCOVERED", obs.hole);
        }
    }
}
