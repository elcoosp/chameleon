//! Decision D1 (plan): the honest river VBR against the SHIPPED blueprint.
//! Fixes over the preliminary: (1) villain policy queried from the VILLAIN
//! seat, (2) per-path villain table (checked-to / vs each bet / vs jam),
//! (3) spread ranges, (4) aggregate over boards with a standard error.
//!
//! Run: CHAM_D1_BP=artifacts/agent-honest-19dim/robust \
//!      cargo nextest run -p cham-agent -E 'test(d1_vbr_full)' \
//!        --run-ignored all --no-capture

use cham_blueprint::policy::BlueprintPolicy;
use cham_core::card::{Card, Deck, Hand2};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State, Street};
use cham_core::obs::{Observables, Player, is_legal};
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};
use cham_search::vbr::RiverVbr;

const CFG: EngineConfig = EngineConfig { start_stack: 10_000, sb: 50, bb: 100 };
const HERO_SEAT: usize = 1; // BB acts first postflop
const VILL_SEAT: usize = 0;

fn board_from_seed(seed: u64) -> [Card; 5] {
    let mut rng = rng_from_seed(seed);
    let mut deck: Vec<u8> = (0..52).collect();
    for i in (1..52).rev() {
        let j = (cham_core::rng::next_f64(&mut rng) * (i + 1) as f64) as usize;
        deck.swap(i, j);
    }
    [Card(deck[0]), Card(deck[1]), Card(deck[2]), Card(deck[3]), Card(deck[4])]
}

fn combos_disjoint(board: &[Card; 5]) -> Vec<[u8; 2]> {
    let mut used = [false; 52];
    for c in board { used[c.idx() as usize] = true; }
    let mut out = Vec::new();
    for a in 0..52u8 {
        if used[a as usize] { continue; }
        for b in (a + 1)..52u8 {
            if used[b as usize] { continue; }
            out.push([a, b]);
        }
    }
    out
}

/// Build a river state; hero is seat 1 (acts first), villain seat 0.
/// Returns (state, seq-to-reach-river) with the line recorded.
fn build_river(hero: [u8; 2], vill: [u8; 2], board: &[Card; 5], enc: &mut Encoder)
    -> Option<(State, ActionSeq)>
{
    // deck order [h0a, h1a, h0b, h1b, board...]: seat0=(h0a,h0b), seat1=(h1a,h1b)
    let prefix = [
        Card(vill[0]), Card(hero[0]), Card(vill[1]), Card(hero[1]),
        board[0], board[1], board[2], board[3], board[4],
    ];
    let mut st = State::new(CFG, Deck::with_prefix(&prefix)).ok()?;
    let mut seq = ActionSeq::default();
    let mut guard = 0;
    while st.street() != Street::River && !st.is_terminal() && guard < 30 {
        guard += 1;
        let p = st.to_act();
        let obs = Observables::view(&st, Player::from_usize(p));
        let a = if is_legal(&obs, Action::Check) { Action::Check } else { Action::Call };
        enc.record(&obs, Player::from_usize(p), a, &mut seq);
        if st.apply(a).is_err() { return None; }
    }
    if st.street() == Street::River { Some((st, seq)) } else { None }
}

fn bet_to(obs: &Observables<'_>, frac: f64) -> Action {
    let to = obs.current_bet + (frac * obs.pot as f64).round() as i64;
    let to = to.min(obs.max_raise_to).max(obs.min_raise_to);
    Action::Bet { to }
}

fn norm4(mut v: Vec<f64>) -> Vec<f64> {
    let t: f64 = v.iter().sum();
    if t > 1e-12 { for x in v.iter_mut() { *x /= t; } }
    while v.len() < 4 { v.push(0.0); }
    v.truncate(4);
    v
}

fn path_idx(p: &str) -> usize {
    match p { "c" => 0, "b0" => 1, "b1" => 2, "j" => 3, _ => 0 }
}

#[test]
#[ignore = "D1 full river VBR; needs shipped bundle"]
fn d1_vbr_full() {
    let bp_dir = std::env::var("CHAM_D1_BP")
        .unwrap_or_else(|_| "artifacts/agent-honest-19dim/robust".into());
    let policy = BlueprintPolicy::load(std::path::Path::new(&bp_dir), 0).expect("load bp");
    // Buckets/config: explicit env, else the bundle layout (bp_dir/..).
    let bundle = std::path::Path::new(&bp_dir).parent().expect("bundle");
    let cfg_path = std::env::var("CHAM_D1_CONFIG").ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| bundle.join("abstraction.toml"));
    let cfg = std::fs::read_to_string(&cfg_path).ok()
        .and_then(|t| cham_engine::config::parse_config(&t).ok())
        .unwrap_or_else(AbstractionConfig::tiny);
    let bk = std::env::var("CHAM_D1_BUCKETS").ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| bundle.join("buckets"));
    let base_enc = Encoder::from_artifacts_dir(&bk, cfg.clone())
        .unwrap_or_else(|_| Encoder::cfg_only(cfg.clone()).expect("enc"));

    let fracs = [0.5f64, 1.0];
    let n_boards = 20u64;
    let n_combos = 60usize;
    let mut values: Vec<f64> = Vec::new();

    for bseed in 0..n_boards {
        let board = board_from_seed(0xB0 + bseed);
        let all = combos_disjoint(&board);
        if all.len() < n_combos { continue; }
        let stride = all.len() / n_combos;
        let hero: Vec<[u8; 2]> = (0..n_combos).map(|k| all[k * stride]).collect();
        let vill: Vec<[u8; 2]> = (0..n_combos).map(|k| all[(k * stride + stride / 2).min(all.len() - 1)]).collect();

        let hr: Vec<u32> = hero.iter().map(|c|
            (cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &board) * 1e6) as u32
        ).collect();
        let vr: Vec<u32> = vill.iter().map(|c|
            (cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &board) * 1e6) as u32
        ).collect();
        let hw = vec![1.0f64 / hero.len() as f64; hero.len()];
        let vw = vec![1.0f64 / vill.len() as f64; vill.len()];

        // villain policy table[path][villain_combo]
        let mut vtable = vec![vec![vec![0.25f64; 4]; vill.len()]; 4];
        let mut hero_dummy = [0u8; 2];
        for j in 0..vill.len() {
            // dummy hero disjoint from board + villain
            let mut used = [false; 52];
            for c in &board { used[c.idx() as usize] = true; }
            used[vill[j][0] as usize] = true;
            used[vill[j][1] as usize] = true;
            let mut d = Vec::new();
            for x in 0..52u8 { if !used[x as usize] { d.push(x); if d.len()==2 { break; } } }
            hero_dummy = [d[0], d[1]];

            if let Some((st0, seq0)) = build_river(hero_dummy, vill[j], &board, &mut base_enc.clone()) {
                for pi in 0..4usize {
                    let mut st = st0;
                    let mut seq = seq0;
                    let mut enc = base_enc.clone();
                    if pi > 0 {
                        // hero (seat 1) acts: bet size pi-1, or jam for pi==3
                        let p = st.to_act();
                        let obs = Observables::view(&st, Player::from_usize(p));
                        let a = if pi == 3 {
                            Action::Bet { to: obs.max_raise_to }
                        } else {
                            bet_to(&obs, fracs[pi - 1])
                        };
                        enc.record(&obs, Player::from_usize(p), a, &mut seq);
                        if st.apply(a).is_err() { continue; }
                    }
                    // villain (seat 0) to act now
                    let p = st.to_act();
                    if p != VILL_SEAT { continue; }
                    let obs = Observables::view(&st, Player::from_usize(p));
                    let mut e = base_enc.clone();
                    if let Some(s) = policy.strategy(&obs, &mut e, &seq) {
                        vtable[pi][j] = norm4(s);
                    }
                }
            }
        }

        let table = vtable.clone();
        let mut pol = |path: &str, j: usize| -> Vec<f64> {
            table[path_idx(path)].get(j).cloned().unwrap_or_else(|| vec![0.25; 4])
        };
        let pot_bb = 2.0; // call/check to the river -> 2 bb pot
        let mut vbr = RiverVbr {
            hero: &hero, hero_rank: &hr, hero_w: &hw,
            vill: &vill, vill_rank: &vr, vill_w: &vw,
            pot: pot_bb, stack: 98.0, bet_fracs: &fracs, policy: &mut pol,
        };
        values.push(vbr.best_response());
    }

    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n.max(1.0);
    let var = (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n.max(1.0)).max(0.0);
    let se = (var / n.max(1.0)).sqrt();
    eprintln!("\n=== D1 river VBR (full harness) ===");
    eprintln!("  boards: {}", values.len());
    eprintln!("  mean VBR: {mean:.3} +/- {se:.3} bb/hand");
    eprintln!("  (positive = a perfect river player beats the blueprint by this much)");
}
