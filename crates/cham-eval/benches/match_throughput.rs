//! P6 gate (SPECS/00 §6): full-pipeline match throughput ≥ 60k seatings/min
//! aggregate on 4 threads (recalibrated at M1).
//!
//! NOTE (post-B1): the pre-B1 baseline was 26 µs; the current ~47 µs is the
//! B1 per-seating lifecycle cost (HandHistory + PublicHistory construction and
//! two on_hand_end dispatches). This is a deliberate correctness trade — the
//! shared-hero path in the ladder and in play depend on those hooks — and is
//! amortized in any real match. Re-measure before treating P6 as a regression.

use cham_core::obs::Agent;
use cham_eval::matcheng::{MatchRunner, MatchSpec};
use cham_opponents::factory::OpponentSpecDto;
use criterion::{Criterion, black_box, criterion_group, criterion_main};

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

criterion_group!(
    name = benches;
    // B10.4: ≥ 10 s measurement for the P6 gate bench.
    config = Criterion::default().measurement_time(std::time::Duration::from_secs(10));
    targets = bench_match
);
criterion_main!(benches);
