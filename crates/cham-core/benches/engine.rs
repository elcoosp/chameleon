//! P2 gate (SPECS/00 §6): engine apply/step throughput ≥ 10M actions/s on the M1.

use cham_core::engine::config::EngineConfig;
use cham_core::engine::State;
use cham_core::obs::LegalAction;
use cham_core::rng::rng_from_seed;
use arrayvec::ArrayVec;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_apply(c: &mut Criterion) {
    let cfg = EngineConfig::depth(100);
    // Play many hands with always-first-legal actions; measures apply + legality.
    c.bench_function("engine_apply", |b| {
        b.iter(|| {
            let mut actions = 0u64;
            let mut seed = 1u64;
            while actions < 2000 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let rng = &mut rng_from_seed(seed);
                let mut s = State::new(cfg, cham_core::card::Deck::shuffled(rng)).expect("state");
                loop {
                    if s.is_terminal() {
                        break;
                    }
                    let mut legal: ArrayVec<LegalAction, 12> = ArrayVec::new();
                    s.legal_actions(&mut legal);
                    let a = legal[(actions % legal.len() as u64) as usize].action;
                    let _ = black_box(s.apply(a));
                    actions += 1;
                    if actions >= 2000 {
                        break;
                    }
                }
            }
            actions
        })
    });
}

criterion_group!(benches, bench_apply);
criterion_main!(benches);
