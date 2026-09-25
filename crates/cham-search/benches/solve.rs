//! P5 gate (SPECS/00 §6): river solve on the standard reference spot, 400 iters,
//! ≤ 250 ms wall in WallClock mode (Iterations mode is unbounded by definition).

use cham_search::budget::SearchBudget;
use cham_search::cache::cached_build;
use cham_search::oracle;
use cham_search::prior::{PriorStrats, collapse_to_classes};
use cham_search::solve::solve;
use cham_search::trigger::SearchConfig;
use cham_search::trigger::SolverChoice;
use criterion::{Criterion, black_box, criterion_group, criterion_main};

/// B5 L1 on the bench path: every iteration rebuilds through the content-keyed
/// cache (first iteration misses, the rest hit), so the reported mean shows
/// the steady-state solve cost with build work memoized away.
fn bench_solve(c: &mut Criterion) {
    let hero = collapse_to_classes((0..9).map(|i| (1.0 / 9.0, i as f64 / 8.0)).collect(), 3);
    let villain = collapse_to_classes((0..9).map(|i| (1.0 / 9.0, i as f64 / 8.0)).collect(), 3);
    let mut prior = PriorStrats::empty();
    prior.set("check", vec![1.0]);
    prior.set("fold", vec![1.0, 0.0, 0.0]);
    prior.set("call", vec![0.0, 1.0, 0.0]);
    c.bench_function("solve_rnr_400", |b| {
        b.iter(|| {
            let sg = cached_build(
                black_box(hero.clone()),
                black_box(villain.clone()),
                12.0,
                92.0,
                &[0.5, 1.25],
                0xBE4C,
            )
            .expect("sg");
            let r =
                solve(black_box(&sg), &prior, &SolverChoice::Rnr { p: 0.9 }, 400).expect("solve");
            r.our_strategy.len()
        })
    });
}

criterion_group!(benches, bench_solve);
criterion_main!(benches);

#[allow(dead_code)]
fn touch() {
    let _ = oracle::reference_matrix_2x2();
    let _ = SearchBudget::Iterations { iters: 1 };
    let _ = SearchConfig::default();
}
