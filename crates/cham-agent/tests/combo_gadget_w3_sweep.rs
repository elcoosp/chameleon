//! W3 gate, 20-board sweep. Reports mean ± SE for VBR(blueprint) and
//! VBR(resolved with gadget) on the shipped bundle.
//!
//! Same setup as `combo_gadget_w3_shipped.rs`, extended from one board
//! to many, with standard errors.
//!
//! Run with:
//!
//!     cargo test -p cham-agent --test combo_gadget_w3_sweep --no-run
//!     env -i PATH="$PATH" HOME="$HOME" \
//!       CHAM_D1_BP="$PWD/artifacts/agent-honest-19dim/robust" \
//!       CHAM_D1_BUCKETS="$PWD/artifacts/agent-honest-19dim/buckets" \
//!       CHAM_W3_BOARDS=20 \
//!       target/debug/deps/combo_gadget_w3_sweep-<hash> --ignored --nocapture

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

fn river_state_and_seq(ladder: &ActionLadder, board: &[Card; 5]) -> (State, ActionSeq) {
    // Dummy hole cards DISJOINT from the board — a fixed `Card(2)..Card(5)`
    // prefix collides whenever the board contains one of those (same bug
    // `fullgame.rs` had before b8d8ad4).
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
fn w3_gate_sweep_20_boards() {
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
        .unwrap_or(20);

    let ladder = ActionLadder::new(&cfg);
    let hero_seat = 1;

    let mut bp_vals: Vec<f64> = Vec::new();
    let mut res_vals: Vec<f64> = Vec::new();

    for bseed in 0..n_boards {
        let board = board_from_seed(0x5AE3_u64 + bseed);
        let (st, seq) = river_state_and_seq(&ladder, &board);
        let tree = PublicTree::build_from_state(st, seq, &ladder, 200_000);

        // Ranges fixed across boards, but ensure they don't collide with
        // this board. If collision, skip that board.
        let mut used = [false; 52];
        for c in &board {
            used[c.idx() as usize] = true;
        }
        let cands_hero: [[u8; 2]; 3] = [[10, 11], [12, 13], [14, 15]];
        let cands_vill: [[u8; 2]; 3] = [[16, 17], [18, 19], [20, 21]];
        if cands_hero
            .iter()
            .any(|c| used[c[0] as usize] || used[c[1] as usize])
            || cands_vill
                .iter()
                .any(|c| used[c[0] as usize] || used[c[1] as usize])
        {
            continue;
        }
        let hero: Vec<[u8; 2]> = cands_hero.to_vec();
        let villain: Vec<[u8; 2]> = cands_vill.to_vec();

        let rank = |c: &[u8; 2]| -> u32 {
            (cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &board) * 1e6)
                as u32
        };
        let hero_rank: Vec<u32> = hero.iter().map(rank).collect();
        let villain_rank: Vec<u32> = villain.iter().map(rank).collect();

        // Blueprint table.
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
        let (bp_hero_br, bp_villain_br) = helper.exploitability_split(&SolvedRiver {
            hero_strat: bp_hero.clone(),
            villain_strat: bp_villain.clone(),
            gadget_root_strat: None,
            iters: 0,
        });
        let vbr_bp = bp_hero_br + bp_villain_br;
        bp_vals.push(vbr_bp);
        // One-sided W3 gate: the OPPONENT's best response against the
        // AGENT's strategy must not rise. Hero is the agent here, so that
        // quantity is `bp_villain_br` (villain's BR value against hero's
        // strategy). The gadget bounds this; the two-seat sum is not the
        // plan's gate (see W3-METRIC-CORRECTION-2026-10-09.md).
        let bp_agent_expl = bp_villain_br;

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
        let (res_hero_br, res_villain_br) = gadget_solver.exploitability_split(&solved);
        let vbr_res = res_hero_br + res_villain_br;
        res_vals.push(vbr_res);
        let res_agent_expl = res_villain_br;

        eprintln!("  board {bseed}: VBR(bp)={vbr_bp:>8.2}  VBR(res)={vbr_res:>8.2}  chips",);
        eprintln!(
            "           agent-expl(bp)={bp_agent_expl:>8.2}  agent-expl(res)={res_agent_expl:>8.2}               delta {:>+8.2}",
            res_agent_expl - bp_agent_expl,
        );
        assert!(
            res_agent_expl <= bp_agent_expl + 1.0,
            "board {bseed}: agent exploitability rose {} -> {} chips",
            bp_agent_expl,
            res_agent_expl,
        );
    }

    assert!(bp_vals.len() >= 3, "not enough boards survived");

    let (bp_m, bp_se) = stats(&bp_vals);
    let (res_m, res_se) = stats(&res_vals);
    let (bp_m_bb, bp_se_bb) = (bp_m / 100.0, bp_se / 100.0);
    let (res_m_bb, res_se_bb) = (res_m / 100.0, res_se / 100.0);
    let delta = res_m - bp_m;
    let delta_se = (bp_se * bp_se + res_se * res_se).sqrt();
    let delta_bb = delta / 100.0;

    eprintln!();
    eprintln!("=== W3 gate, {} boards ===", bp_vals.len());
    eprintln!("  VBR(shipped blueprint): {bp_m_bb:.4} +/- {bp_se_bb:.4} bb");
    eprintln!("  VBR(resolved, gadget) : {res_m_bb:.4} +/- {res_se_bb:.4} bb");
    eprintln!(
        "  delta (res - bp)      : {delta_bb:+.4} +/- {:.4} bb",
        delta_se / 100.0
    );
    let z = if delta_se > 0.0 {
        delta / delta_se
    } else {
        0.0
    };
    eprintln!("  z = delta/SE(delta)   : {z:+.2}");
    eprintln!();
    eprintln!("  NOTE: the two-seat sum is reported for context. The W3 gate");
    eprintln!("  is asserted per-board above, one-sidedly (the agent's own");
    eprintln!("  exploitability does not rise).");

    let _ = (res_m_bb, bp_m_bb, delta_bb, delta_se);
}
