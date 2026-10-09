//! W3 gate, 40-board sweep, per-board ranges (disjoint half-decks).
//!
//! The earlier 20-board sweep used fixed ranges and skipped 13 of 20
//! boards on collision. This one draws a fresh 3-combo range per side
//! from cards disjoint from the board, guaranteeing ~40/40 survivors.
//!
//! Reports the one-sided gate per board (agent's own exploitability)
//! and the mean ± SE.

use cham_blueprint::policy::BlueprintPolicy;
use cham_core::card::{Card, Deck, Hand2};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State, Street};
use cham_core::obs::{Observables, Player, is_legal};
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};
use cham_engine::ladder::ActionLadder;
use cham_search::pubtree::PublicTree;
use cham_search::river_cfr::{RiverCfr, SolvedRiver, build_blueprint_strategy_table};

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};

fn board_from_seed(seed: u64) -> [Card; 5] {
    let mut rng = rng_from_seed(seed);
    let mut deck: Vec<u8> = (0..52).collect();
    for i in (1..52).rev() {
        let j = (cham_core::rng::next_f64(&mut rng) * (i + 1) as f64) as usize;
        deck.swap(i, j);
    }
    [
        Card(deck[0]),
        Card(deck[1]),
        Card(deck[2]),
        Card(deck[3]),
        Card(deck[4]),
    ]
}

/// Draw `n` disjoint combos from `pool` after excluding cards in `used`.
fn draw_range(pool: &[u8], used: &mut [bool; 52], n: usize) -> Vec<[u8; 2]> {
    // Draw `n` combos from `pool`, skipping any card that overlaps the
    // BOARD (`used`). The drawn combos MAY share cards with each other —
    // real ranges do. The prior version marked each drawn card as used,
    // which forced disjoint combos and capped the draw at floor(pool/2);
    // that made 15/side impossible and the sweep reported 0 boards.
    let avail: Vec<u8> = pool
        .iter()
        .copied()
        .filter(|c| !used[*c as usize])
        .collect();
    let mut out = Vec::with_capacity(n);
    'outer: for i in 0..avail.len() {
        for j in (i + 1)..avail.len() {
            out.push([avail[i], avail[j]]);
            if out.len() == n {
                break 'outer;
            }
        }
    }
    out
}

fn river_state_and_seq(ladder: &ActionLadder, board: &[Card; 5]) -> (State, ActionSeq) {
    let mut used = [false; 52];
    for c in board {
        used[c.idx() as usize] = true;
    }
    let mut dummies = [0u8; 4];
    let mut k = 0usize;
    for c in 0..52u8 {
        if !used[c as usize] {
            dummies[k] = c;
            k += 1;
            if k == 4 {
                break;
            }
        }
    }
    let prefix = [
        Card(dummies[0]),
        Card(dummies[1]),
        Card(dummies[2]),
        Card(dummies[3]),
        board[0],
        board[1],
        board[2],
        board[3],
        board[4],
    ];
    let mut st = State::new(CFG, Deck::with_prefix(&prefix)).expect("state");
    let mut seq = ActionSeq::default();
    let mut guard = 0;
    while st.street() != Street::River && !st.is_terminal() && guard < 30 {
        guard += 1;
        let p = st.to_act();
        let obs = Observables::view(&st, Player::from_usize(p));
        let a = if is_legal(&obs, Action::Check) {
            Action::Check
        } else {
            Action::Call
        };
        cham_engine::ladder::record_action(ladder, &obs, Player::from_usize(p), a, &mut seq);
        st.apply(a).expect("apply");
    }
    assert_eq!(st.street(), Street::River);
    (st, seq)
}

fn stats(vs: &[f64]) -> (f64, f64) {
    let n = vs.len() as f64;
    let m = vs.iter().sum::<f64>() / n;
    let var = vs.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
    (m, (var / n).sqrt())
}

#[test]
#[ignore = "integration sweep; needs shipped bundle"]
fn w3_gate_sweep_40_boards() {
    let bp_dir = std::env::var("CHAM_D1_BP")
        .unwrap_or_else(|_| "artifacts/agent-honest-19dim/robust".into());
    let robust = BlueprintPolicy::load(std::path::Path::new(&bp_dir), 0).expect("load bp");

    let bundle = std::path::Path::new(&bp_dir).parent().expect("bundle");
    let bk = std::env::var("CHAM_D1_BUCKETS")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| bundle.join("buckets"));
    let cfg_path = bundle.join("abstraction.toml");
    let cfg = std::fs::read_to_string(&cfg_path)
        .ok()
        .and_then(|t| cham_engine::config::parse_config(&t).ok())
        .unwrap_or_else(AbstractionConfig::tiny);
    let encoder = Encoder::from_artifacts_dir(&bk, cfg.clone())
        .unwrap_or_else(|_| Encoder::cfg_only(cfg.clone()).expect("cfg_only"));

    let n_boards: u64 = std::env::var("CHAM_W3_BOARDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(40);

    let ladder = ActionLadder::new(&cfg);
    let hero_seat = 1;
    let half: Vec<u8> = (0..26).collect();
    let other: Vec<u8> = (26..52).collect();

    let mut bp_agents: Vec<f64> = Vec::new();
    let mut res_agents: Vec<f64> = Vec::new();

    for bseed in 0..n_boards {
        let board = board_from_seed(0x5AE3_u64 + bseed);
        let mut used = [false; 52];
        for c in &board {
            used[c.idx() as usize] = true;
        }
        let n_side: usize = std::env::var("CHAM_SWEEP_COMBOS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3);
        let hero = draw_range(&half, &mut used, n_side);
        let villain = draw_range(&other, &mut used, n_side);
        if hero.len() < n_side || villain.len() < n_side {
            continue;
        }

        let (st, seq) = river_state_and_seq(&ladder, &board);
        let tree = PublicTree::build_from_state(st, seq, &ladder, 200_000);

        let rank = |c: &[u8; 2]| -> u32 {
            (cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &board) * 1e6)
                as u32
        };
        let hero_rank: Vec<u32> = hero.iter().map(rank).collect();
        let villain_rank: Vec<u32> = villain.iter().map(rank).collect();

        let bp_policy = |o: &Observables<'_>, s: &ActionSeq| -> Option<Vec<f64>> {
            let mut enc = encoder.clone();
            robust.strategy(o, &mut enc, s)
        };
        let (bp_hero, bp_villain) = build_blueprint_strategy_table(
            &tree, st, seq, &ladder, &hero, &villain, hero_seat, bp_policy,
        );

        let helper = RiverCfr::new(
            &tree,
            &hero,
            &hero_rank,
            &villain,
            &villain_rank,
            st,
            hero_seat,
            None,
        );
        let (_bp_h, bp_v) = helper.exploitability_split(&SolvedRiver {
            hero_strat: bp_hero.clone(),
            villain_strat: bp_villain.clone(),
            gadget_root_strat: None,
            iters: 0,
        });
        bp_agents.push(bp_v);

        let villain_cfv = helper.villain_cfv_under_strategy(&bp_hero, &bp_villain);
        let v_bp_hero: Vec<f64> = villain_cfv.iter().map(|v| -v).collect();

        let gadget_solver = RiverCfr::new(
            &tree,
            &hero,
            &hero_rank,
            &villain,
            &villain_rank,
            st,
            hero_seat,
            Some(v_bp_hero),
        );
        let solved = gadget_solver.solve(4000);
        let (_res_h, res_v) = gadget_solver.exploitability_split(&solved);
        res_agents.push(res_v);

        eprintln!(
            "  board {bseed:>2}: agent(bp)={bp_v:>8.2}  agent(res)={res_v:>8.2}  delta {:>+8.2} chips",
            res_v - bp_v
        );
        assert!(
            res_v <= bp_v + 1.0,
            "board {bseed}: agent exploitability rose {bp_v} -> {res_v}"
        );
    }

    assert!(
        bp_agents.len() >= 30,
        "only {} boards survived",
        bp_agents.len()
    );

    let (bp_m, bp_se) = stats(&bp_agents);
    let (res_m, res_se) = stats(&res_agents);
    let deltas: Vec<f64> = res_agents
        .iter()
        .zip(bp_agents.iter())
        .map(|(r, b)| r - b)
        .collect();
    let (d_m, d_se) = stats(&deltas);
    let z = if d_se > 0.0 { d_m / d_se } else { 0.0 };

    eprintln!();
    eprintln!("=== W3 one-sided gate, {} boards ===", bp_agents.len());
    eprintln!(
        "  agent-expl(bp)     = {:.3} +/- {:.3} bb",
        bp_m / 100.0,
        bp_se / 100.0
    );
    eprintln!(
        "  agent-expl(res)    = {:.3} +/- {:.3} bb",
        res_m / 100.0,
        res_se / 100.0
    );
    eprintln!(
        "  delta (res - bp)   = {:+.3} +/- {:.3} bb",
        d_m / 100.0,
        d_se / 100.0
    );
    eprintln!("  z = delta/SE       = {:+.2}", z);
    eprintln!();
}
