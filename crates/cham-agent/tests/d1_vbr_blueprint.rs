//! Decision D1 (plan): run the exact river VBR against the SHIPPED
//! blueprint. Builds a river spot, queries the blueprint's per-combo
//! strategy, and reports the exploitability the honest ruler sees.
//!
//! FIRST READOUT: river-slice only (the blueprint's river play vs a perfect
//! river best-responder at sampled spots), not the full-game VBR. Printed
//! with the fraction of combos actually covered by the blueprint.
//!
//! Run: CHAM_D1_BP=artifacts/agent-honest-19dim/robust \
//!      cargo nextest run -p cham-agent -E 'test(d1_vbr)' --run-ignored all --no-capture

use cham_blueprint::policy::BlueprintPolicy;
use cham_core::card::{Card, Deck, Hand2};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State, Street};
use cham_core::obs::{Observables, Player};
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};
use cham_search::vbr::RiverVbr;

const CFG: EngineConfig = EngineConfig { start_stack: 10_000, sb: 50, bb: 100 };

fn all_combos(board: &[Card; 5], n: u8) -> Vec<[u8; 2]> {
    let mut used = [false; 52];
    for c in board { used[c.idx() as usize] = true; }
    let mut out = Vec::new();
    for a in 0..52u8 {
        if used[a as usize] { continue; }
        for b in (a + 1)..52u8 {
            if used[b as usize] { continue; }
            out.push([a, b]);
            if out.len() >= n as usize { return out; }
        }
    }
    out
}

/// Build a river State with the given hero combo (player 0) + board; play a
/// fixed line (call, check, x/x, x/x) to reach the river. Returns None if the
/// line is illegal.
fn river_state(hero: [u8; 2], board: &[Card; 5], enc: &mut Encoder) -> Option<(State, ActionSeq)> {
    // deck order: h0a,h1a,h0b,h1b, then board. Villain gets two arbitrary
    // cards (they only matter for the VILLAIN node's blueprint query, which
    // this river-slice harness does not do — it uses a uniform villain).
    // Pick two dummy villain cards DISJOINT from hero + board (else
    // Deck::with_prefix overruns on the duplicate).
    let mut used = [false; 52];
    used[hero[0] as usize] = true;
    used[hero[1] as usize] = true;
    for c in board { used[c.idx() as usize] = true; }
    let mut dummy = Vec::new();
    for x in 0..52u8 {
        if !used[x as usize] {
            dummy.push(x);
            if dummy.len() == 2 { break; }
        }
    }
    let (v0, v1) = (dummy[0], dummy[1]);
    let prefix = [
        Card(hero[0]), Card(v0), Card(hero[1]), Card(v1),
        board[0], board[1], board[2], board[3], board[4],
    ];
    let mut st = State::new(CFG, Deck::with_prefix(&prefix)).ok()?;
    let mut seq = ActionSeq::default();
    // Play call/check to the river, RECORDING each action into the seq
    // (the blueprint key is (obs, seq); an empty seq matches nothing).
    let mut guard = 0;
    while st.street() != Street::River && !st.is_terminal() && guard < 20 {
        guard += 1;
        let p = st.to_act();
        let obs = Observables::view(&st, Player::from_usize(p));
        let a = if cham_core::obs::is_legal(&obs, Action::Check) { Action::Check } else { Action::Call };
        enc.record(&obs, Player::from_usize(p), a, &mut seq);
        if st.apply(a).is_err() { return None; }
    }
    if st.street() == Street::River { Some((st, seq)) } else { None }
}

#[test]
#[ignore = "needs shipped bundle; river-slice D1 readout"]
fn d1_vbr_river_slice() {
    let bp_dir = std::env::var("CHAM_D1_BP")
        .unwrap_or_else(|_| "artifacts/agent-honest-19dim/robust".into());
    let policy = BlueprintPolicy::load(std::path::Path::new(&bp_dir), 0).expect("load bp");
    // The encoder MUST match the one the blueprint was trained with:
    // its bucket artifacts, not cfg_only (which uses the fallback and
    // produces different keys -> every lookup misses).
    let bundle = std::path::Path::new(&bp_dir).parent().expect("bundle dir");
    let cfg_path = bundle.join("abstraction.toml");
    let cfg = std::fs::read_to_string(&cfg_path).ok()
        .and_then(|t| cham_engine::config::parse_config(&t).ok())
        .unwrap_or_else(AbstractionConfig::tiny);
    let buckets = bundle.join("buckets");
    let base_enc = if buckets.exists() {
        Encoder::from_artifacts_dir(&buckets, cfg.clone()).expect("enc from artifacts")
    } else {
        Encoder::cfg_only(cfg.clone()).expect("enc cfg_only")
    };

    // A few fixed boards.
    let boards: [[u8; 5]; 3] = [
        [0, 5, 10, 20, 30],
        [1, 6, 11, 21, 31],
        [2, 7, 12, 22, 32],
    ];
    let mut total_ev = 0.0f64;
    let mut total_n = 0.0f64;
    let mut covered = 0usize;
    let mut total = 0usize;

    for b in boards {
        let board = [Card(b[0]), Card(b[1]), Card(b[2]), Card(b[3]), Card(b[4])];
        let hc = all_combos(&board, 60);
        let hero: Vec<[u8; 2]> = hc.clone();
        let vill: Vec<[u8; 2]> = hc.clone();

        // rank = exact river equity vs a random hand (proxy ordering).
        let hr: Vec<u32> = hero.iter().map(|c| {
            (cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &board) * 1e6) as u32
        }).collect();
        let vr = hr.clone();
        let hw = vec![1.0f64 / hero.len() as f64; hero.len()];
        let vw = vec![1.0f64 / vill.len() as f64; vill.len()];

        // Blueprint strategy per hero combo at the river root (uniform fallback).
        let mut enc = base_enc.clone();
        let mut table: Vec<Vec<f64>> = Vec::with_capacity(hero.len());
        for c in &hero {
            let dist = river_state(*c, &board, &mut enc).and_then(|(st, seq)| {
                let obs = Observables::view(&st, Player::from_usize(st.to_act()));
                let mut e2 = enc.clone();
                policy.strategy(&obs, &mut e2, &seq).map(|s| (s, obs.legal.len()))
            });
            match dist {
                Some((s, n)) => {
                    covered += 1;
                    let mut d = s;
                    let tot: f64 = d.iter().sum();
                    if tot > 0.0 { for x in d.iter_mut() { *x /= tot; } }
                    while d.len() < 4 { d.push(if d.len() < n { 1.0 / n as f64 } else { 0.0 }); }
                    table.push(d);
                }
                None => table.push(vec![0.25; 4]),
            }
            total += 1;
        }
        // policy closure: (path, combo_idx) -> probs. Path-independent here
        // (river-root slice); deeper nodes reuse the root table (a coarse
        // first cut — the honest full-tree VBR re-queries per node).
        let mut ti = 0usize;
        let mut pol = |_path: &str, j: usize| -> Vec<f64> {
            table.get(j.min(table.len().saturating_sub(1))).cloned().unwrap_or_else(|| vec![0.25; 4])
        };
        let _ = &mut ti;
        let fracs = [0.5f64, 1.0];
        let mut vbr = RiverVbr {
            hero: &hero, hero_rank: &hr, hero_w: &hw,
            vill: &vill, vill_rank: &vr, vill_w: &vw,
            pot: 20.0, stack: 90.0, bet_fracs: &fracs, policy: &mut pol,
        };
        let ev = vbr.best_response();
        total_ev += ev;
        total_n += 1.0;
        eprintln!("  board {b:?}: VBR {ev:.3} bb over {} combos", hero.len());
    }
    eprintln!("\n=== D1 (river slice) ===");
    eprintln!("  mean VBR per spot: {:.3} bb", total_ev / total_n.max(1.0));
    eprintln!("  blueprint coverage: {covered}/{total} combos ({:.1}%)",
        100.0 * covered as f64 / total.max(1) as f64);
    eprintln!("  (positive = a perfect river player beats the blueprint by this much)");
}
