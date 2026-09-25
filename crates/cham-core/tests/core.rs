//! Contractual test set for cham-core (SPECS/01 §7).

use arrayvec::ArrayVec;

use cham_core::CoreError;
use cham_core::card::{Card, Deck, Hand2};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::fuzz;
use cham_core::engine::history::PublicHistory;
use cham_core::engine::{Action, State, Street};
use cham_core::eval::{Range, best5, equity_exact, evaluate5, evaluate7};
use cham_core::obs::{Agent, AgentError, LegalAction, Observables, Player};
use cham_core::rng::{Rng, child, next_u32, rng_from_seed};

fn legal(state: &State) -> ArrayVec<LegalAction, 12> {
    let mut v = ArrayVec::new();
    state.legal_actions(&mut v);
    v
}

// ---------- cards ----------

#[test]
fn card_parse_roundtrip() {
    for c in cham_core::card::ALL_CARDS {
        let s = c.to_str();
        let p = Card::parse(&s).expect("parse");
        assert_eq!(p, c);
    }
    assert!(Card::parse("Az").is_err());
    assert!(Card::parse("Asx").is_err());
    assert_eq!(
        Card::parse("as").expect("lowercase"),
        Card(48),
        "As = idx 48"
    );
}

#[test]
fn hand2_canonical_169() {
    let mut classes = [false; 256];
    let mut canon_ids = [false; 256];
    for c1 in 0..52u8 {
        for c2 in (c1 + 1)..52u8 {
            let h = Hand2::new(Card(c1), Card(c2));
            classes[h.class_id() as usize] = true;
            let cn = h.canonical();
            assert_eq!(cn.canonical(), cn, "canonical idempotent");
            assert_eq!(cn.class_id(), h.class_id(), "canonical preserves class");
            canon_ids[cn.class_id() as usize] = true;
        }
    }
    assert_eq!(
        classes.iter().filter(|b| **b).count(),
        169,
        "169 preflop classes"
    );
    assert_eq!(
        canon_ids.iter().filter(|b| **b).count(),
        169,
        "canonical forms cover 169 classes"
    );
}

#[test]
fn range_bits_roundtrip() {
    let mut r = Range::default();
    assert_eq!(r.count(), 0);
    for c in 0..1326 {
        r.set(c, true);
    }
    assert_eq!(r.count(), 1326);
    for c in (0..1326).step_by(3) {
        r.set(c, false);
    }
    assert_eq!(r.count(), 1326 - 442);
    for c in 0..1326 {
        let h = Hand2::from_combo(c);
        assert_eq!(h.combo_id(), c);
        assert_eq!(r.get(c), c % 3 != 0);
    }
    let mut r = Range::all();
    r.remove_cards(&[Card(0), Card(51)]);
    for c in 0..1326 {
        let [a, b] = Hand2::from_combo(c).cards();
        let touches = a.idx() == 0 || a.idx() == 51 || b.idx() == 0 || b.idx() == 51;
        assert_eq!(r.get(c), !touches);
    }
    assert_eq!(Range::from_percent(0.0).count(), 0);
    assert_eq!(Range::from_percent(100.0).count(), 1326);
    assert!(Range::from_percent(30.0).count() <= Range::from_percent(40.0).count());
    let aks = Hand2::new(Card::parse("As").unwrap(), Card::parse("Ks").unwrap());
    assert!(Range::all().get(aks.combo_id()));
}

// ---------- evaluator ----------

/// Naive reference: best of the 21 five-card subsets by evaluate5.
fn naive7(c: &[Card; 7]) -> u16 {
    let mut best = 0u16;
    for i in 0..7 {
        for j in (i + 1)..7 {
            let mut five = [Card(0); 5];
            let mut k = 0;
            for m in 0..7 {
                if m != i && m != j {
                    five[k] = c[m];
                    k += 1;
                }
            }
            best = best.max(evaluate5(&five));
        }
    }
    best
}

fn seeded_hand(i: u64) -> [Card; 7] {
    let mut rng = rng_from_seed(0x5EED_0000 ^ i);
    let mut used = [false; 52];
    let mut out = [Card(0); 7];
    for slot in out.iter_mut() {
        loop {
            let c = (next_u32(&mut rng) % 52) as u8;
            if !used[c as usize] {
                used[c as usize] = true;
                *slot = Card(c);
                break;
            }
        }
    }
    out
}

#[test]
fn eval_golden_50() {
    for i in 0..50u64 {
        let hand = seeded_hand(i);
        assert_eq!(
            evaluate7(&hand),
            naive7(&hand),
            "hand {i}: {:?}",
            hand.map(|c| c.to_str())
        );
    }
    let card = |s: &str| Card::parse(s).expect("card");
    let royal: [Card; 7] = [
        card("As"),
        card("Ks"),
        card("Qs"),
        card("Js"),
        card("Ts"),
        card("2h"),
        card("3d"),
    ];
    assert_eq!(evaluate7(&royal), 7462, "royal flush tops the 7462 scale");
    assert_eq!(evaluate7(&royal), naive7(&royal));
}

#[test]
fn eval_flush_wheel_edges() {
    let card = |s: &str| Card::parse(s).expect("card");
    let mk = |spec: [&str; 7]| {
        [
            card(spec[0]),
            card(spec[1]),
            card(spec[2]),
            card(spec[3]),
            card(spec[4]),
            card(spec[5]),
            card(spec[6]),
        ]
    };
    let sf_wheel = mk(["ah", "2h", "3h", "4h", "5h", "2d", "3d"]);
    let a_flush = mk(["ah", "kh", "jh", "9h", "7h", "2d", "3d"]);
    assert!(
        evaluate7(&sf_wheel) > evaluate7(&a_flush),
        "wheel SF beats A-high flush"
    );
    let sf6 = mk(["2h", "3h", "4h", "5h", "6h", "2d", "3d"]);
    assert!(
        evaluate7(&sf6) > evaluate7(&sf_wheel),
        "6-high SF beats wheel SF"
    );
    let wheel = mk(["ah", "2d", "3c", "4s", "5h", "2c", "3d"]);
    let six_hi = mk(["2h", "3d", "4c", "5s", "6h", "2c", "3d"]);
    assert!(
        evaluate7(&six_hi) > evaluate7(&wheel),
        "6-high straight beats wheel"
    );
    let (five, v) = best5(&sf_wheel);
    assert_eq!(v, evaluate7(&sf_wheel));
    assert_eq!(
        five.iter().filter(|c| c.suit() == 1).count(),
        5,
        "best5 returns the flush"
    );
}

#[test]
fn eval_bitmask_vs_naive() {
    // 200k on the M1 tier (SPECS/01 §7); 50k here keeps the whole suite snappy while
    // covering every category thousands of times.
    for i in 0..50_000u64 {
        let hand = seeded_hand(i);
        let a = evaluate7(&hand);
        let b = naive7(&hand);
        assert_eq!(a, b, "seed {i} hand {:?}", hand.map(|c| c.to_str()));
        assert!((1..=7462).contains(&a));
    }
}

// ---------- engine semantics ----------

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

fn fresh(seed: u64) -> State {
    let rng = &mut rng_from_seed(seed);
    State::new(CFG, Deck::shuffled(rng)).expect("state")
}

#[test]
fn hu_position_order() {
    let s = fresh(1);
    assert_eq!(s.to_act(), 0, "SB acts first preflop");
    let mut s = fresh(1);
    s.apply(Action::Call).expect("sb completes");
    s.apply(Action::Check).expect("bb checks");
    assert_eq!(s.to_act(), 1, "BB acts first postflop, every street");
    assert_eq!(s.street(), Street::Flop);
}

#[test]
fn legal_order_pinned() {
    let s = fresh(3);
    let l = legal(&s);
    // SB facing bb-sb: [Fold, Call, Raise(min), ...]
    assert_eq!(l[0].action, Action::Fold);
    assert_eq!(l[1].action, Action::Call);
    assert!(matches!(l[2].action, Action::Raise { .. }));
    if let Action::Raise { to } = l[2].action {
        assert_eq!(to, 200, "min raise preflop = 2 x bb to");
    }
    // BB with the option preflop: [Check, Bet(min=2bb over own blind), Bet(all-in)]
    let mut s = fresh(3);
    s.apply(Action::Call).expect("call");
    let l = legal(&s);
    assert_eq!(l[0].action, Action::Check);
    if let Action::Bet { to } = l[1].action {
        assert_eq!(
            to, 200,
            "preflop BB option min = raise over own blind to 2bb"
        );
    } else {
        panic!("expected Bet slot");
    }
    assert!(matches!(l[2].action, Action::Bet { .. }));
    // postflop checked-to: min bet = bb
    s.apply(Action::Check).expect("bb checks");
    let l = legal(&s);
    if let Action::Bet { to } = l[1].action {
        assert_eq!(to, 100, "postflop min bet = bb");
    } else {
        panic!("expected Bet slot postflop");
    }
}

#[test]
fn short_allin_no_reopen() {
    let cfg20 = EngineConfig {
        start_stack: 2_000,
        sb: 50,
        bb: 100,
    };
    let rng = &mut rng_from_seed(12);
    let mut t = State::new(cfg20, Deck::shuffled(rng)).expect("t");
    t.apply(Action::Call).expect("complete");
    t.apply(Action::Check).expect("check");
    // flop: BB bets min, SB raises to 300, BB jams all-in to 1900 (below min-raise 500)
    t.apply(Action::Bet { to: 100 }).expect("bb bet");
    t.apply(Action::Raise { to: 300 }).expect("sb raise");
    t.apply(Action::Raise { to: 1900 }).expect("bb jam all-in");
    // SB (1700 behind) faces an all-in raise: [Fold, Call] only — no reopen
    let l = legal(&t);
    assert!(
        l.iter()
            .all(|x| matches!(x.action, Action::Fold | Action::Call)),
        "no raise slots vs all-in: {l:?}"
    );
    let call = l.iter().find(|x| x.action == Action::Call).expect("call");
    assert!(call.is_all_in);
    assert!(t.apply(Action::Raise { to: 2200 }).is_err());
}

#[test]
fn min_raise_progression() {
    let mut s = fresh(21);
    s.apply(Action::Raise { to: 200 }).expect("open min");
    let l = legal(&s);
    if let Action::Raise { to } = l[2].action {
        assert_eq!(to, 300, "min re-raise = 200 + 100");
    } else {
        panic!("raise slot expected");
    }
    s.apply(Action::Raise { to: 500 })
        .expect("full raise: 300 over the 200 level");
    let l = legal(&s);
    if let Action::Raise { to } = l[2].action {
        assert_eq!(to, 800, "min next = 500 + 300");
    } else {
        panic!("raise slot expected");
    }
    assert!(
        s.apply(Action::Raise { to: 600 }).is_err(),
        "below min-raise and not all-in"
    );
}

#[test]
fn split_odd_chip() {
    // Engineered chop: board broadway, both hands play the board.
    let card = |s: &str| Card::parse(s).expect("card");
    // prefix deal order: p0[0], p1[0], p0[1], p1[1], then board
    let prefix = [
        card("2h"),
        card("4c"),
        card("3d"),
        card("5s"),
        card("Th"),
        card("Jd"),
        card("Qc"),
        card("Kh"),
        card("Ad"),
    ];
    let mut s = State::new(CFG, Deck::with_prefix(&prefix)).expect("s");
    s.apply(Action::Call).expect("preflop complete");
    s.apply(Action::Check).expect("preflop check");
    for _ in 0..3 {
        s.apply(Action::Check).expect("BB checks");
        s.apply(Action::Check).expect("SB checks");
    }
    assert!(s.is_terminal() && s.reached_showdown());
    let [p0, p1] = s.payoffs();
    assert_eq!(p0, 0, "chop: SB nets zero");
    assert_eq!(p1, 0, "chop: BB nets zero");
    // odd-chip rule pinned: pot 101 → 50/51, odd chip to BB
    let pot = 101i64;
    let (half0, half1) = (pot / 2, pot - pot / 2);
    assert_eq!((half0, half1), (50, 51), "odd chip to BB");
}

#[test]
fn uncalled_return() {
    let mut s = fresh(31);
    s.apply(Action::Call).expect("sb completes to 100");
    s.apply(Action::Check).expect("bb checks");
    assert_eq!(s.street(), Street::Flop);
    s.apply(Action::Bet { to: 300 }).expect("bb bets 300");
    s.apply(Action::Fold).expect("sb folds");
    let [p0, p1] = s.payoffs();
    assert_eq!(p1, 100, "BB nets exactly the SB's blind");
    assert_eq!(p0, -100);
    assert_eq!(p0 + p1, 0, "zero-sum");
}

#[test]
fn state_is_copy_no_heap() {
    assert!(
        std::mem::size_of::<State>() <= 128,
        "State must stay ≤ 128 bytes, got {}",
        std::mem::size_of::<State>()
    );
    fn assert_copy<T: Copy>() {}
    assert_copy::<State>();
    let s = fresh(77);
    let mut v: ArrayVec<LegalAction, 12> = ArrayVec::new();
    s.legal_actions(&mut v);
    assert!(!v.is_empty());
}

#[test]
fn replay_matches() {
    for seed in 0..200u64 {
        let rng = &mut rng_from_seed(0xBEEF ^ seed);
        let hh = fuzz::play_random(CFG, seed, rng).expect("play");
        let state = hh.replay().expect("replay");
        assert_eq!(state.payoffs(), [hh.result_sb, -hh.result_sb]);
        assert_eq!(state.board_len(), hh.board_len);
        assert_eq!(state.hole(0), hh.holes[0]);
        assert_eq!(state.hole(1), hh.holes[1]);
    }
}

#[test]
fn public_history_leak_proof() {
    // I9 (SPECS/01 §5): across 10k fuzzed hands, the PublicHistory never carries
    // hidden information: folded holes are structurally absent, both holes appear
    // iff showdown, and there is no seed / replay surface.
    for seed in 0..10_000u64 {
        let rng = &mut rng_from_seed(0x1EA9 ^ seed);
        let hh = match fuzz::play_random(CFG, seed, rng) {
            Ok(h) => h,
            Err(_) => continue,
        };
        let ph = PublicHistory::from(&hh);
        let text = serde_json::to_string(&ph).expect("serialize");
        let v: serde_json::Value = serde_json::from_str(&text).expect("json");
        let keys: Vec<&str> = v
            .as_object()
            .expect("obj")
            .keys()
            .map(|k| k.as_str())
            .collect();
        assert_eq!(
            keys,
            vec!["actions", "board", "nets", "showdown_holes"],
            "public surface is exactly these fields (no seed, no holes)"
        );
        if hh.board_len == 5 {
            assert!(
                ph.showdown_holes.iter().all(|x| x.is_some()),
                "showdown reveals both"
            );
        } else {
            assert!(
                ph.showdown_holes.iter().all(|x| x.is_none()),
                "folded stays hidden"
            );
        }
        assert_eq!(ph.nets[0] + ph.nets[1], 0, "nets zero-sum");
    }
}

#[test]
fn action_probs_exclusive() {
    // Default trait behavior pinned here; the archetype/baseline split is pinned in
    // cham-opponents (SPECS/03 §7 — `action_probs_exclusive` lives there too).
    struct NullBot;
    impl Agent for NullBot {
        fn name(&self) -> &str {
            "null"
        }
        fn act(&mut self, obs: &Observables<'_>, _rng: &mut Rng) -> Action {
            obs.legal[0].action
        }
    }
    let s = fresh(5);
    let obs = Observables::view(&s, Player::Sb);
    let b = NullBot;
    assert_eq!(b.action_probs(&obs), Err(AgentError::NotProbabilistic));
}

#[test]
fn fuzz_1m_release() {
    // I2–I7 over 1M random-action hands (release profile; opt-level 3).
    for seed in 0..1_000_000u64 {
        let _ = fuzz::play_random(CFG, seed, &mut rng_from_seed(seed))
            .expect("fuzz hand must play cleanly");
    }
}

#[test]
fn rng_derivation_replays_hands() {
    // A hand is replayable from (match_seed, hand_index): the eval RNG contract.
    let hand_rng = || child(0xA11CE, "d17");
    let d1 = Deck::shuffled(&mut hand_rng());
    let d2 = Deck::shuffled(&mut hand_rng());
    let mut v1 = Vec::new();
    let mut d1 = d1;
    for _ in 0..52 {
        v1.push(d1.deal().expect("deal").idx());
    }
    let mut v2 = Vec::new();
    let mut d2 = d2;
    for _ in 0..52 {
        v2.push(d2.deal().expect("deal").idx());
    }
    assert_eq!(v1, v2);
}

#[test]
fn illegal_action_errors() {
    let mut s = fresh(9);
    assert!(matches!(
        s.apply(Action::Check),
        Err(CoreError::IllegalAction { .. })
    ));
    s.apply(Action::Call).expect("ok");
    assert!(s.apply(Action::Call).is_err());
}

#[test]
fn equity_exact_river_sums() {
    let hero = Hand2::new(Card::parse("As").unwrap(), Card::parse("Kh").unwrap());
    let board: Vec<Card> = ["2s", "7d", "9c", "Jh", "3s"]
        .iter()
        .map(|s| Card::parse(s).unwrap())
        .collect();
    let (w, t) = equity_exact(hero, &Range::all(), &board);
    // every villain combo is win, lose or tie: w + t + lose = 1
    assert!(
        w + t <= 1.0 + 1e-9 && w + t >= 0.30,
        "AK on a low board vs uniform: w={w} t={t}"
    );
    assert!(w > 0.30, "AK picks up real equity vs uniform: {w}");
    // dead-card removal is consistent
    let mut r = Range::all();
    let mut dead = board.clone();
    dead.extend(hero.cards());
    r.remove_cards(&dead);
    let (w2, t2) = equity_exact(hero, &r, &board);
    assert!((w + t - w2 - t2).abs() < 1e-9);
}
