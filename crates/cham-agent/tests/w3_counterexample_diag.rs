//! Diagnostic: reproduce the two W3 sweep counterexamples (board seeds
//! 0x5AE3+13 and +15) and dump the numbers the gadget bound depends on.
//!
//! Not a pass/fail test — reports:
//!   VBR(blueprint), VBR(resolved)
//!   v_bp_hero vector (the gadget's terminate payment per villain combo)
//!   the blueprint's root strategy per combo
//!   how many rows of the blueprint table are uniform (bucket misses)
//!
//! Run with:
//!
//!     env -i PATH="$PATH" HOME="$HOME" \
//!       CHAM_D1_BP="$PWD/artifacts/agent-honest-19dim/robust" \
//!       CHAM_D1_BUCKETS="$PWD/artifacts/agent-honest-19dim/buckets" \
//!       target/debug/deps/w3_counterexample_diag-<hash> --ignored --nocapture

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

fn count_uniform_rows(tab: &[Vec<Vec<f64>>]) -> (usize, usize) {
    let mut uniform = 0usize;
    let mut total = 0usize;
    for node_rows in tab {
        for row in node_rows {
            total += 1;
            if row.is_empty() {
                continue;
            }
            let first = row[0];
            if row.iter().all(|p| (p - first).abs() < 1e-9) {
                uniform += 1;
            }
        }
    }
    (uniform, total)
}

#[test]
#[ignore = "diagnostic; needs shipped bundle"]
fn dump_counterexamples() {
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

    let ladder = ActionLadder::new(&cfg);
    let hero_seat = 1;

    for bseed in [13u64, 15u64] {
        let seed = 0x5AE3_u64 + bseed;
        let board = board_from_seed(seed);
        let (st, seq) = river_state_and_seq(&ladder, &board);
        let tree = PublicTree::build_from_state(st, seq, &ladder, 200_000);

        let mut used = [false; 52];
        for c in &board {
            used[c.idx() as usize] = true;
        }
        let hero: Vec<[u8; 2]> = vec![[10, 11], [12, 13], [14, 15]];
        let villain: Vec<[u8; 2]> = vec![[16, 17], [18, 19], [20, 21]];
        if hero
            .iter()
            .any(|c| used[c[0] as usize] || used[c[1] as usize])
            || villain
                .iter()
                .any(|c| used[c[0] as usize] || used[c[1] as usize])
        {
            eprintln!("board {bseed}: SKIPPED (collision)");
            continue;
        }

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
        let vbr_bp = helper.exploitability(&SolvedRiver {
            hero_strat: bp_hero.clone(),
            villain_strat: bp_villain.clone(),
            gadget_root_strat: None,
            iters: 0,
        });

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
            Some(v_bp_hero.clone()),
        );
        let solved = gadget_solver.solve(4000);
        let vbr_res = gadget_solver.exploitability(&solved);

        let (hu, ht) = count_uniform_rows(&bp_hero);
        let (vu, vt) = count_uniform_rows(&bp_villain);

        eprintln!();
        eprintln!("=== board {bseed} (seed {seed:#x}) ===");
        eprintln!(
            "  board: {:?}",
            board.iter().map(|c| c.idx()).collect::<Vec<_>>()
        );
        eprintln!("  hero combos: {:?}", hero);
        eprintln!("  villain combos: {:?}", villain);
        eprintln!("  hero ranks: {:?}", hero_rank);
        eprintln!("  villain ranks: {:?}", villain_rank);
        eprintln!("  VBR(bp)  = {vbr_bp:>10.3} chips");
        eprintln!("  VBR(res) = {vbr_res:>10.3} chips");
        eprintln!("  delta    = {:>+10.3} chips", vbr_res - vbr_bp);
        eprintln!(
            "  v_bp_hero (per villain combo): {:?}",
            v_bp_hero
                .iter()
                .map(|v| (v * 100.0).round() / 100.0)
                .collect::<Vec<_>>()
        );
        eprintln!("  blueprint uniform rows: hero {hu}/{ht}, villain {vu}/{vt}");
        eprintln!("  root node index: {}", tree.root);
        let root_idx = tree.root as usize;
        if !bp_hero[root_idx].is_empty() {
            eprintln!("  bp_hero[root] = {:?}", bp_hero[root_idx]);
        }
        if !bp_villain[root_idx].is_empty() {
            eprintln!("  bp_villain[root] = {:?}", bp_villain[root_idx]);
        }
    }
}
