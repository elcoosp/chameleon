//! 2026-10-02: compare the aggressive-slot inventory of the tiny ladder vs
//! the "rich" ladder. The `SIZE-BUCKET-DEGENERACY` finding says the tiny
//! ladder has ~1 normal size per street, so the infoset key cannot encode
//! size. This test asks whether `abstraction-tiny-rich.toml` actually has
//! more sizes — i.e. whether "use the rich ladder" is a real lever.

use cham_core::card::Deck;
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::obs::{Observables, Player};
use cham_core::rng::rng_from_seed;
use cham_engine::config::{AbstractionConfig, parse_config};
use cham_engine::encoder::ActionSeq;
use cham_engine::ladder::ActionLadder;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

/// For one abstraction, walk hands and collect the SET of distinct normal
/// aggressive sizes (in chips-to) seen per street, plus the jam slot.
fn inventory(
    cfg: &AbstractionConfig,
    seeds: u64,
) -> Vec<(u8, std::collections::BTreeSet<i64>, u32)> {
    let ladder = ActionLadder::new(cfg);
    // per-street: set of normal sizes, count of jam slots seen
    let mut per_street: Vec<(std::collections::BTreeSet<i64>, u32)> =
        vec![(Default::default(), 0); 4];
    for seed in 0..seeds {
        let mut st = State::new(CFG, Deck::shuffled(&mut rng_from_seed(seed))).expect("s");
        let mut steps = 0;
        while !st.is_terminal() && steps < 50 {
            steps += 1;
            let p = st.to_act();
            let obs = Observables::view(&st, Player::from_usize(p));
            let seq = ActionSeq::default();
            let slots = ladder.slots(&obs, &seq);
            let s = obs.street.as_u8() as usize;
            for slot in slots.iter() {
                match slot.action {
                    Action::Bet { to } | Action::Raise { to } => {
                        if slot.frac.is_infinite() {
                            per_street[s].1 += 1;
                        } else {
                            per_street[s].0.insert(to);
                        }
                    }
                    _ => {}
                }
            }
            let a = if cham_core::obs::is_legal(&obs, Action::Call) {
                Action::Call
            } else {
                Action::Check
            };
            if st.apply(a).is_err() {
                break;
            }
        }
    }
    per_street
        .into_iter()
        .enumerate()
        .map(|(s, (sizes, jams))| (s as u8, sizes, jams))
        .collect()
}

fn report(name: &str, inv: &[(u8, std::collections::BTreeSet<i64>, u32)]) -> usize {
    eprintln!("\n=== {name}: distinct normal aggressive sizes per street ===");
    let mut total = 0usize;
    for (s, sizes, jams) in inv {
        let street = ["pre", "flop", "turn", "river"]
            .get(*s as usize)
            .unwrap_or(&"?");
        total += sizes.len();
        eprintln!(
            "  {street:>5}: {} distinct normal size(s) {:?}, jam seen {jams}x",
            sizes.len(),
            sizes.iter().collect::<Vec<_>>()
        );
    }
    eprintln!("  TOTAL distinct normal sizes across streets: {total}");
    total
}

#[test]
fn rich_ladder_has_more_sizes_than_tiny() {
    let tiny = AbstractionConfig::tiny();
    let rich = parse_config(include_str!("../../../config/abstraction-tiny-rich.toml"))
        .expect("parse rich config");

    let tiny_inv = inventory(&tiny, 300);
    let rich_inv = inventory(&rich, 300);

    let tiny_total = report("TINY", &tiny_inv);
    let rich_total = report("RICH", &rich_inv);

    // The claim under test: the rich ladder offers more distinct normal
    // sizes. If this fails, "use the rich ladder for size resolution" is
    // wrong and the F6c direction needs rethinking.
    assert!(
        rich_total > tiny_total,
        "rich ladder should have MORE distinct normal sizes than tiny: \
         rich={rich_total}, tiny={tiny_total}"
    );
}

/// The decisive follow-up: does the stack-fraction bucket DISTINGUISH the
/// rich ladder's extra sizes, or do they all collapse to one bucket? If
/// they collapse, the rich ladder alone is not enough — the slot bucket
/// (`CHAM_SLOT_BUCKET`) is also required.
#[test]
fn rich_sizes_collapse_under_stack_fraction_bucket() {
    let rich = parse_config(include_str!("../../../config/abstraction-tiny-rich.toml"))
        .expect("parse rich config");
    let ladder = ActionLadder::new(&rich);

    // Reach a flop node.
    let mut st = State::new(CFG, Deck::shuffled(&mut rng_from_seed(7))).expect("s");
    st.apply(Action::Call).expect("call");
    st.apply(Action::Check).expect("check");
    let p = Player::from_usize(st.to_act());
    let obs = Observables::view(&st, p);
    let seq = ActionSeq::default();
    let slots = ladder.slots(&obs, &seq);

    let mut seen = Vec::new();
    for s in slots.iter() {
        if let Action::Bet { to } | Action::Raise { to } = s.action {
            let mut probe = ActionSeq::default();
            cham_engine::ladder::record_action(&ladder, &obs, p, s.action, &mut probe);
            let st_idx = obs.street.as_u8() as usize;
            let n = probe.lens[st_idx] as usize;
            let bucket = probe.entries[st_idx * 8 + (n - 1)].size_bucket;
            seen.push((to, bucket, s.frac.is_infinite()));
        }
    }
    eprintln!("\nrich flop slots (to, stack_bucket, is_jam): {seen:?}");
    let normal_buckets: std::collections::BTreeSet<u8> = seen
        .iter()
        .filter(|(_, _, jam)| !jam)
        .map(|(_, b, _)| *b)
        .collect();
    eprintln!("distinct stack buckets among NORMAL rich flop sizes: {normal_buckets:?}");
    eprintln!(
        "  => rich ladder has {} normal flop sizes but {} distinct stack buckets",
        seen.iter().filter(|(_, _, j)| !j).count(),
        normal_buckets.len()
    );
}
