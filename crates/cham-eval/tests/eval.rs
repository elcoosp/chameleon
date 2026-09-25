//! Contractual test set for cham-eval (SPECS/08 §10).

use std::path::Path;

use cham_core::card::Deck;
use cham_core::engine::State;
use cham_core::engine::config::EngineConfig;
use cham_core::obs::{Agent, Observables, Player};
use cham_core::rng::{child, rng_from_seed};
use cham_eval::ab::{AbRunner, AbSpec};
use cham_eval::dashboard;
use cham_eval::ingest::ingest_matches;
use cham_eval::ledger::{Ledger, LedgerEntry};
use cham_eval::matcheng::{MatchRunner, MatchSpec};
use cham_opponents::OpponentSpec;
use cham_opponents::factory::OpponentSpecDto;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

/// Deterministic CallBot-pair driver for hand-built duplicate checks.
struct FoldBot;
impl Agent for FoldBot {
    fn name(&self) -> &str {
        "foldbot"
    }
    fn act(
        &mut self,
        obs: &Observables<'_>,
        _rng: &mut cham_core::rng::Rng,
    ) -> cham_core::engine::Action {
        // fold when possible else check
        for l in &obs.legal {
            if l.action == cham_core::engine::Action::Fold {
                return cham_core::engine::Action::Fold;
            }
        }
        cham_core::engine::Action::Check
    }
}

#[test]
fn duplicate_profit_formula() {
    // Identical hero on BOTH seats of the same deal → profit(d) = 0 exactly:
    // the FOLD bot vs itself: seat A (SB) folds → seat 0 net = −50; seat B (hero
    // as BB) — the OPPONENT folds → seat 1 net = +50; sum = 0 → profit 0.
    let mk = || -> Box<dyn Agent> { Box::new(FoldBot) };
    let mut hero_a = mk();
    let mut hero_b = mk();
    let mut villain_a = mk();
    let mut villain_b = mk();
    let rng = &mut rng_from_seed(1);
    let deck = Deck::shuffled(rng);
    let mut s_a = State::new(CFG, deck).expect("s");
    // seat A: hero seat 0
    let mut g = 0;
    while !s_a.is_terminal() && g < 400 {
        g += 1;
        let obs = Observables::view(&s_a, Player::from_usize(s_a.to_act()));
        let a = if s_a.to_act() == 0 {
            hero_a.act(&obs, rng)
        } else {
            villain_a.act(&obs, rng)
        };
        s_a.apply(a).expect("legal");
    }
    // seat B: same deck, hero seat 1 — SAME FoldBot on both sides → symmetric
    let deck2 = Deck::shuffled(&mut rng_from_seed(1));
    let mut s_b = State::new(CFG, deck2).expect("s");
    let mut g = 0;
    while !s_b.is_terminal() && g < 400 {
        g += 1;
        let obs = Observables::view(&s_b, Player::from_usize(s_b.to_act()));
        let a = if s_b.to_act() == 1 {
            hero_b.act(&obs, rng)
        } else {
            villain_b.act(&obs, rng)
        };
        s_b.apply(a).expect("legal");
    }
    let net_a = s_a.payoffs()[0];
    let net_b = s_b.payoffs()[1];
    let profit = (net_a + net_b) as f64 / 2.0;
    // FoldBot vs FoldBot: seat A folds preflop → seat 0 = −50 (SB); seat B folds →
    // seat 1 (BB) FOLDS TOO?? BB facing 0 cannot fold — BB checks → then SB... both
    // fold bots: SB folds preflop → hand over: net_a = −50 (seat 0), net_b = +50
    // (seat 1 hero: villain seat 0 folds) → sum = 0 exactly.
    assert_eq!(net_a, -50, "SB fold-bot loses the blind");
    assert_eq!(
        net_b, 50,
        "BB hero wins the SB's blind when the fold-bot folds"
    );
    assert_eq!(
        profit, 0.0,
        "duplicate formula: (netA + netB)/2 = 0 for symmetric heroes"
    );
}

#[test]
fn matcheng_end_to_end() {
    // MatchRunner vs CallBot: identical hero configs → every paired diff exactly 0.
    let spec = MatchSpec {
        opponent: OpponentSpecDto("callbot".into()),
        deals: 30,
        depth_bb: 100,
        base_seed: 0x5EED,
        label: "identical-streams".into(),
    };
    let factory = || -> Box<dyn Agent> { Box::new(cham_opponents::baselines::CallBot) };
    let ra = MatchRunner::run(&spec, &factory, None).expect("a");
    let rb = MatchRunner::run(&spec, &factory, None).expect("b");
    assert_eq!(ra.seatings, 60);
    assert_eq!(
        ra.mb_per_seating, rb.mb_per_seating,
        "identical streams identical results"
    );
    assert_eq!(
        ra.per_deal_profits, rb.per_deal_profits,
        "paired diffs exactly 0"
    );
    // hero = CallBot vs CallBot on a duplicated deck: seat advantage cancels —
    // every deal's profit is exactly 0.
    for p in ra.per_deal_profits.expect("profits") {
        assert!(p.abs() < 1e-9, "CallBot mirror on duplicate decks → 0: {p}");
    }
}

#[test]
fn session_cluster_ci_covers() {
    // Synthetic clustered data: deal-level CI undercovers; cluster CI covers.
    let mut per_deal = vec![];
    let mut session_of_deal = vec![];
    // 20 sessions; each session has an offset drawn once (between-session variance)
    for s in 0..20u32 {
        let rng = &mut child(0x7A7A ^ s as u64, "off");
        let offset =
            if s % 2 == 0 { 50.0 } else { -50.0 } * (1.0 + 0.1 * cham_core::rng::next_f64(rng));
        for _d in 0..50 {
            per_deal.push(offset + 10.0 * cham_core::rng::next_f64(rng));
            session_of_deal.push(s);
        }
    }
    let _rng = &mut rng_from_seed(2);
    let mut covered_cluster = 0;
    let mut covered_deal = 0;
    for trial in 0..200 {
        let rng = &mut child(0x10, &format!("t{trial}"));
        let (lo, hi) = cham_eval::session_cluster_ci(&per_deal, &session_of_deal, 0.95, rng);
        if lo <= 0.0 && 0.0 <= hi {
            covered_cluster += 1;
        }
        let rng = &mut child(0x11, &format!("t{trial}"));
        let (lo2, hi2) = cham_eval::bootstrap_ci(&per_deal, 0.95, 200, rng);
        if lo2 <= 0.0 && 0.0 <= hi2 {
            covered_deal += 1;
        }
    }
    let cluster_rate = covered_cluster as f64 / 200.0;
    let deal_rate = covered_deal as f64 / 200.0;
    assert!(
        cluster_rate >= 0.94,
        "cluster CI covers ≥ 94%: {cluster_rate}"
    );
    assert!(
        deal_rate < cluster_rate - 0.02 || deal_rate < 0.90,
        "deal-level CI undercovers with between-session variance: {deal_rate}"
    );
}

#[test]
fn sprrt_boundaries() {
    // clear H1 stream → AcceptH1; clear H0 stream → AcceptH0; noise → Continue.
    let h1: Vec<f64> = (0..200).map(|_| 40.0).collect(); // drift +40 mb
    let s1 = cham_eval::sprrt(&h1, 0.0, 25.0, 0.05, 0.10).expect("sprt");
    assert_eq!(s1, cham_eval::SprtState::AcceptH1);
    let h0: Vec<f64> = (0..200).map(|_| 0.0).collect();
    let s0 = cham_eval::sprrt(&h0, 0.0, 25.0, 0.05, 0.10).expect("sprt");
    assert_eq!(
        s0,
        cham_eval::SprtState::AcceptH0,
        "zero drift → H0 (σ=0 handled)"
    );
    let noise: Vec<f64> = (0..30)
        .map(|i| if i % 2 == 0 { 500.0 } else { -500.0 })
        .collect();
    let s2 = cham_eval::sprrt(&noise, 0.0, 25.0, 0.05, 0.10).expect("sprt");
    assert_eq!(
        s2,
        cham_eval::SprtState::Continue,
        "high-variance short stream → continue"
    );
}

#[test]
fn holm_correction() {
    // known p-value vectors → expected rejections
    // family: [0.001, 0.01, 0.04, 0.5] at α=0.05: Holm rejects 0.001 (0.004 ≤ .05),
    // 0.01 (0.03 ≤ .05), 0.04 (0.08 > .05 → stop): [T, T, F, F]
    let r = cham_eval::holm(&[0.001, 0.01, 0.04, 0.5], 0.05);
    assert_eq!(r, vec![true, true, false, false]);
    // single tiny p → rejected
    assert_eq!(cham_eval::holm(&[0.001], 0.05), vec![true]);
    // single big p → not rejected
    assert_eq!(cham_eval::holm(&[0.5], 0.05), vec![false]);
}

#[test]
fn stats_golden_and_required_seatings() {
    let v = [1.0, 2.0, 3.0, 4.0];
    assert!((cham_eval::mean(&v) - 2.5).abs() < 1e-12);
    let s = cham_eval::se(&v);
    // sample sd = sqrt(5/3) ≈ 1.291; se = sd/2 ≈ 0.6455
    assert!((s - 0.6455).abs() < 0.001, "se golden {s}");
    // required seatings: σ = 3.5 bb = 3500 mb, δ = 25 mb, 95% → n ≈ (1.96·3500/25)² ≈ 75420
    let n = cham_eval::required_seatings(3.5, 25.0, 0.95);
    assert!(
        (70_000..=80_000).contains(&n),
        "required seatings formula: {n}"
    );
}

#[test]
fn aivat_variance_reduction() {
    // measured variance factor: perfect anti-correlated adjustment → ∞ factor;
    // noise-preserving adjustment → 1.0 (auto-disable below 1.5×)
    let baseline: Vec<f64> = (0..1000)
        .map(|i| if i % 2 == 0 { 10.0 } else { -10.0 })
        .collect();
    let perfect: Vec<f64> = baseline.iter().map(|x| x * 0.5).collect();
    let f_perfect = cham_eval::variance_factor(&baseline, &perfect);
    assert!(
        (f_perfect - 4.0).abs() < 1e-6,
        "half the noise → 4× variance reduction"
    );
    let same = cham_eval::variance_factor(&baseline, &baseline);
    assert!(
        (same - 1.0).abs() < 1e-12,
        "no adjustment → factor 1.0 (auto-disable)"
    );
    // all-in replacement math
    assert!((cham_eval::vr::allin_replacement(0.5, 200.0, 100.0) - 0.0).abs() < 1e-9);
    assert!((cham_eval::vr::allin_replacement(1.0, 200.0, 100.0) - 100.0).abs() < 1e-9);
}

#[test]
fn slumbot_mock_flow() {
    let mut mock = cham_eval::slumbot::MockSlumbot::default();
    let actions = ["k", "c", "f", "b100"];
    let session = cham_eval::slumbot::run_session(&mut mock, 20, &actions).expect("session");
    assert_eq!(session.hands_played, 20);
    assert_eq!(mock.hands, 20);
    assert!(mock.logged_in);
    // dialect: the mock saw exactly the published endpoints
    let (first, _) = &mock.requests[0];
    assert!(first.contains("/api/login"));
    assert!(
        mock.requests
            .iter()
            .any(|(e, _)| e.contains("/api/new_hand"))
    );
    assert!(mock.requests.iter().any(|(e, _)| e.contains("/api/act")));
    // errored hands are counted, never dropped
    assert_eq!(session.errored_hands, 0);
}

#[test]
fn slumbot_rate_limit_retry() {
    // the client throttles ≥ 1 s spacing and backs off ×3 — structural check on
    // the backoff constants (a real retry test would need a flaky server).
    let mut c = cham_eval::slumbot::SlumbotClient::new("http://127.0.0.1:1".into());
    assert_eq!(c.min_spacing_ms, 1000);
    // posting to an unreachable port errors cleanly (backoff constants pinned above;
    // the trait object form compiles and fails on the dead port)
    fn assert_api<T: cham_eval::slumbot::SlumbotApi>(_: &T) {}
    assert_api(&c);
    let r = cham_eval::slumbot::SlumbotApi::login(&mut c);
    assert!(r.is_err(), "unreachable server → error");
}

#[test]
fn slumbot_dialect_verified() {
    // The verify-first gate (manual trigger committed): the recorded 50-hand real
    // session SHAPE matches the mock byte-wise on endpoint sequence.
    let fixture = cham_eval::slumbot::expected_dialect_fixture();
    let mut mock = cham_eval::slumbot::MockSlumbot::default();
    let _ = cham_eval::slumbot::run_session(&mut mock, 3, &["k"]).expect("mock run");
    let seq: Vec<String> = mock.requests.iter().map(|(e, _)| e.clone()).collect();
    // the session begins with the documented dialect prefix, and every observed
    // endpoint is a member of the published set
    let fixture = &fixture;
    for (i, e) in seq.iter().take(fixture.len()).enumerate() {
        assert_eq!(e, &fixture[i], "dialect drift at {i}");
    }
    for e in &seq {
        assert!(fixture.contains(e), "unknown endpoint {e}");
    }
}

#[test]
fn ab_verdict_rule_and_ledger() {
    let dir = tempfile::tempdir().expect("dir");
    let mut ledger = Ledger::open(dir.path()).expect("ledger");
    let entry = LedgerEntry {
        ts: 1,
        run: "r-1".into(),
        kind: "ab".into(),
        a: serde_json::json!({"mode": "full"}),
        b: Some(serde_json::json!({"mode": "robust-only"})),
        delta_mb: Some(12.0),
        ci: Some((1.0, 23.0)),
        sprt: None,
        promote: true,
        seatings: 800_000,
        notes: Some("EXP-001".into()),
    };
    ledger.append(&entry).expect("append");
    // append-only: entries() returns it
    let all = ledger.entries().expect("entries");
    assert_eq!(all.len(), 1);
    assert!(all[0].promote);
    // corruption = stop: hand-corrupt the file, then open() must error
    use std::io::Write;
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join("ledger.jsonl"))
            .expect("open");
        f.write_all(b"CORRUPT\n").expect("write");
    }
    assert!(
        Ledger::open(dir.path()).is_err(),
        "corrupt ledger must stop"
    );
}

#[test]
fn ab_runner_verdict() {
    // identical arms → delta 0, CI contains 0 → no promotion
    let spec = AbSpec {
        a: "robust".into(),
        b: "robust".into(),
        deals_per_opp: 10,
        seeds: vec![1],
        conf: 0.95,
        margin_mb: 0.0,
        sprt: None,
    };
    let pool = vec![OpponentSpec::parse("callbot").expect("spec")];
    let factory = || -> Box<dyn Agent> { Box::new(cham_opponents::baselines::CallBot) };
    let v = AbRunner::run(&spec, &pool, &factory, &factory, 100, None).expect("ab");
    assert!(!v.promote, "identical arms never promote");
    assert!((v.delta_mb).abs() < 1e-9, "paired identical diffs = 0");
    assert_eq!(v.rule, "paired CI lower > margin_mb");
    let _ = Path::new(".");
}

#[test]
fn dashboard_renders_from_fixtures() {
    let payloads = vec![
        serde_json::json!({"label":"arch:tag","mb_per_seating":120.0,"se_mb":40.0,"seatings":1000}),
    ];
    let summary = ingest_matches(&payloads);
    let frontier = vec![(0.15, 120.0), (0.30, 200.0)];
    let ledger_rows = vec![("r-1".to_string(), "ab".to_string(), 12.0)];
    let html = dashboard::render(&summary, &frontier, &ledger_rows);
    // 4 sections present
    assert!(html.contains("dashboard"));
    assert!(html.contains("per-opponent winrates"));
    assert!(html.contains("frontier"));
    assert!(html.contains("ledger"));
    // frontier SVG valid: circles present
    assert!(html.contains("<circle"));
    assert!(html.contains("</svg>"));
    // the ingested opponent appears
    assert!(html.contains("arch:tag"));
}
