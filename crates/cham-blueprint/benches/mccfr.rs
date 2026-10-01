//! P4 gate (SPECS/00 §6): ES-MCCFR throughput @ 200bb, mid abstraction, single
//! thread. Provisional ≥ 1.5k iters/s; recalibrated by the M-1/M1 spike.

use cham_blueprint::table::{RegretTable, ThreadMode};
use cham_core::engine::config::EngineConfig;
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};
use cham_opponents::archetype::ArchetypeAgent;
use cham_opponents::params::ArchetypeId;
use cham_opponents::percentile::PercentileChart;
use criterion::{Criterion, black_box, criterion_group, criterion_main};

fn bench_mccfr_iter(c: &mut Criterion) {
    let chart = PercentileChart::global();
    let cfg = AbstractionConfig::tiny();
    let mut enc = Encoder::cfg_only(cfg).expect("enc");
    let mut table = RegretTable::new(ThreadMode::Deterministic);
    let mut opp = ArchetypeAgent::point(ArchetypeId::Tag, chart);
    let engine = EngineConfig::depth(200);
    c.bench_function("mccfr_iter_200bb_tiny", |b| {
        b.iter(|| {
            let mut acc = 0.0f64;
            for t in 0..20u64 {
                let rng = &mut rng_from_seed(0xBEEF ^ t);
                let mut state =
                    cham_core::engine::State::new(engine, cham_core::card::Deck::shuffled(rng))
                        .expect("s");
                let mut seq = ActionSeq::default();
                let mut walker = cham_blueprint::traversal::Traversal {
                    table: cham_blueprint::traversal::TableRef::Exclusive(black_box(&mut table)),
                    opp: &mut opp,
                    rbp: cham_blueprint::traversal::RbpConfig::default(),
                    iteration: t,
                    mode: cham_blueprint::modes::TrainModeTag::Exploit,
                    hero_nodes: 0,
                    pruned_nodes: 0,
                    regret_discount: 1.0,
                    allow_insert: true,
                    warmup_only: false,
                    explore_eps: 0.0,
                };
                acc += walker.walk(&mut state, (t % 2) as usize, 1.0, &mut seq, &mut enc, rng);
            }
            acc
        })
    });
}

criterion_group!(
    name = benches;
    // B10.4: ≥ 10 s measurement for the P4 gate bench.
    config = Criterion::default()
        .measurement_time(std::time::Duration::from_secs(10))
        .sample_size(50);
    targets = bench_mccfr_iter
);
criterion_main!(benches);
