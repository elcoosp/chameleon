//! P3a/P3b gates (SPECS/00 §6): encode throughput. flop/turn (table path) ≥ 1M/s;
//! river (exact-equity path) ≥ 100k/s on the M1.

use cham_core::card::{Card, Deck};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State, Street};
use cham_core::obs::{Observables, Player};
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn setup() -> (Encoder, State, State) {
    let cfg = AbstractionConfig::tiny();
    let enc = Encoder::cfg_only(cfg).expect("enc");
    let card = |r: u8, s: u8| Card(r * 4 + s);
    let prefix: Vec<Card> = vec![card(12, 0), card(1, 1), card(11, 0), card(2, 2), card(8, 3), card(4, 1), card(9, 2)];
    let mut flop = State::new(EngineConfig::depth(100), Deck::with_prefix(&prefix)).expect("s");
    flop.apply(Action::Call).expect("ok");
    flop.apply(Action::Check).expect("ok");
    let mut river = flop;
    for _ in 0..2 {
        if river.street() != Street::River {
            let _ = river.apply(Action::Check);
            let _ = river.apply(Action::Check);
        }
    }
    (enc, flop, river)
}

fn bench_encode_flop_turn(c: &mut Criterion) {
    let (mut enc, flop, _) = setup();
    let obs = Observables::view(&flop, Player::Sb);
    let seq = ActionSeq::default();
    c.bench_function("encode_flop", |b| {
        b.iter(|| {
            let mut acc = 0u64;
            for _ in 0..100 {
                let k = enc.key(black_box(&obs), &seq);
                acc ^= k.0;
            }
            acc
        })
    });
}

fn bench_encode_river(c: &mut Criterion) {
    let (mut enc, _, river) = setup();
    let obs = Observables::view(&river, Player::Bb);
    let seq = ActionSeq::default();
    c.bench_function("encode_river", |b| {
        b.iter(|| {
            let mut acc = 0u64;
            for _ in 0..10 {
                let k = enc.key(black_box(&obs), &seq);
                acc ^= k.0;
            }
            acc
        })
    });
}

criterion_group!(benches, bench_encode_flop_turn, bench_encode_river);
criterion_main!(benches);

#[allow(dead_code)]
fn rng_touch() {
    let _ = rng_from_seed(1);
}
