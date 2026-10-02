//! F6c / E6 follow-up (2026-10-02): quantify how often the stack-fraction
//! `size_bucket` saturates at a value that makes off-tree translation a
//! no-op. The E6 measurement found translation is a no-op at 100bb /
//! small-pot states; this samples real hands across ALL streets.

use cham_core::card::Deck;
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::obs::{Observables, Player};
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::ActionSeq;
use cham_engine::ladder::{ActionLadder, record_action};

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

fn bucket_of(ladder: &ActionLadder, obs: &Observables<'_>, p: Player, a: Action) -> u8 {
    // ActionSeq::push writes to `entries[street*8 + n]`, so the entry we
    // want is the LAST one written for this street — not entries[0].
    let mut fresh = ActionSeq::default();
    record_action(ladder, obs, p, a, &mut fresh);
    let s = obs.street.as_u8() as usize;
    let n = fresh.lens[s] as usize;
    if n == 0 {
        return 255; // sentinel: not recorded (window full)
    }
    fresh.entries[s * 8 + (n - 1)].size_bucket
}

/// Walk one hand to a terminal. At aggressive opportunities, take the
/// first non-jam aggressive slot on EVEN steps and call/check on ODD
/// steps, so the hand actually progresses through streets instead of
/// raise-warring to the step cap.
fn sample_buckets(seed: u64, hist: &mut [u64; 16], by_street: &mut [[u64; 2]; 4]) {
    let ladder = ActionLadder::new(&AbstractionConfig::tiny());
    let mut st = State::new(CFG, Deck::shuffled(&mut rng_from_seed(seed))).expect("state");
    let mut steps = 0u32;
    while !st.is_terminal() && steps < 60 {
        steps += 1;
        let p = st.to_act();
        let obs = Observables::view(&st, Player::from_usize(p));
        let aggressive = steps % 2 == 0;
        let mut chosen = None;
        if aggressive {
            for l in &obs.legal {
                if matches!(l.action, Action::Bet { .. } | Action::Raise { .. }) && !l.is_all_in {
                    chosen = Some(l.action);
                    break;
                }
            }
        }
        let a = chosen.unwrap_or_else(|| {
            if cham_core::obs::is_legal(&obs, Action::Call) {
                Action::Call
            } else {
                Action::Check
            }
        });
        if matches!(a, Action::Bet { .. } | Action::Raise { .. }) {
            let bucket = bucket_of(&ladder, &obs, Player::from_usize(p), a) as usize;
            if bucket < 16 {
                hist[bucket] += 1;
                let s = obs.street.as_u8() as usize;
                by_street[s.min(3)][0] += 1;
                if bucket <= 1 {
                    by_street[s.min(3)][1] += 1;
                }
            }
        }
        if st.apply(a).is_err() {
            break;
        }
    }
}

#[test]
fn size_bucket_distribution() {
    let mut hist = [0u64; 16];
    let mut by_street = [[0u64; 2]; 4];
    for seed in 0..2000u64 {
        sample_buckets(seed, &mut hist, &mut by_street);
    }
    let total: u64 = hist.iter().sum();
    eprintln!("\n=== size_bucket distribution over {total} aggressive actions ===");
    for (b, n) in hist.iter().enumerate() {
        if *n > 0 {
            eprintln!(
                "  bucket {b:>2}: {n:>7} ({:.1}%)",
                100.0 * *n as f64 / total as f64
            );
        }
    }
    eprintln!("\n=== by street (aggressive actions / bucket<=1) ===");
    for (s, name) in ["pre", "flop", "turn", "river"].iter().enumerate() {
        let (n, z) = (by_street[s][0], by_street[s][1]);
        if n > 0 {
            eprintln!(
                "  {name:>5}: {n:>6} actions, {z:>6} bucket<=1 ({:.1}%)",
                100.0 * z as f64 / n as f64
            );
        }
    }
    assert!(total > 0, "no aggressive actions sampled");
}
