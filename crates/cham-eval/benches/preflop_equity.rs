//! EXP-019: cold-HashMap (first seating) vs warm-HashMap cost per preflop
//! all-in adjustment. Answers whether the offline GPU table (§2.1 step 3) is
//! still worth building or memoization is sufficient as shipped.

use criterion::{Criterion, black_box, criterion_group, criterion_main};

fn bench_preflop_cold_vs_warm(c: &mut Criterion) {
    use cham_core::card::{Card, Hand2};
    let hero = Hand2::new(Card::parse("As").unwrap(), Card::parse("Ks").unwrap());
    let villain = Hand2::new(Card::parse("Qh").unwrap(), Card::parse("Qd").unwrap());
    c.bench_function("preflop_equity_cold", |b| {
        b.iter(|| {
            cham_eval::vr::preflop_memo_clear_for_tests();
            cham_eval::vr::preflop_equity(black_box(hero), black_box(villain))
        })
    });
    // Warm: populate once, then measure lookup.
    cham_eval::vr::preflop_memo_clear_for_tests();
    let _ = cham_eval::vr::preflop_equity(hero, villain);
    c.bench_function("preflop_equity_warm", |b| {
        b.iter(|| cham_eval::vr::preflop_equity(black_box(hero), black_box(villain)))
    });
}

criterion_group!(name = benches; config = Criterion::default(); targets = bench_preflop_cold_vs_warm);
criterion_main!(benches);
