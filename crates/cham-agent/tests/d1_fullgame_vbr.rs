//! Decision D1: full-game VBR against the shipped blueprint.
//!
//! Loads `BlueprintPolicy` from `CHAM_D1_BP` (default
//! artifacts/agent-honest-19dim/robust), builds the encoder from
//! `CHAM_D1_BUCKETS` (default <bundle>/buckets), and runs the
//! full-game VBR over `CHAM_D1_BOARDS` (default 20) sampled boards.
//!
//! Run (note: NO CHAM_SLOT_BUCKET — the shipped bundle was trained
//! with stack-fraction keys, so setting that flag corrupts every
//! non-root seq entry and produces a 99% policy miss; verified by
//! d1_key_probe at 69af8b9):
//!   CHAM_D1_BP=$PWD/artifacts/agent-honest-19dim/robust \
//!   CHAM_D1_CONFIG=$PWD/artifacts/agent-honest-19dim/abstraction.toml \
//!   CHAM_D1_BUCKETS=$PWD/artifacts/agent-honest-19dim/buckets \
//!     target/debug/deps/d1_fullgame_vbr-<hash> --ignored --nocapture

use cham_blueprint::policy::BlueprintPolicy;
use cham_core::card::{Card, Deck, Hand2};
use cham_core::engine::config::EngineConfig;
use cham_core::engine::{Action, State};
use cham_core::obs::{Observables, Player};
use cham_core::rng::rng_from_seed;
use cham_engine::config::AbstractionConfig;
use cham_engine::encoder::{ActionSeq, Encoder};
use cham_engine::ladder::ActionLadder;
use cham_search::fullgame::FullGameVbr;
use cham_search::pubtree::PublicTree;

const CFG: EngineConfig = EngineConfig {
    start_stack: 10_000,
    sb: 50,
    bb: 100,
};
const HERO_SEAT: usize = 1;
const VILL_SEAT: usize = 0;

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

fn split_ranges(board: &[Card; 5], n: usize) -> (Vec<[u8; 2]>, Vec<[u8; 2]>) {
    // Seeded shuffle of ALL combos in each half-deck, then take the
    // first n. Lexicographic enumeration (the prior version) produced
    // ranges where 25 of 30 combos shared a card — see
    // docs/plans/D1-HARNESS-RANGE-FINDING-2026-10-09.md.
    use cham_core::rng::{next_f64, rng_from_seed};

    let mut avail: Vec<u8> = Vec::new();
    for c in 0..52u8 {
        if !board.iter().any(|b| b.idx() as usize == c as usize) {
            avail.push(c);
        }
    }
    let half = avail.len() / 2;

    fn all_combos(pool: &[u8]) -> Vec<[u8; 2]> {
        let mut out = Vec::with_capacity(pool.len() * (pool.len() - 1) / 2);
        for i in 0..pool.len() {
            for j in (i + 1)..pool.len() {
                out.push([pool[i], pool[j]]);
            }
        }
        out
    }
    fn shuffled(mut v: Vec<[u8; 2]>, seed: u64) -> Vec<[u8; 2]> {
        let mut rng = rng_from_seed(seed);
        for i in (1..v.len()).rev() {
            let j = (next_f64(&mut rng) * (i + 1) as f64) as usize;
            v.swap(i, j);
        }
        v
    }

    let hero_all = all_combos(&avail[..half]);
    let vill_all = all_combos(&avail[half..]);
    let hero = shuffled(hero_all, 0xD1_2026_1009);
    let vill = shuffled(vill_all, 0xD1_2026_1009 ^ 0x5A5A);
    (
        hero.into_iter().take(n).collect(),
        vill.into_iter().take(n).collect(),
    )
}

fn norm_na(mut v: Vec<f64>, na: usize) -> Vec<f64> {
    v.resize(na, 0.0);
    v.truncate(na);
    let s: f64 = v.iter().sum();
    if s > 1e-12 {
        for x in v.iter_mut() {
            *x /= s;
        }
    } else {
        for x in v.iter_mut() {
            *x = 1.0 / na as f64;
        }
    }
    v
}

#[test]
#[ignore = "D1 full-game VBR; needs shipped bundle"]
fn d1_fullgame_vbr() {
    let bp_dir = std::env::var("CHAM_D1_BP")
        .unwrap_or_else(|_| "artifacts/agent-honest-19dim/robust".into());
    let policy = BlueprintPolicy::load(std::path::Path::new(&bp_dir), 0).expect("load bp");

    let bundle = std::path::Path::new(&bp_dir).parent().expect("bundle");
    let cfg_path = std::env::var("CHAM_D1_CONFIG")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| bundle.join("abstraction.toml"));
    let cfg = std::fs::read_to_string(&cfg_path)
        .ok()
        .and_then(|t| cham_engine::config::parse_config(&t).ok())
        .unwrap_or_else(AbstractionConfig::tiny);
    let bk = std::env::var("CHAM_D1_BUCKETS")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| bundle.join("buckets"));
    let base_enc = Encoder::from_artifacts_dir(&bk, cfg.clone())
        .unwrap_or_else(|_| Encoder::cfg_only(cfg.clone()).expect("enc"));

    let n_boards: u64 = std::env::var("CHAM_D1_BOARDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    let n_combos: usize = std::env::var("CHAM_D1_COMBOS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);

    let ladder = ActionLadder::new(&cfg);
    let tree = PublicTree::build(CFG, &ladder, 1_000_000);

    let mut values: Vec<f64> = Vec::new();
    let mut misses = 0usize;
    let mut queries = 0usize;

    for bseed in 0..n_boards {
        let board = board_from_seed(0xB000 + bseed);
        let (hero, vill) = split_ranges(&board, n_combos);
        if hero.len() < n_combos || vill.len() < n_combos {
            continue;
        }

        let rank = |c: &[u8; 2]| -> u32 {
            (cham_engine::tables::river_equity(Hand2::new(Card(c[0]), Card(c[1])), &board) * 1e6)
                as u32
        };
        let hero_rank: Vec<u32> = hero.iter().map(rank).collect();
        let vill_rank: Vec<u32> = vill.iter().map(rank).collect();
        let hw = vec![1.0 / hero.len() as f64; hero.len()];
        let vw = vec![1.0 / vill.len() as f64; vill.len()];

        // Villain callback: rebuild a state with villain's combo-specific
        // holes, replay `path`, then query the blueprint from VILL_SEAT.
        let mut loc_misses = 0usize;
        let mut loc_q = 0usize;
        let mut cb =
            |_st: &State, path: &[Action], _seq: &ActionSeq, na: usize, combo: usize| -> Vec<f64> {
                loc_q += 1;
                let vh = vill[combo];
                // pick dummy hero holes disjoint from board and vh
                let mut used = [false; 52];
                for c in &board {
                    used[c.idx() as usize] = true;
                }
                used[vh[0] as usize] = true;
                used[vh[1] as usize] = true;
                let mut d = [0u8; 2];
                let mut k = 0usize;
                for c in 0..52u8 {
                    if !used[c as usize] {
                        d[k] = c;
                        k += 1;
                        if k == 2 {
                            break;
                        }
                    }
                }
                if k < 2 {
                    loc_misses += 1;
                    return norm_na(vec![], na);
                }
                let prefix = [
                    Card(vh[0]),
                    Card(d[0]),
                    Card(vh[1]),
                    Card(d[1]),
                    board[0],
                    board[1],
                    board[2],
                    board[3],
                    board[4],
                ];
                let mut st = match State::new(CFG, Deck::with_prefix(&prefix)) {
                    Ok(s) => s,
                    Err(_) => {
                        loc_misses += 1;
                        return norm_na(vec![], na);
                    }
                };
                let mut seq = ActionSeq::default();
                let mut enc = base_enc.clone();
                let mut ok = true;
                for a in path {
                    let p = st.to_act();
                    let obs = Observables::view(&st, Player::from_usize(p));
                    enc.record(&obs, Player::from_usize(p), *a, &mut seq);
                    if st.apply(*a).is_err() {
                        ok = false;
                        break;
                    }
                }
                if !ok {
                    loc_misses += 1;
                    return norm_na(vec![], na);
                }
                let obs = Observables::view(&st, Player::from_usize(VILL_SEAT));
                match policy.strategy(&obs, &mut enc, &seq) {
                    Some(s) => norm_na(s, na),
                    None => {
                        loc_misses += 1;
                        norm_na(vec![], na)
                    }
                }
            };

        let mut vbr = FullGameVbr {
            tree: &tree,
            ladder: &ladder,
            hero_range: &hero,
            hero_rank: &hero_rank,
            hero_w: &hw,
            villain_range: &vill,
            villain_rank: &vill_rank,
            villain_w: &vw,
            cfg: CFG,
            hero_seat: HERO_SEAT,
            policy: &mut cb,
        };
        if let Some(v) = vbr.best_response(&board) {
            values.push(v);
        }
        misses += loc_misses;
        queries += loc_q;
        eprintln!(
            "board {:>2}: VBR = {:>8.4} bb  (queries={}, misses={})",
            bseed,
            values.last().copied().unwrap_or(f64::NAN),
            loc_q,
            loc_misses
        );
    }

    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n.max(1.0);
    let var =
        (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0)).max(0.0);
    let se = (var / n.max(1.0)).sqrt();
    let miss_pct = if queries > 0 {
        100.0 * misses as f64 / queries as f64
    } else {
        0.0
    };

    eprintln!();
    eprintln!("=== D1 full-game VBR vs blueprint ===");
    eprintln!("  blueprint:   {}", bp_dir);
    eprintln!("  boards:      {}", values.len());
    eprintln!("  combos:      {}h / {}v", n_combos, n_combos);
    eprintln!("  policy miss: {:.2}%  ({}/{})", miss_pct, misses, queries);
    eprintln!("  mean VBR:    {:.4} +/- {:.4} bb/hand", mean, se);
    eprintln!("  (positive = perfect full-game player beats the blueprint by this much)");
    eprintln!();
}

#[test]
fn split_ranges_is_spread() {
    // Regression: the lexicographic construction gave 25 of 30 combos
    // the same shared card. The shuffled one should not.
    let board = [Card(40), Card(41), Card(42), Card(43), Card(44)];
    let (hero, vill) = split_ranges(&board, 30);
    assert_eq!(hero.len(), 30);
    assert_eq!(vill.len(), 30);

    fn max_single_card_share(range: &[[u8; 2]]) -> usize {
        let mut counts = [0usize; 52];
        for c in range {
            counts[c[0] as usize] += 1;
            counts[c[1] as usize] += 1;
        }
        counts.iter().copied().max().unwrap_or(0)
    }

    let hero_max = max_single_card_share(&hero);
    let vill_max = max_single_card_share(&vill);
    // 30 random combos from 26 cards: no card should appear in more
    // than half. The old code gave 25.
    assert!(
        hero_max < 15,
        "hero range still concentrated: max card share = {hero_max}"
    );
    assert!(
        vill_max < 15,
        "villain range still concentrated: max card share = {vill_max}"
    );
}
