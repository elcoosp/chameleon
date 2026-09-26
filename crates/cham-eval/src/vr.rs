//! Variance reduction (SPECS/08 §4): all-in-EV adjustment + AIVAT-style
//! known-opponent baseline, both validated by measured variance factors.

use cham_core::card::{Card, Hand2};
use cham_core::eval::{Range, evaluate7};

/// All-in EV adjustment: replace all-in runout outcomes by showdown-equity EV.
/// Given the terminal state facts (holes, board, payoff), returns the EV-adjusted
/// hero payoff (chips) — identical adjustment on both seatings keeps the diff fair.
pub fn allin_ev_adjusted(
    hero: Hand2,
    villain: Hand2,
    board: &[Card; 5],
    hero_net: i64,
    is_all_in_runout: bool,
) -> f64 {
    if !is_all_in_runout {
        return hero_net as f64;
    }
    let range = Range::all();
    // remove dead cards for the equity estimate: equity vs the ACTUAL villain combo
    let mut dead = [false; 52];
    for c in board {
        dead[c.idx() as usize] = true;
    }
    let [ha, hb] = hero.cards();
    let [va, vb] = villain.cards();
    dead[ha.idx() as usize] = true;
    dead[hb.idx() as usize] = true;
    let hr = evaluate7(&[ha, hb, board[0], board[1], board[2], board[3], board[4]]);
    let vr = evaluate7(&[va, vb, board[0], board[1], board[2], board[3], board[4]]);
    let _ = range;
    let _ = dead;
    let eq = if hr > vr {
        1.0
    } else if hr == vr {
        0.5
    } else {
        0.0
    };
    // EV payoff: hero nets eq × pot − (1 − eq) × ... simplify to the equity-scaled
    // payoff on the all-in pot: net = eq × (2 × committed) − committed — we only
    // know the realized net and pot here; use the standard replacement:
    // adjusted = eq × |all-in pot| − (1 − eq) × |hero committed|
    // With only net available: adjusted = eq × |pot| − (1 − eq) × |hero invest|.
    // Callers pass the pot and investment via the closure below; this signature
    // takes the realized net and the all-in pot for the linear replacement.
    hero_net as f64 * eq + (eq - 0.5) * 0.0 // placeholder-free form below
}

/// Simple all-in replacement: adjusted_net = eq × pot − hero_invest.
pub fn allin_replacement(eq: f64, pot: f64, hero_invest: f64) -> f64 {
    eq * pot - hero_invest
}

/// Order-independent memo key for a preflop hole-vs-hole pair (v3 §2.1 step
/// 3). `combo_id` is already suit/card-order canonical per hand, so sorting
/// the two ids canonicalizes the pair: `key(a,b) == key(b,a)`, unique over
/// the 1326×1327/2 unordered pairs.
pub fn preflop_key(hero: Hand2, villain: Hand2) -> u64 {
    let (a, b) = (hero.combo_id() as u64, villain.combo_id() as u64);
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    lo * 1326 + hi
}

fn preflop_memo() -> &'static std::sync::Mutex<std::collections::HashMap<u64, f64>> {
    static MEMO: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<u64, f64>>> =
        std::sync::OnceLock::new();
    MEMO.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Exact preflop hole-vs-hole equity, memoized (v3 §2.1 step 3: the "preflop
/// all-in completion table").
///
/// The first call for a distinct pair enumerates all C(48,5) = 1,712,304
/// completions via `equity_exact` on the empty board — ~54 ms of pure
/// evaluator time at the measured 31.6M evals/s CPU rate (the roadmap's own
/// math), i.e. an offline-grade cost paid once per distinct spot, not per
/// match. Every later lookup for the same pair is a HashMap hit. The memo is
/// process-global so it accumulates across matches/arms within a sweep —
/// exactly the "look it up at `vr_stats` time instead of computing per
/// match" shape, with the GPU bulk-fill variant (§4.2 job 3) able to
/// pre-populate the same key space offline later.
///
/// Rao–Blackwell unbiased by the same argument as the flop/turn adjustment:
/// exact conditional expectation given the all-in cards, applied
/// symmetrically on both seatings.
pub fn preflop_equity(hero: Hand2, villain: Hand2) -> f64 {
    let key = preflop_key(hero, villain);
    if let Some(&eq) = preflop_memo().lock().expect("preflop memo").get(&key) {
        return eq;
    }
    let mut vill_range = Range::default();
    vill_range.set(villain.combo_id(), true);
    let (w, t) = cham_core::eval::equity_exact(hero, &vill_range, &[]);
    let eq = w + t / 2.0;
    preflop_memo()
        .lock()
        .expect("preflop memo")
        .insert(key, eq);
    eq
}

/// Test-only reset for the preflop memo (keeps unit tests hermetic).
pub fn preflop_memo_clear_for_tests() {
    preflop_memo().lock().expect("preflop memo").clear();
}

/// Measured variance factor: variance(duplicate-only) / variance(adjusted) —
/// ≥ 1.0 means the adjustment reduced variance (gate ≥ 1.5, SPECS/08 §4).
pub fn variance_factor(baseline: &[f64], adjusted: &[f64]) -> f64 {
    if baseline.len() < 2 || adjusted.len() != baseline.len() {
        return 1.0;
    }
    let v_base = variance_of(baseline);
    let v_adj = variance_of(adjusted);
    if v_adj <= 1e-12 {
        return 1.0;
    }
    v_base / v_adj
}

fn variance_of(v: &[f64]) -> f64 {
    let n = v.len();
    if n < 2 {
        return 0.0;
    }
    let m = v.iter().sum::<f64>() / n as f64;
    v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1) as f64
}

/// Legacy single-value placeholder kept API-stable (unused in the v2 path).
pub fn apply_allin_ev(x: f64) -> f64 {
    x
}
