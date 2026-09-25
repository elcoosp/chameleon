//! P1 gate (SPECS/00 §6): 7-card evaluator throughput. Threshold ≥ 100M evals/s
//! on the M1 release build. `just bench` reports; `verify --perf` shells this.

use cham_core::card::Card;
use cham_core::eval::evaluate7;
use cham_core::rng::next_u32;
use cham_core::rng::rng_from_seed;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_evaluate7(c: &mut Criterion) {
    let mut rng = rng_from_seed(0xBADC0DE);
    let hands: Vec<[Card; 7]> = (0..256)
        .map(|_| {
            let mut used = [false; 52];
            let mut out = [Card(0); 7];
            for slot in out.iter_mut() {
                loop {
                    let x = (next_u32(&mut rng) % 52) as u8;
                    if !used[x as usize] {
                        used[x as usize] = true;
                        *slot = Card(x);
                        break;
                    }
                }
            }
            out
        })
        .collect();
    let mut i = 0usize;
    c.bench_function("eval_evaluate7", |b| {
        b.iter(|| {
            let mut acc = 0u64;
            for _ in 0..1000 {
                i = (i + 1) & 255;
                acc += evaluate7(black_box(&hands[i])) as u64;
            }
            acc
        })
    });
}

criterion_group!(benches, bench_evaluate7);
criterion_main!(benches);
