//! P1 gate (SPECS/00 §6): 7-card evaluator throughput. Threshold ≥ 100M evals/s
//! on the M1 release build. `just bench` reports; `verify --perf` shells this.

use cham_core::card::Card;
use cham_core::eval::{evaluate7, evaluate7_batch};
use cham_core::rng::next_u32;
use cham_core::rng::rng_from_seed;
use criterion::{Criterion, black_box, criterion_group, criterion_main};

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
    // Batch API: 8 hands per call × 1000 iterations (8000 evals per sample).
    // Batches are precomputed outside the timed loop so only evaluation is
    // measured (mirrors real batch callers that already hold SoA buffers).
    let batches: Vec<[[Card; 7]; 8]> = (0..32)
        .map(|b| {
            let mut batch = [hands[0]; 8];
            for k in 0..8 {
                batch[k] = hands[(b * 8 + k) & 255];
            }
            batch
        })
        .collect();
    let mut j = 0usize;
    c.bench_function("eval_evaluate7_batch8", |b| {
        b.iter(|| {
            let mut acc = 0u64;
            let mut out = [0u16; 8];
            for _ in 0..1000 {
                j = (j + 1) & 31;
                evaluate7_batch(black_box(&batches[j]), &mut out);
                black_box(&out);
                acc += out[0] as u64;
            }
            acc
        })
    });
}

fn bench_evaluate7_dense(c: &mut Criterion) {
    use cham_core::eval::evaluate7_dense;
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
    c.bench_function("eval_evaluate7_dense", |b| {
        b.iter(|| {
            let mut acc = 0u64;
            for _ in 0..1000 {
                i = (i + 1) & 255;
                acc += evaluate7_dense(black_box(&hands[i])) as u64;
            }
            acc
        })
    });
}

criterion_group!(benches, bench_evaluate7);
criterion_group!(benches_dense, bench_evaluate7_dense);
criterion_main!(benches, benches_dense);
