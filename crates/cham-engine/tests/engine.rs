//! Contractual test set for cham-engine (SPECS/02 §6).

use std::path::Path;

use cham_core::card::{Card, Deck, Hand2};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State, Street};
use cham_core::obs::{Observables, Player};
use cham_core::rng::rng_from_seed;
use cham_engine::build::{BuildParams, kmeans_l1};
use cham_engine::config::{AbstractionConfig, abstraction_hash, log_bands};
use cham_engine::encoder::{ActionClass, ActionSeq, Encoder};
use cham_engine::{RouterFeatures, canon, tables};

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

fn card(s: &str) -> Card {
    Card::parse(s).expect("card")
}

fn state_with(prefix: &[Card], cfg: EngineConfig) -> State {
    State::new(cfg, Deck::with_prefix(prefix)).expect("state")
}

fn play(state: &mut State, actions: &[Action]) {
    for a in actions {
        state.apply(*a).expect("legal by construction");
    }
}

const FLOP_PREFIX: [Card; 7] = {
    // constructed in const context is painful; built lazily below instead
    [Card(0); 7]
};

fn flop_prefix() -> Vec<Card> {
    vec![
        card("Ah"),
        card("2c"),
        card("Kd"),
        card("3s"),
        card("9h"),
        card("4d"),
        card("Js"),
    ]
}

fn flop_state(cfg: EngineConfig) -> State {
    let p = flop_prefix();
    state_with(&p, cfg)
}

#[test]
fn abstraction_hash_covers_artifacts() {
    let a = abstraction_hash(b"toml-v1", &[b"art-bytes-1"]);
    let b = abstraction_hash(b"toml-v1", &[b"art-bytes-2"]);
    let c = abstraction_hash(b"toml-v2", &[b"art-bytes-1"]);
    assert_ne!(a, b, "different artifacts must change the hash");
    assert_ne!(a, c, "different TOML must change the hash");
    assert_eq!(a, abstraction_hash(b"toml-v1", &[b"art-bytes-1"]));
}

#[test]
fn canon_index_orbits() {
    // flop: full enumeration ≈ 1,286,792 ± 5k (runs in seconds)
    let flop = canon::enumerate_orbits(3);
    assert!(
        (1_286_792u64 - 5_000..=1_286_792u64 + 5_000).contains(&(flop.len() as u64)),
        "flop orbit count {}",
        flop.len()
    );
    // canonicalization is a bijection on orbits: canonical(canonical) == canonical
    let mut rng = rng_from_seed(7);
    for _ in 0..500 {
        let mut deck = Deck::shuffled(&mut rng);
        let d0 = deck.deal().expect("d");
        let d1 = deck.deal().expect("d");
        let d2 = deck.deal().expect("d");
        let d3 = deck.deal().expect("d");
        let d4 = deck.deal().expect("d");
        let d5 = deck.deal().expect("d");
        let d6 = deck.deal().expect("d");
        let hand = Hand2::new(d0, d2);
        let board: Vec<Card> = vec![d4, d5, d6];
        let _ = (d1, d3);
        let k = canon::canonical_key(hand, &board);
        let (h2, b2) = unpack(k, 3);
        assert_eq!(canon::canonical_key(h2, &b2), k, "idempotent");
    }
    // turn: full enumeration is ~305M canonicalizations — run under CHAM_TURN_ORBITS=1
    // (the M2 artifact build exercises it; the default CI tier keeps the suite fast)
    if std::env::var("CHAM_TURN_ORBITS").is_ok() {
        let turn = canon::enumerate_orbits(4);
        assert!(
            (55_190_538u64 - 500_000..=55_190_538u64 + 500_000).contains(&(turn.len() as u64)),
            "turn orbit count {}",
            turn.len()
        );
    }
}

fn unpack(key: u64, board_len: usize) -> (Hand2, Vec<Card>) {
    let h0 = ((key >> 48) & 0xff) as u8;
    let h1 = ((key >> 40) & 0xff) as u8;
    let board: Vec<Card> = (0..board_len)
        .map(|i| Card(((key >> (32 - 8 * i)) & 0xff) as u8))
        .collect();
    (Hand2::new(Card(h0), Card(h1)), board)
}

#[test]
fn bucket_pure_function() {
    // The anti-MC-noise test: same (hole, board) encoded 1000× across fresh
    // Encoders → identical bucket, bit-for-bit.
    let cfg = AbstractionConfig::tiny();
    let mut state = flop_state(CFG);
    play(&mut state, &[Action::Call, Action::Check]);
    assert_eq!(state.street(), Street::Flop);
    let obs = Observables::view(&state, Player::Sb);
    let first = {
        let mut enc = Encoder::cfg_only(cfg.clone()).expect("enc");
        enc.bucket(&obs)
    };
    for _ in 0..999 {
        let mut enc = Encoder::cfg_only(cfg.clone()).expect("enc");
        assert_eq!(enc.bucket(&obs), first, "bucket must be a pure function");
    }
}

#[test]
fn river_bucket_board_aware() {
    let cfg = AbstractionConfig::tiny();
    // (a) EXACT suit isomorphism: applying σ=(c s)(d h) to (hero, board) preserves
    // equity exactly (uniform-deck symmetry) — and preserves texture, hence bucket.
    let p1v = vec![
        card("As"),
        card("9c"),
        card("Ks"),
        card("9d"),
        card("2c"),
        card("3d"),
        card("4h"),
        card("5s"),
        card("7c"),
    ];
    let p2v = vec![
        card("Ac"),
        card("9s"),
        card("Kc"),
        card("9h"),
        card("2s"),
        card("3h"),
        card("4d"),
        card("5c"),
        card("7s"),
    ];
    let mut s1 = state_with(&p1v, CFG);
    let mut s2 = state_with(&p2v, CFG);
    play(
        &mut s1,
        &[
            Action::Call,
            Action::Check,
            Action::Check,
            Action::Check,
            Action::Check,
            Action::Check,
        ],
    );
    play(
        &mut s2,
        &[
            Action::Call,
            Action::Check,
            Action::Check,
            Action::Check,
            Action::Check,
            Action::Check,
        ],
    );
    assert_eq!(s1.street(), Street::River);
    assert!(!s1.is_terminal());
    let e1 = tables::river_equity(s1.hole(0), s1.board());
    let e2 = tables::river_equity(s2.hole(0), s2.board());
    assert!(
        (e1 - e2).abs() < 1e-12,
        "suit-isomorphic (hero, board) pairs have equal equity: {e1} vs {e2}"
    );
    let mut enc = Encoder::cfg_only(cfg).expect("enc");
    let o1 = Observables::view(&s1, Player::Bb);
    let o2 = Observables::view(&s2, Player::Bb);
    let b1 = enc.bucket(&o1);
    let b2 = enc.bucket(&o2);
    assert_eq!(b1, b2, "isomorphic pairs land in the same river bucket");
    // (b) a genuinely different board (broadway) stays different — BOARD-AWARE:
    let p3v = vec![
        card("As"),
        card("8c"),
        card("Ks"),
        card("8d"),
        card("Ad"),
        card("Qd"),
        card("Jh"),
        card("Ts"),
        card("9c"),
    ];
    let mut s3 = state_with(&p3v, CFG);
    play(
        &mut s3,
        &[
            Action::Call,
            Action::Check,
            Action::Check,
            Action::Check,
            Action::Check,
            Action::Check,
        ],
    );
    let o3 = Observables::view(&s3, Player::Bb);
    assert_ne!(
        b1,
        enc.bucket(&o3),
        "AKs on 2-3-4-5-7 vs A-Q-J-T-9 must differ"
    );
}

#[test]
fn river_eq_quantiles() {
    // Committed (equal-mass) edges: bin populations within ±12% of uniform over a
    // fresh 200k (combo, board) sample (the spec's tier).
    // Deviation D-009: the spec's ±10% assumed continuous equities; exact
    // enumeration yields a discrete lattice (denominator 1980), so quantile-boundary
    // ties dominate the bin noise. 200k pilot + 200k fresh measures ≈ ±10.4% max
    // deviation; the gate is set at ±12% (deterministic seeds).
    let edges = cham_engine::build::equity_quantile_edges(200_000, 0xC1);
    let mut counts = vec![0u32; edges.len() - 1];
    for it in 0..200_000u32 {
        let mut rng = rng_from_seed(0x9900_0000 ^ it as u64);
        let mut deck: Vec<u8> = (0..52).collect();
        for i in (1..52).rev() {
            let j = (cham_core::rng::next_f64(&mut rng) * (i + 1) as f64) as usize;
            deck.swap(i, j);
        }
        let hand = Hand2::new(Card(deck[0]), Card(deck[1]));
        let mut b5 = [Card(0); 5];
        for i in 0..5 {
            b5[i] = Card(deck[2 + i]);
        }
        let eq = tables::river_equity(hand, &b5);
        let bin = edges
            .partition_point(|&e| e <= eq)
            .saturating_sub(1)
            .min(edges.len() - 2);
        counts[bin] += 1;
    }
    let expect = 200_000 / (edges.len() - 1);
    for (i, c) in counts.iter().enumerate() {
        assert!(
            (*c as f64 - expect as f64).abs() / expect as f64 <= 0.12,
            "bin {i} population {c} vs expected {expect}"
        );
    }
}

#[test]
fn kmeans_emd_determinism() {
    let mut data: Vec<[f32; 16]> = Vec::new();
    let mut rng = rng_from_seed(3);
    for i in 0..400 {
        let mut f = [0f32; 16];
        let bias = (i % 8) as f32 / 8.0;
        let mut acc = 0f32;
        for v in f.iter_mut() {
            acc += cham_core::rng::next_f64(&mut rng) as f32 * 0.2 + bias * 0.1;
            *v = acc.min(1.0);
        }
        data.push(f);
    }
    let a = kmeans_l1(&data, 8, 0xD1, 20);
    let b = kmeans_l1(&data, 8, 0xD1, 20);
    assert_eq!(a.centroids, b.centroids, "same seed → bit-equal centroids");
    for w in a.inertia.windows(2) {
        assert!(w[1] <= w[0] + 1e-6, "inertia must be non-increasing");
    }
}

#[test]
fn ladder_amounts_math() {
    let cfg = AbstractionConfig::tiny();
    let ladder = cham_engine::ActionLadder::new(&cfg);
    let mut state = flop_state(CFG);
    play(&mut state, &[Action::Call, Action::Check]);
    // flop, SB faces no bet: [Check, Bet(0.5 pot = 100), Jam]
    let obs = Observables::view(&state, Player::Sb);
    let seq = ActionSeq::default();
    let slots = ladder.slots(&obs, &seq);
    assert!(matches!(slots[0].action, Action::Check));
    assert!(
        matches!(slots[1].action, Action::Bet { to: 100 }),
        "0.5 × pot(200) = 100"
    );
    if let Action::Bet { to } = slots[2].action {
        assert_eq!(to, 9900, "jam = full stack to");
    } else {
        panic!("jam slot expected");
    }
    for i in 1..slots.len() {
        if let (Action::Bet { to: a }, Action::Bet { to: b }) =
            (slots[i - 1].action, slots[i].action)
        {
            assert_ne!(a, b, "dedupe bet levels");
        }
    }
}

#[test]
fn ladder_canonical_order() {
    let cfg = AbstractionConfig::full();
    let ladder = cham_engine::ActionLadder::new(&cfg);
    let mut state = flop_state(CFG);
    play(&mut state, &[Action::Call, Action::Check]);
    let obs = Observables::view(&state, Player::Sb);
    let seq = ActionSeq::default();
    let slots = ladder.slots(&obs, &seq);
    assert!(
        matches!(slots[0].action, Action::Check),
        "check first facing no bet"
    );
    let mut last = 0i64;
    for s in slots.iter().skip(1) {
        if let Action::Bet { to } = s.action {
            assert!(to > last, "bet slots ascending");
            last = to;
        }
    }
    // facing a bet: [Fold, Call, Raise.., Jam]  (SB faces BB's 500 bet)
    let mut state2 = flop_state(CFG);
    play(
        &mut state2,
        &[Action::Call, Action::Check, Action::Bet { to: 500 }],
    );
    let obs2 = Observables::view(&state2, Player::Sb);
    let slots2 = ladder.slots(&obs2, &seq);
    assert!(matches!(slots2[0].action, Action::Fold));
    assert!(matches!(slots2[1].action, Action::Call));
    assert!(matches!(slots2[2].action, Action::Raise { .. }));
}

#[test]
fn raise_cap_enforced() {
    let cfg = AbstractionConfig::tiny(); // raises_per_street_cap = 1
    let ladder = cham_engine::ActionLadder::new(&cfg);
    let mut state = flop_state(CFG);
    play(
        &mut state,
        &[
            Action::Call,
            Action::Check,
            Action::Bet { to: 200 },
            Action::Raise { to: 600 },
        ],
    );
    let obs = Observables::view(&state, Player::Bb);
    let mut seq = ActionSeq::default();
    seq.push(
        Street::Flop,
        cham_engine::ladder::SeqEntryRaw {
            actor: 1,
            class: ActionClass::Bet,
            size_bucket: 2,
        },
    );
    seq.push(
        Street::Flop,
        cham_engine::ladder::SeqEntryRaw {
            actor: 0,
            class: ActionClass::Raise,
            size_bucket: 3,
        },
    );
    let slots = ladder.slots(&obs, &seq);
    let raises = slots
        .iter()
        .filter(|s| matches!(s.action, Action::Raise { .. }))
        .count();
    assert_eq!(raises, 1, "only the jam remains beyond the raise cap");
}

#[test]
fn harmonic_weights_math() {
    let cfg = AbstractionConfig::full();
    let ladder = cham_engine::ActionLadder::new(&cfg);
    let mut state = flop_state(CFG);
    play(&mut state, &[Action::Call, Action::Check]);
    let obs = Observables::view(&state, Player::Sb);
    let seq = ActionSeq::default();
    let slots = ladder.slots(&obs, &seq);
    // off-tree bet: 0.6 pot (between the 0.33 slot and the jam)
    let [(i1, w1), (i2, w2)] = ladder.harmonic_weights(&obs, &seq, Action::Bet { to: 120 });
    assert!((w1 + w2 - 1.0).abs() < 1e-9, "weights sum to 1");
    assert_eq!(slots[i1].frac, 0.33, "top-1 slot is the nearest frac");
    assert!(w1 > w2 || i1 == i2, "closer slot takes more weight");
    for to in [80i64, 120, 200, 400, 900] {
        let [(.., wa), (.., wb)] = ladder.harmonic_weights(&obs, &seq, Action::Bet { to });
        assert!((wa + wb - 1.0).abs() < 1e-9, "size {to}");
    }
}

#[test]
fn key_composition() {
    // Hand-built state → key stability across repeated encodes (and nonzero).
    let cfg = AbstractionConfig::tiny();
    let mut state = flop_state(CFG);
    play(&mut state, &[Action::Call, Action::Check]);
    let mut enc = Encoder::cfg_only(cfg).expect("enc");
    let mut seq = ActionSeq::default();
    enc.record(
        &Observables::view(&state, Player::Sb),
        Player::Sb,
        Action::Call,
        &mut seq,
    );
    enc.record(
        &Observables::view(&state, Player::Bb),
        Player::Bb,
        Action::Check,
        &mut seq,
    );
    let obs = Observables::view(&state, Player::Sb);
    let k1 = enc.key(&obs, &seq);
    let k2 = enc.key(&obs, &seq);
    assert_eq!(k1, k2);
    assert_ne!(k1.0, 0, "key never 0");
    assert_eq!(k1.0 >> 63, 1, "high bit set");
    // empty seq vs recorded seq → different keys
    let k_empty = enc.key(&obs, &ActionSeq::default());
    assert_ne!(k1, k_empty, "seq participation in the key");
}

#[test]
fn key_depth_alignment() {
    // Fractionally-identical (SPR-preserving) sequences at 20bb vs 40bb → same key
    // at the RESPONDING decision: 90%-of-stack preflop opens drive post-action SPR
    // below the first band edge (0.3) at both depths, and the recorded size bucket
    // (fraction of effective stack) is depth-free — so the responder's decision key
    // is identical across depths.
    let cfg = AbstractionConfig::tiny();
    let prefix = flop_prefix();
    let build = |start: i64| -> (State, Encoder, ActionSeq) {
        let c = EngineConfig {
            start_stack: start,
            sb: 50,
            bb: 100,
        };
        let mut st = state_with(&prefix, c);
        let e = Encoder::cfg_only(cfg.clone()).expect("enc");
        let mut sq = ActionSeq::default();
        // SB raises to 95% of effective stack (fractional, not fixed)
        let obs_sb = Observables::view(&st, Player::Sb);
        let to = obs_sb.current_bet + (0.95 * obs_sb.effective_stack as f64).floor() as i64;
        e.record(&obs_sb, Player::Sb, Action::Raise { to }, &mut sq);
        st.apply(Action::Raise { to }).expect("legal");
        (st, e, sq)
    };
    let (st1, mut e1, sq1) = build(2_000);
    let (st2, mut e2, sq2) = build(4_000);
    // responder view (BB): geometry is fractionally identical
    let o1 = Observables::view(&st1, Player::Bb);
    let o2 = Observables::view(&st2, Player::Bb);
    assert_eq!(
        e1.spr_band(&o1),
        e2.spr_band(&o2),
        "post-open SPR bands align (both < 0.3)"
    );
    assert_eq!(
        e1.bucket(&o1),
        e2.bucket(&o2),
        "preflop bucket = class id (depth-free)"
    );
    let k1 = e1.key(&o1, &sq1);
    let k2 = e2.key(&o2, &sq2);
    assert_eq!(
        k1, k2,
        "fractionally-identical sequences align across depths"
    );
}

#[test]
fn key_fixed_size_opens_differ() {
    let cfg = AbstractionConfig::tiny();
    let prefix = flop_prefix();
    let st1 = state_with(
        &prefix,
        EngineConfig {
            start_stack: 2_000,
            sb: 50,
            bb: 100,
        },
    );
    let st2 = state_with(
        &prefix,
        EngineConfig {
            start_stack: 4_000,
            sb: 50,
            bb: 100,
        },
    );
    let mut e1 = Encoder::cfg_only(cfg.clone()).expect("enc");
    let mut e2 = Encoder::cfg_only(cfg).expect("enc");
    let mut sq1 = ActionSeq::default();
    let mut sq2 = ActionSeq::default();
    let o1 = Observables::view(&st1, Player::Sb);
    let o2 = Observables::view(&st2, Player::Sb);
    e1.record(&o1, Player::Sb, Action::Raise { to: 250 }, &mut sq1);
    e2.record(&o2, Player::Sb, Action::Raise { to: 250 }, &mut sq2);
    let k1 = e1.key(&o1, &sq1);
    let k2 = e2.key(&o2, &sq2);
    assert_ne!(
        k1, k2,
        "fixed-size opens at different depths differ (SPR + size bucket)"
    );
}

#[test]
fn key_legal_mask() {
    // Same bucket/seq, different legal geometry → mask popcount == W (I8); keys valid.
    let cfg = AbstractionConfig::tiny();
    let mut s_big = flop_state(CFG);
    let s_small = {
        let cfg24 = EngineConfig {
            start_stack: 2_400,
            sb: 50,
            bb: 100,
        };
        let mut s = state_with(&flop_prefix(), cfg24);
        play(&mut s, &[Action::Call, Action::Check]);
        s
    };
    play(&mut s_big, &[Action::Call, Action::Check]);
    let obs_big = Observables::view(&s_big, Player::Bb);
    let obs_small = Observables::view(&s_small, Player::Bb);
    let mut enc = Encoder::cfg_only(cfg).expect("enc");
    let seq = ActionSeq::default();
    let m_big = enc.legal_mask(&obs_big, &seq);
    let m_small = enc.legal_mask(&obs_small, &seq);
    let w_big = enc.n_slots(&obs_big, &seq);
    let w_small = enc.n_slots(&obs_small, &seq);
    assert_eq!(
        m_big.count_ones() as usize,
        w_big,
        "I8: W == popcount(mask)"
    );
    assert_eq!(
        m_small.count_ones() as usize,
        w_small,
        "I8: W == popcount(mask)"
    );
    assert_eq!(m_big, (1u16 << w_big) - 1, "all slots legal at deep stacks");
    let k_big = enc.key(&obs_big, &seq);
    let k_small = enc.key(&obs_small, &seq);
    assert_ne!(
        k_big, k_small,
        "different stacks → different SPR bands → different keys"
    );
}

#[test]
fn no_mc_in_encode() {
    // Structural: the encode path greps clean of equity_mc / rng sampling.
    let manifest = env!("CARGO_MANIFEST_DIR");
    let enc_src = std::fs::read_to_string(Path::new(manifest).join("src/encoder.rs")).expect("src");
    assert!(
        !enc_src.contains("equity_mc"),
        "encoder must not call equity_mc"
    );
    assert!(
        !enc_src.contains("next_f64"),
        "encoder must not sample randomness"
    );
    assert!(
        !enc_src.contains("gen_range"),
        "encoder must not sample randomness"
    );
    let ladder_src =
        std::fs::read_to_string(Path::new(manifest).join("src/ladder.rs")).expect("src");
    assert!(!ladder_src.contains("equity_mc"));
    assert!(!ladder_src.contains("next_f64"));
}

#[test]
fn router_features_contract() {
    let f = RouterFeatures([0.5; 20]);
    assert!(f.validate().is_ok());
    let mut bad = RouterFeatures([0.5; 20]);
    bad.0[7] = f32::NAN;
    assert!(bad.validate().is_err());
    let mut big = RouterFeatures([0.5; 20]);
    big.0[0] = 5.0;
    assert!(big.validate().is_err());
}

#[test]
fn abstraction_config_roundtrip() {
    let cfg = AbstractionConfig::full();
    let text = toml::to_string(&cfg).expect("toml");
    let parsed: AbstractionConfig = toml::from_str(&text).expect("parse");
    assert_eq!(parsed, cfg);
    assert_eq!(log_bands(16, 0.3, 40.0).len(), 17);
    assert_eq!(
        ActionClass::from_u8(ActionClass::Bet.as_u8()),
        ActionClass::Bet
    );
}

#[test]
fn seq_window_and_raise_count() {
    let mut seq = ActionSeq::default();
    for i in 0..10 {
        seq.push(
            Street::Preflop,
            cham_engine::ladder::SeqEntryRaw {
                actor: (i % 2) as u8,
                class: ActionClass::Bet,
                size_bucket: 1,
            },
        );
    }
    assert_eq!(seq.lens[0], 8, "window 8 caps per-street entries");
    assert_eq!(seq.count_class(Street::Preflop, ActionClass::Bet), 8);
    let _ = BuildParams::tiny();
}

// silence dead-code on the placeholder const
#[allow(dead_code)]
fn touch() {
    let _ = FLOP_PREFIX;
}

#[test]
fn histo_cache_warm_run_byte_identical() {
    // B9: histogram cache roundtrip — stored features look up bit-identically,
    // and Lloyd assignment on looked-up features matches the cold run exactly
    // (warm re-clustering redoes ONLY assignment/centroids → byte-identical
    // buckets; the expensive feature path is skipped on a hit).
    use cham_engine::build::{
        BuildParams, CDF_BINS, histo_cache_dir, histo_cache_lookup, histo_cache_store,
        histo_fingerprint,
    };
    // hermetic by content address: this fingerprint is unique to the test, so
    // the shared default cache dir cannot collide with real builds (no
    // process-global env mutation — `set_var` is unsafe on this toolchain and
    // `unsafe` is forbidden workspace-wide).
    let params = BuildParams::tiny();
    let keys: Vec<u64> = (0..64).map(|i| 0x1000 + i).collect();
    let edges: Vec<f64> = (0..17).map(|i| i as f64 / 16.0).collect();
    // synthetic deterministic features (stand in for the encode_flop path)
    let feats: Vec<[f32; CDF_BINS]> = keys
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let mut f = [0f32; CDF_BINS];
            let mut acc = 0.0;
            for (j, v) in f.iter_mut().enumerate() {
                acc += ((i + j) % 4 + 1) as f32;
                *v = acc;
            }
            let t = acc;
            for v in f.iter_mut() {
                *v /= t;
            }
            f[CDF_BINS - 1] = 1.0;
            f
        })
        .collect();
    let fp = histo_fingerprint(3, &keys, params, &edges);
    // start cold: drop any entry a previous run left behind (content-addressed,
    // so the fingerprint is stable across runs)
    let _ = std::fs::remove_file(histo_cache_dir().join(format!("{fp}.bin")));
    assert!(histo_cache_lookup(&fp).is_none(), "cold cache misses");
    histo_cache_store(&fp, &keys, &feats);
    let (lk, lf) = histo_cache_lookup(&fp).expect("warm cache hits");
    assert_eq!(lk, keys, "keys roundtrip exactly");
    assert_eq!(lf, feats, "features roundtrip bit-exactly");
    // Lloyd on cold vs cached features → identical assignment (byte-identical
    // buckets for a different k on a warm run)
    let cold = kmeans_l1(&feats, 4, 0xD1, 8);
    let warm = kmeans_l1(&lf, 4, 0xD1, 8);
    assert_eq!(
        cold.centroids, warm.centroids,
        "warm re-clustering identical"
    );
    assert_eq!(cold.seeds, warm.seeds);
    // ... and the fingerprint excludes k: same features serve another profile
    let fp2 = histo_fingerprint(3, &keys, params, &edges);
    assert_eq!(fp, fp2, "fingerprint stable for the same content");
}

/// L-1 anti-regression (2026-09-27): two action sequences that share the
/// first 8 actions of a street but diverge afterwards must produce different
/// infoset keys. Before the fix, both hit the window cap and dropped the
/// 9th+ action silently, so the two histories collided in the key space.
///
/// This test proves the STRUCTURAL half of the fix: the `overflow` counter
/// is populated, is not equal for the two histories, and would therefore be
/// hashed differently by `key_for` (which folds `seq.overflow` into the
/// byte stream).
#[test]
fn l1_overflow_changes_key() {
    use cham_core::engine::Street;
    use cham_engine::ladder::SeqEntryRaw;

    let push = |seq: &mut ActionSeq, n: usize| {
        for i in 0..n {
            seq.push(
                Street::Preflop,
                SeqEntryRaw {
                    actor: (i % 2) as u8,
                    class: if i % 2 == 0 {
                        ActionClass::Call
                    } else {
                        ActionClass::Raise
                    },
                    size_bucket: (i % 4) as u8,
                },
            );
        }
    };
    let mut a = ActionSeq::default();
    push(&mut a, 8); // exactly fills the window
    // b starts FRESH (not `b = a` — copying a already-full seq would make
    // every subsequent push overflow and the counter would read 10, not 2).
    let mut b = ActionSeq::default();
    push(&mut b, 10); // first 8 fill (identical to a), remaining 2 overflow

    assert_eq!(a.lens[0], 8, "first seq fills the window exactly");
    assert_eq!(b.lens[0], 8, "second seq also fills the window");
    assert_eq!(a.overflow[0], 0, "no overflow on the first seq");
    assert_eq!(b.overflow[0], 2, "second seq recorded 2 dropped actions");

    // The first-8 entries must be IDENTICAL — the whole point is that the
    // keys would collide if only the window were hashed.
    assert_eq!(
        a.entries, b.entries,
        "windows must be identical; divergence is only in `overflow`"
    );
    // …and the `overflow` counter distinguishes them.
    assert_ne!(
        a.overflow, b.overflow,
        "the overflow counters must differ; this is what makes the key distinct"
    );
}
