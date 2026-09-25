//! B5 evidence bench: the content-keyed subgame cache on the real trigger
//! pattern — a stream of river spots whose (board class, SPR band) keys come
//! from a small recurring set. After the first occurrence every spot is a
//! hash lookup, which is where the plan's ">=1.3x on second iteration" lives;
//! `solve_rnr_400` alone cannot see it because CFR+ iteration cost dominates
//! the one-time build by two orders of magnitude.

use cham_search::cache::cached_build;
use cham_search::prior::collapse_to_classes;
use cham_search::subgame::{Class, Subgame};
use criterion::{Criterion, black_box, criterion_group, criterion_main};

/// Distinct spot keys in the stream — small on purpose (river classes recur).
const K: usize = 16;
/// Spots drawn per bench iteration.
const N: usize = 256;

fn spot(i: usize) -> (Vec<Class>, Vec<Class>, f64, f64, [f64; 2]) {
    let h: Vec<(f64, f64)> = (0..9)
        .map(|j| (1.0 / 9.0, (((i * 9 + j) % 17) as f64) / 16.0))
        .collect();
    let v: Vec<(f64, f64)> = (0..9)
        .map(|j| (1.0 / 9.0, (((i * 7 + j) % 13) as f64) / 12.0))
        .collect();
    let hero = collapse_to_classes(h, 3);
    let villain = collapse_to_classes(v, 3);
    let pot = 12.0 + (i % 4) as f64;
    (hero, villain, pot, 92.0, [0.5, 1.25])
}

fn bench_trigger_stream(c: &mut Criterion) {
    // Warm the cache first so the hit-line measures steady-state.
    for i in 0..K {
        let (h, v, pot, stack, fr) = spot(i);
        let _ = cached_build(h, v, pot, stack, &fr, 0xBE4C).expect("warm");
    }
    c.bench_function("trigger_stream_cache_hit", |b| {
        b.iter(|| {
            let mut acc = 0usize;
            for s in 0..N {
                let (h, v, pot, stack, fr) = spot(s % K);
                let sg = cached_build(
                    black_box(h),
                    black_box(v),
                    black_box(pot),
                    black_box(stack),
                    black_box(&fr),
                    black_box(0xBE4C),
                )
                .expect("hit");
                acc += sg.bet_fracs.len();
            }
            acc
        })
    });
    c.bench_function("trigger_stream_fresh_build", |b| {
        b.iter(|| {
            let mut acc = 0usize;
            for s in 0..N {
                let (h, v, pot, stack, fr) = spot(s % K);
                let sg = Subgame::build(
                    black_box(h),
                    black_box(v),
                    black_box(pot),
                    black_box(stack),
                    black_box(&fr),
                )
                .expect("fresh");
                acc += sg.bet_fracs.len();
            }
            acc
        })
    });
}

criterion_group!(benches, bench_trigger_stream);
criterion_main!(benches);
