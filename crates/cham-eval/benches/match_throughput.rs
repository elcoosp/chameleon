//! P6 gate (SPECS/00 §6): full-pipeline match throughput ≥ 60k seatings/min
//! aggregate on 4 threads (recalibrated at M1).

use cham_eval::matcheng::{MatchRunner, MatchSpec};
use cham_core::obs::Agent;
use cham_opponents::factory::OpponentSpecDto;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_match(c: &mut Criterion) {
    let spec = MatchSpec {
        opponent: OpponentSpecDto("callbot".into()),
        deals: 20,
        depth_bb: 100,
        base_seed: 1,
        label: "bench".into(),
    };
    let factory = || -> Box<dyn Agent> { Box::new(cham_opponents::baselines::CallBot) };
    c.bench_function("match_20_deals", |b| {
        b.iter(|| {
            let r = MatchRunner::run(black_box(&spec), &factory, None).expect("run");
            r.seatings
        })
    });
}

criterion_group!(benches, bench_match);
criterion_main!(benches);
