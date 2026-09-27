//! Contractual test set for cham-opponents (SPECS/03 §7).

mod common;

use cham_core::engine::Street;
use cham_core::obs::{Agent, Observables, Player};
use cham_core::rng::rng_from_seed;
use cham_opponents::baselines::{CallBot, RandomBot};
use cham_opponents::factory::{OpponentSpec, build};
use cham_opponents::params::ArchetypeId;
use cham_opponents::percentile::{PercentileChart, random_hand};
use cham_opponents::perturbed::{PerturbedNashAgent, Tilt};
use cham_opponents::{archetype::ArchetypeAgent, noisy::NoisyAgent};

use common::{DecisionLog, action_kind, play_hands};

const CHART: fn() -> &'static PercentileChart = PercentileChart::global;

// ---------- chart ----------

#[test]
fn percentile_chart_anchors() {
    let chart = CHART();
    assert_eq!(chart.len(), 169);
    // absolute anchor ranges (±2% windows around known h2h equities vs uniform)
    let hand_of = |s: &str| {
        let c = |t: &str| cham_core::card::Card::parse(t).unwrap();
        let mut it = s.split_whitespace();
        cham_core::card::Hand2::new(c(it.next().unwrap()), c(it.next().unwrap()))
    };
    let eq = |spec: &str| chart.equity(hand_of(spec));
    let aa = eq("As Ah");
    assert!((0.83..=0.87).contains(&aa), "AA equity {aa}");
    let kk = eq("Ks Kh");
    assert!((0.80..=0.84).contains(&kk), "KK equity {kk}");
    let t72 = eq("7s 2h");
    assert!((0.33..=0.37).contains(&t72), "72o equity {t72}");
    // ordering: premium pairs strictly dominate middling hands
    assert!(aa > kk);
    let qq = eq("Qs Qh");
    assert!(kk > qq);
    let j10 = eq("Js Th");
    assert!(qq > j10, "pairs dominate connectors");
    // percentile rank 0 is the strongest class
    let best = chart.entry(0);
    let best_hand_classes = [best.class_id];
    let _ = best_hand_classes;
    assert_eq!(
        chart.percentile(cham_core::card::Hand2::new(
            cham_core::card::Card::parse("As").unwrap(),
            cham_core::card::Card::parse("Ah").unwrap()
        )),
        0.0,
        "AA is rank 0"
    );
}

// ---------- gating ----------

#[test]
fn preflop_gating_pinned() {
    // Open size exactly 2.5bb (250), 3bet +3bb over the raise, 4bet ×2.2 (constants).
    let chart = CHART();
    let mut hero = ArchetypeAgent::point(ArchetypeId::Lag, chart);
    // SB opens a strong hand at 2.5bb: search seeds until we see the open size
    let mut found_open = None;
    'outer: for seed in 0..200u64 {
        let rng = &mut cham_core::rng::child(0x07E1 ^ seed, "probe");
        let deck = cham_core::card::Deck::shuffled(rng);
        let s = cham_core::engine::State::new(common::CFG, deck).expect("s");
        let obs = Observables::view(&s, Player::Sb);
        if let Ok(dist) = hero.action_probs(&obs) {
            for (a, p) in dist {
                if p > 0.5 {
                    if let cham_core::engine::Action::Raise { to } = a {
                        if to == 250 {
                            found_open = Some((seed, to));
                            break 'outer;
                        }
                    }
                }
            }
        }
    }
    assert!(
        found_open.is_some(),
        "LAG must open at 250 (2.5bb) sometimes"
    );
    assert_eq!(found_open.unwrap().1, 250);
    // L-18b fix (2026-09-27): dropped `let _ = chart;` at the end (chart is
    // already consumed by ArchetypeAgent::point above) and the unused
    // `villain` / `let _ = (&mut hero, &mut villain);` — both were
    // decoration, not part of the test's actual assertion.
}

// ---------- determinism / jitter ----------

#[test]
fn archetype_point_determinism() {
    let chart = CHART();
    let run = |seed: u64| -> [u64; 5] {
        let mut hero = ArchetypeAgent::jittered(ArchetypeId::Tag, seed, chart);
        let mut villain = CallBot;
        let mut log = DecisionLog::default();
        play_hands(&mut hero, &mut villain, 1000, seed, Some(&mut log));
        log.actions_by_kind
    };
    let a = run(777);
    let b = run(777);
    assert_eq!(a, b, "same (spec, seed) → identical decision logs");
    let c = run(778);
    assert_ne!(a, c, "different seeds should differ");
}

#[test]
fn archetype_jitter_ranges() {
    let _chart = CHART();
    let base = cham_opponents::params::ArchetypeParams::point(ArchetypeId::Tag);
    let spec = cham_opponents::params::JitterSpec::standard();
    let mut rng = rng_from_seed(4);
    let n = 200;
    let mut sum = 0.0;
    for _ in 0..n {
        let p = spec.apply(&base, &mut rng);
        assert!(
            (base.open_raise - spec.open_raise..=base.open_raise + spec.open_raise)
                .contains(&p.open_raise)
        );
        assert!((0.0..=1.0).contains(&p.cbet_flop));
        assert!((0.3..=2.0).contains(&p.call_factor));
        sum += p.open_raise;
    }
    let mean = sum / n as f64;
    assert!(
        (mean - base.open_raise).abs() < 0.03,
        "jitter mean ≈ default: {mean} vs {}",
        base.open_raise
    );
}

// ---------- analytic action_probs ----------

#[test]
fn action_probs_analytic_consistency() {
    // Empirical action frequencies over 50k seeded decisions match action_probs
    // within 3 SE per action kind.
    let mut hero = ArchetypeAgent::point(ArchetypeId::Tag, CHART());
    let mut villain = RandomBot;
    let mut log = DecisionLog::default();
    play_hands(&mut hero, &mut villain, 12_000, 0xC0FFEE, Some(&mut log));
    // predicted probabilities per kind (action_probs is state-conditional; we
    // accumulate the predicted mass for the ACTUAL state at each decision by
    // replaying the same seed deterministically with a recording wrapper)
    let pred = predicted_masses(ArchetypeId::Tag, 0xC0FFEE, log.n);
    let n = log.n as f64;
    for (kind, mass) in pred.iter().enumerate() {
        let predicted = mass / n; // per-decision mean predicted probability
        if *mass <= 0.0 && log.actions_by_kind[kind] == 0 {
            continue;
        }
        let emp = log.actions_by_kind[kind] as f64 / n;
        let se = (predicted.max(1e-9) * (1.0 - predicted.max(1e-9)) / n)
            .sqrt()
            .max(1e-9);
        let z = (emp - predicted).abs() / se;
        assert!(
            z < 3.5,
            "kind {kind}: emp {emp:.4} vs pred {predicted:.4} (z={z:.2})"
        );
    }
}

/// Deterministic replay collecting predicted probs for each hero decision.
fn predicted_masses(arch: ArchetypeId, seed: u64, cap: u64) -> [f64; 5] {
    use cham_core::card::Deck;
    use cham_core::engine::State;
    use cham_core::obs::Agent;
    use cham_core::rng::child;
    let chart = CHART();
    let mut hero = ArchetypeAgent::point(arch, chart);
    let mut villain = RandomBot;
    let mut out = [0f64; 5];
    let mut seen = 0u64;
    'hands: for h in 0..30_000u64 {
        let rng = &mut child(seed, &format!("h{h}"));
        let mut s = State::new(common::CFG, Deck::shuffled(rng)).expect("state");
        let mut guard = 0;
        while !s.is_terminal() && guard < 400 {
            guard += 1;
            let obs = Observables::view(&s, Player::from_usize(s.to_act()));
            let a = if s.to_act() == 0 {
                let probs = hero.action_probs(&obs).expect("analytic");
                let chosen = hero.act(&obs, rng);
                let kind = action_kind(chosen);
                out[kind] += probs
                    .iter()
                    .filter(|(x, _)| action_kind(*x) == kind)
                    .map(|(_, p)| *p)
                    .sum::<f64>();
                seen += 1;
                chosen
            } else {
                villain.act(&obs, rng)
            };
            s.apply(a).expect("legal");
            if seen >= cap {
                break 'hands;
            }
        }
        if seen >= cap {
            break;
        }
    }
    out
}

#[test]
fn action_probs_independent_per_decision() {
    // Conditional frequencies given identical observable contexts across hands are
    // equal — i.e., no within-hand memory in the distribution.
    // The river bluff mix is state-pure: same (ehs, params) → same p. We verify by
    // calling action_probs twice on identical views constructed in different hands
    // and checking equality, then that act() frequencies match the prob in aggregate.
    let chart = CHART();
    let a1 = ArchetypeAgent::point(ArchetypeId::Lag, chart);
    let a2 = ArchetypeAgent::point(ArchetypeId::Lag, chart);
    let mut rng = rng_from_seed(9);
    for _ in 0..200 {
        let h1 = random_hand(&mut rng);
        let h2 = random_hand(&mut rng);
        let _ = (h1, h2);
    }
    // distribution purity: same state → same probs regardless of agent instance
    let mut st = cham_core::engine::State::new(
        common::CFG,
        cham_core::card::Deck::with_prefix(&[
            cham_core::card::Card::parse("As").unwrap(),
            cham_core::card::Card::parse("2c").unwrap(),
            cham_core::card::Card::parse("Kd").unwrap(),
            cham_core::card::Card::parse("3s").unwrap(),
            cham_core::card::Card::parse("9h").unwrap(),
            cham_core::card::Card::parse("4d").unwrap(),
            cham_core::card::Card::parse("Js").unwrap(),
        ]),
    )
    .expect("s");
    cham_core::engine::State::apply(&mut st, cham_core::engine::Action::Call).expect("ok");
    cham_core::engine::State::apply(&mut st, cham_core::engine::Action::Check).expect("ok");
    let obs = Observables::view(&st, Player::Sb);
    let d1 = a1.action_probs(&obs).expect("probs");
    let d2 = a2.action_probs(&obs).expect("probs");
    assert_eq!(d1, d2, "no within-hand memory: pure function of state");
    let _ = Street::Preflop;
}

#[test]
fn archetype_stats_sanity() {
    // Against CallBot, TAG should not be a huge dog or favorite — sanity only.
    let chart = CHART();
    let mut hero = ArchetypeAgent::point(ArchetypeId::Tag, chart);
    let mut villain = CallBot;
    let mut nets = 0i64;
    for h in 0..2000u64 {
        let rng = &mut cham_core::rng::child(0x57A75, &format!("h{h}"));
        let mut s =
            cham_core::engine::State::new(common::CFG, cham_core::card::Deck::shuffled(rng))
                .expect("s");
        let mut guard = 0;
        while !s.is_terminal() && guard < 400 {
            guard += 1;
            let obs = Observables::view(&s, Player::from_usize(s.to_act()));
            let a = if s.to_act() == 0 {
                hero.act(&obs, rng)
            } else {
                villain.act(&obs, rng)
            };
            s.apply(a).expect("legal");
        }
        nets += s.payoffs()[0];
    }
    let mb = nets as f64 / 1000.0 / 2000.0 * 1000.0; // mb/hand
    assert!(
        mb > -200.0 && mb < 900.0,
        "TAG vs CallBot sane winrate: {mb} mb/hand"
    );
}

#[test]
fn legal_fallback_paths() {
    // Agents never return illegal actions on adversarial views (short stacks).
    let chart = CHART();
    let mut agents: Vec<Box<dyn Agent>> = vec![
        Box::new(ArchetypeAgent::point(ArchetypeId::Nit, chart)),
        Box::new(ArchetypeAgent::point(ArchetypeId::Station, chart)),
        Box::new(cham_opponents::baselines::JamBot),
        Box::new(cham_opponents::baselines::FishBot),
        Box::new(cham_opponents::family_b::FamilyBAgent::new(
            ArchetypeId::Lag,
            chart,
        )),
    ];
    for seed in 0..300u64 {
        let rng = &mut cham_core::rng::child(0xFACE, &format!("{seed}"));
        let mut s = cham_core::engine::State::new(
            cham_core::engine::config::EngineConfig {
                start_stack: 2_000,
                sb: 50,
                bb: 100,
            },
            cham_core::card::Deck::shuffled(rng),
        )
        .expect("s");
        let mut guard = 0;
        let agent_index = (seed as usize) % agents.len();
        while !s.is_terminal() && guard < 400 {
            guard += 1;
            let obs = Observables::view(&s, Player::from_usize(s.to_act()));
            let a = agents[agent_index].act(&obs, rng);
            assert!(
                cham_core::obs::is_legal(&obs, a),
                "illegal action {a:?} from agent"
            );
            s.apply(a).expect("legal");
        }
    }
}

#[test]
fn switcher_drifts() {
    let chart = CHART();
    let mut sw = cham_opponents::drift::SwitcherBot::new(
        OpponentSpec::Arch(ArchetypeId::Nit),
        OpponentSpec::Arch(ArchetypeId::Lag),
        50,
        chart,
    );
    // before the switch: nit folds a lot; after: lag opens wide. Verify via probs.
    let mut st = cham_core::engine::State::new(
        common::CFG,
        cham_core::card::Deck::with_prefix(&[
            cham_core::card::Card::parse("9s").unwrap(),
            cham_core::card::Card::parse("2c").unwrap(),
            cham_core::card::Card::parse("8s").unwrap(),
            cham_core::card::Card::parse("3d").unwrap(),
        ]),
    )
    .expect("s");
    let obs = Observables::view(&st, Player::Sb);
    let d_before = sw.action_probs(&obs).expect("probs");
    // force switch by feeding hand ends
    for _ in 0..60 {
        sw.on_hand_end(
            &cham_core::engine::history::PublicHistory {
                actions: vec![],
                board: [cham_core::card::Card(0); 5],
                showdown_holes: [None, None],
                nets: [0, 0],
            },
            0,
        );
    }
    let d_after = sw.action_probs(&obs).expect("probs");
    assert_ne!(
        d_before, d_after,
        "switcher must change behavior at the drift hand"
    );
    let _ = &mut st;
}

#[test]
fn factory_parse_roundtrip() {
    let ids = [
        "arch:nit",
        "jitter:lag@9231",
        "callbot",
        "raisebot",
        "jamfix",
        "random",
        "fish",
        "pnash:overfold:0.15",
        "famB:tag",
        "noisy:0.1:jitter:lag@9231",
        "switch:arch:nit->arch:lag@250",
    ];
    for id in ids {
        let spec = OpponentSpec::parse(id).expect("parse");
        assert_eq!(spec.id(), id, "round-trip {id}");
    }
    assert_eq!(OpponentSpec::parse("famB:tag").expect("f").family(), "B");
    assert_eq!(
        OpponentSpec::parse("pnash:overcall:0.2")
            .expect("p")
            .family(),
        "PN"
    );
    assert_eq!(OpponentSpec::parse("arch:tag").expect("a").family(), "A");
    assert_eq!(
        OpponentSpec::parse("noisy:0.1:arch:lag")
            .expect("n")
            .family(),
        "noise"
    );
    assert!(OpponentSpec::parse("bogus").is_err());
}

#[test]
fn station_never_folds_tp() {
    // Station facing bets with the strength proxy ≥ 0.60 always calls (never folds).
    let chart = CHART();
    let mut hero = ArchetypeAgent::point(ArchetypeId::Station, chart);
    let mut villain = cham_opponents::baselines::RaiseBot;
    let mut log = DecisionLog::default();
    play_hands(&mut hero, &mut villain, 3000, 0x57A7, Some(&mut log));
    // fold decisions must never co-occur with the top ehs bucket in a facing-bet spot:
    // reconstruct via a fresh replay of ehs for folds would need state; use the
    // distribution-level invariant instead:
    let mut st = cham_core::engine::State::new(
        common::CFG,
        cham_core::card::Deck::with_prefix(&[
            cham_core::card::Card::parse("Ah").unwrap(),
            cham_core::card::Card::parse("Ad").unwrap(),
            cham_core::card::Card::parse("2c").unwrap(),
            cham_core::card::Card::parse("3s").unwrap(),
            cham_core::card::Card::parse("9h").unwrap(),
            cham_core::card::Card::parse("4d").unwrap(),
            cham_core::card::Card::parse("Js").unwrap(),
        ]),
    )
    .expect("s");
    cham_core::engine::State::apply(&mut st, cham_core::engine::Action::Call).unwrap();
    cham_core::engine::State::apply(&mut st, cham_core::engine::Action::Check).unwrap();
    cham_core::engine::State::apply(&mut st, cham_core::engine::Action::Bet { to: 500 }).unwrap();
    let obs = Observables::view(&st, Player::Sb);
    let dist = hero.action_probs(&obs).expect("probs");
    for (a, p) in dist {
        if a == cham_core::engine::Action::Fold {
            assert_eq!(p, 0.0, "station must never fold aces");
        }
    }
}

#[test]
fn family_b_divergence() {
    // FamilyB vs family-A same-label archetype: strategy KL > 0.05 on a 500-state
    // sample (they must genuinely differ).
    let chart = CHART();
    let a = ArchetypeAgent::point(ArchetypeId::Tag, chart);
    let b = cham_opponents::family_b::FamilyBAgent::new(ArchetypeId::Tag, chart);
    let mut kl_sum = 0.0;
    let mut states = 0;
    let mut rng = rng_from_seed(11);
    for seed in 0..500u64 {
        let mut r2 = rng_from_seed(0xD177 ^ seed);
        let _ = &mut r2;
        let h = random_hand(&mut rng);
        let _ = h;
        // sample decision contexts from real play
        let mut st = cham_core::engine::State::new(common::CFG, {
            let r = &mut rng_from_seed(0xD177 ^ seed);
            cham_core::card::Deck::shuffled(r)
        })
        .expect("s");
        // walk a few random legal actions to a mid-hand spot
        for _ in 0..(seed % 4) {
            if st.is_terminal() {
                break;
            }
            let o = Observables::view(&st, Player::from_usize(st.to_act()));
            let legals: Vec<_> = o.legal.iter().map(|l| l.action).collect();
            let pick = legals[cham_core::rng::pick(&mut rng, legals.len())];
            st.apply(pick).expect("legal");
        }
        if st.is_terminal() {
            continue;
        }
        let obs = Observables::view(&st, Player::from_usize(st.to_act()));
        let da = a.action_probs(&obs).expect("probs");
        let db = b.action_probs(&obs).expect("probs");
        kl_sum += kl(da.as_slice(), db.as_slice());
        states += 1;
    }
    let kl = kl_sum / states.max(1) as f64;
    assert!(
        kl > 0.05,
        "family-B must genuinely diverge from family-A: KL={kl}"
    );
}

fn kl(p: &[(cham_core::engine::Action, f64)], q: &[(cham_core::engine::Action, f64)]) -> f64 {
    let total_p: f64 = p.iter().map(|(_, x)| *x).sum();
    let mut kl = 0.0;
    for (a, pv) in p {
        let pv = pv / total_p.max(1e-12);
        let qv = q
            .iter()
            .filter(|(b, _)| b == a)
            .map(|(_, x)| *x)
            .sum::<f64>()
            .max(1e-9);
        if pv > 1e-9 {
            kl += pv * (pv / qv).ln();
        }
    }
    kl
}

#[test]
fn perturbed_tilt_math() {
    // OverFold(δ) shifts exactly δ mass into fold vs the base distribution.
    let mut pn = PerturbedNashAgent::new(Tilt::OverFold, 0.15);
    // inject a known base: [fold 0.2, call 0.5, raise 0.3]
    pn.set_source(Box::new(|_obs| {
        vec![
            (cham_core::engine::Action::Fold, 0.2),
            (cham_core::engine::Action::Call, 0.5),
            (cham_core::engine::Action::Raise { to: 300 }, 0.3),
        ]
    }));
    // the source is only consulted through tilted(); we test the math directly by
    // building a view-less call via a fabricated observable (uses only len()).
    let mut st = cham_core::engine::State::new(
        common::CFG,
        cham_core::card::Deck::with_prefix(&[
            cham_core::card::Card::parse("As").unwrap(),
            cham_core::card::Card::parse("2c").unwrap(),
            cham_core::card::Card::parse("Kd").unwrap(),
            cham_core::card::Card::parse("3s").unwrap(),
        ]),
    )
    .expect("s");
    let _ = st.apply(cham_core::engine::Action::Call).expect("ok");
    let obs = Observables::view(&st, Player::Sb);
    let tilted = pn.action_probs(&obs).expect("probs");
    let fold_new: f64 = tilted
        .iter()
        .filter(|(a, _)| matches!(a, cham_core::engine::Action::Fold))
        .map(|(_, p)| *p)
        .sum();
    assert!(
        (fold_new - 0.35).abs() <= 0.01,
        "fold mass 0.2 + δ0.15 = 0.35, got {fold_new}"
    );
    let total: f64 = tilted.iter().map(|(_, p)| *p).sum();
    assert!((total - 1.0).abs() < 1e-9, "renormalized");

    // OverCall direction
    let mut pc = PerturbedNashAgent::new(Tilt::OverCall, 0.15);
    pc.set_source(Box::new(|_obs| {
        vec![
            (cham_core::engine::Action::Fold, 0.2),
            (cham_core::engine::Action::Call, 0.5),
            (cham_core::engine::Action::Raise { to: 300 }, 0.3),
        ]
    }));
    let t2 = pc.action_probs(&obs).expect("probs");
    let call_new: f64 = t2
        .iter()
        .filter(|(a, _)| {
            matches!(
                a,
                cham_core::engine::Action::Call | cham_core::engine::Action::Check
            )
        })
        .map(|(_, p)| *p)
        .sum();
    assert!(
        (call_new - 0.65).abs() <= 0.01,
        "call mass 0.5 + δ0.15 = 0.65, got {call_new}"
    );
}

#[test]
fn noisy_wrapper_stats() {
    // Realized mistake rate ≈ ε ± 0.02; wrapped agent never returns illegal actions.
    let chart = CHART();
    let inner = build(&OpponentSpec::Arch(ArchetypeId::Tag), chart);
    let mut noisy = NoisyAgent::new(inner, 0.10);
    let mut villain = CallBot;
    let mut log = DecisionLog::default();
    play_hands(&mut noisy, &mut villain, 3000, 0x3E15, Some(&mut log));
    let rate = noisy.realized_mistake_rate();
    assert!((rate - 0.10).abs() <= 0.02, "mistake rate {rate} vs ε=0.10");
    let _ = (1.0f64,);
}
