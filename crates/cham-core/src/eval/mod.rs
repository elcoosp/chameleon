//! Hand evaluation (SPECS/01 §3): a fast, allocation-free 7-card evaluator with an
//! honest P1 gate (≥ 100M evals/s on the M1; measured by `benches/eval.rs`).
//!
//! Design (in-crate bitmask evaluator — the spec's fallback path, used as primary
//! since `holdem-hand-evaluator` could not be gate-verified off-M1):
//! - flush path: per-suit 13-bit rank bitsets; straight-flush via an 8192-entry
//!   straight table, else top-5 flush ranks.
//! - non-flush path: 7-rank multiset → prime product → precomputed best-value map.
//! - packed 5-card values map to a dense 1..=7462 scale (Cactus-Kev convention,
//!   1 = weakest, 7462 = royal flush), generated at first use and asserted to be
//!   exactly 7462 classes.
//!
//! `equity_mc` exists ONLY for offline table building (SPECS/02 §3) — the encode
//! path and live decisions never sample.

use std::sync::OnceLock;

use rand::Rng as _;

use crate::card::{ALL_CARDS, Card, Hand2};
use crate::rng::Rng;

mod range;
pub use range::Range;

/// Category codes (higher beats lower).
const CAT_HIGH: u32 = 0;
const CAT_PAIR: u32 = 1;
const CAT_TWO_PAIR: u32 = 2;
const CAT_TRIPS: u32 = 3;
const CAT_STRAIGHT: u32 = 4;
const CAT_FLUSH: u32 = 5;
const CAT_FULL_HOUSE: u32 = 6;
const CAT_QUADS: u32 = 7;
const CAT_STRAIGHT_FLUSH: u32 = 8;

/// primes per rank for the multiset product key
const PRIMES: [u64; 13] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41];

const W5: u32 = 15; // base for kicker packing (13 ranks fit)

#[inline]
fn pack(cat: u32, ks: &[u8]) -> u32 {
    let mut v = cat * W5.pow(5);
    let mut mul = W5.pow(4);
    for (i, &k) in ks.iter().take(5).enumerate() {
        v += (k as u32) * mul;
        if i < 4 {
            mul /= W5;
        }
    }
    v
}

/// straight[13-bit mask] → top rank index of the best straight, or 0xff.
/// A wheel (A2345) yields 3 (rank index of the 5-high top card).
fn build_straight_table() -> [u8; 8192] {
    let mut t = [0xffu8; 8192];
    let wheel: u16 = (1 << 12) | 0b1111;
    for mask in 0..8192u16 {
        let mut best: u8 = 0xff;
        for h in (4..=12).rev() {
            let need = ((1u16 << (h + 1)) - 1) & !((1u16 << (h - 4)) - 1);
            if mask & need == need {
                best = h as u8;
                break; // iterating high→low: first hit is the best
            }
        }
        if best == 0xff && mask & wheel == wheel {
            best = 3;
        }
        t[mask as usize] = best;
    }
    t
}

static STRAIGHT_TABLE: OnceLock<[u8; 8192]> = OnceLock::new();
static TABLES: OnceLock<Tables> = OnceLock::new();

struct Tables {
    straight: [u8; 8192],
    /// 7-rank prime product → dense rank of the best non-flush 5-card value.
    seven_map: LinearMap,
    /// flush-context packed value → dense rank (small map; avoids partition_point).
    flush_map: LinearMap,
    /// sorted distinct packed 5-card values (the dense scale basis; asserted 7462).
    dense: Vec<u32>,
}

/// Minimal power-of-two linear-probe u64→u16 map (in-crate, allocation at build
/// time only, lookup ~5ns — std's SipHash `HashMap` costs ~20ns and would break
/// the P1 gate; `rustc-hash` is not whitelisted for cham-core). Key and value are
/// interleaved so a probe touches one cache line.
struct LinearMap {
    entries: Vec<(u64, u16)>,
    mask: u64,
}

impl LinearMap {
    fn new(n: usize) -> LinearMap {
        let cap = (n * 2).next_power_of_two().max(16);
        LinearMap {
            entries: vec![(u64::MAX, 0); cap],
            mask: (cap - 1) as u64,
        }
    }
    #[inline]
    fn hash(x: u64) -> u64 {
        // multiply-xor-shift (splitmix-style finalizer)
        let mut z = x.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        z ^= z >> 29;
        z = z.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z ^ (z >> 32)
    }
    fn insert(&mut self, k: u64, v: u16) {
        let mut i = (Self::hash(k) & self.mask) as usize;
        while self.entries[i].0 != u64::MAX {
            if self.entries[i].0 == k {
                self.entries[i].1 = v;
                return;
            }
            i = (i + 1) & self.mask as usize;
        }
        self.entries[i] = (k, v);
    }
    #[inline]
    fn get(&self, k: u64) -> u16 {
        let mut i = (Self::hash(k) & self.mask) as usize;
        loop {
            let (key, val) = self.entries[i];
            if key == k {
                return val;
            }
            if key == u64::MAX {
                return 0;
            }
            i = (i + 1) & self.mask as usize;
        }
    }
}

fn best_nonflush_packed_from_counts(counts: &[u8; 13]) -> u32 {
    let mut quad = None;
    let mut trips: Vec<u8> = Vec::new();
    let mut pairs: Vec<u8> = Vec::new();
    let mut singles: Vec<u8> = Vec::new();
    let mut mask = 0u16;
    for r in 0..13 {
        if counts[r] > 0 {
            mask |= 1u16 << r;
        }
        match counts[r] {
            4 => quad = Some(r as u8),
            3 => trips.push(r as u8),
            2 => pairs.push(r as u8),
            1 => singles.push(r as u8),
            _ => {}
        }
    }
    trips.reverse(); // descending
    pairs.reverse();
    singles.reverse();
    if let Some(q) = quad {
        // kicker = highest remaining rank (a pair/trip rank beats a single)
        let k = (0..13)
            .rev()
            .find(|&r| counts[r] > 0 && r as u8 != q)
            .expect("quad always has a kicker in a 5+ card context") as u8;
        return pack(CAT_QUADS, &[q, k]);
    }
    if let Some(&t0) = trips.first() {
        // full house partners: best pair OR second trips (from a second triple)
        let partner = pairs
            .first()
            .copied()
            .into_iter()
            .chain(trips.get(1).copied())
            .max();
        if let Some(p) = partner {
            return pack(CAT_FULL_HOUSE, &[t0, p]);
        }
    }
    let st = STRAIGHT_TABLE.get_or_init(build_straight_table)[mask as usize];
    if st != 0xff {
        return pack(CAT_STRAIGHT, &[st]);
    }
    if let Some(&t0) = trips.first() {
        let k1 = singles.first().copied().unwrap_or(0);
        let k2 = singles.get(1).copied().unwrap_or(0);
        return pack(CAT_TRIPS, &[t0, k1, k2]);
    }
    if pairs.len() >= 2 {
        // kicker may be the top single OR a third pair's rank
        let k = singles
            .first()
            .copied()
            .into_iter()
            .chain(pairs.get(2).copied())
            .max()
            .unwrap_or(0);
        return pack(CAT_TWO_PAIR, &[pairs[0], pairs[1], k]);
    }
    if let Some(&p) = pairs.first() {
        return pack(
            CAT_PAIR,
            &[
                p,
                *singles.first().unwrap_or(&0),
                *singles.get(1).unwrap_or(&0),
                *singles.get(2).unwrap_or(&0),
            ],
        );
    }
    pack(
        CAT_HIGH,
        &[
            singles.first().copied().unwrap_or(0),
            singles.get(1).copied().unwrap_or(0),
            singles.get(2).copied().unwrap_or(0),
            singles.get(3).copied().unwrap_or(0),
            singles.get(4).copied().unwrap_or(0),
        ],
    )
}

/// Enumerate all rank multisets of `size` (per-rank count ≤ maxc) and call f.
fn for_each_multiset(size: usize, maxc: u8, f: &mut impl FnMut(&[u8; 13])) {
    fn rec(r: usize, left: usize, counts: &mut [u8; 13], maxc: u8, f: &mut impl FnMut(&[u8; 13])) {
        if r == 13 {
            if left == 0 {
                f(counts);
            }
            return;
        }
        let max_here = maxc.min(left as u8);
        for c in 0..=max_here {
            counts[r] = c;
            rec(r + 1, left - c as usize, counts, maxc, f);
        }
        counts[r] = 0;
    }
    let mut counts = [0u8; 13];
    rec(0, size, &mut counts, maxc, f);
}

fn build_tables() -> Tables {
    let straight = build_straight_table();
    // ---- dense scale: distinct packed 5-card values (must be exactly 7462) ----
    let mut values: Vec<u32> = Vec::with_capacity(8000);
    // non-flush contexts: multisets with per-rank count ≤ 4 (5-of-a-kind impossible)
    for_each_multiset(5, 4, &mut |counts| {
        values.push(best_nonflush_packed_from_counts(counts));
    });
    // flush contexts: squarefree multisets (5 distinct ranks); SF when straight
    let mut flush_values: Vec<(u32, ())> = Vec::with_capacity(1400);
    for_each_multiset(5, 1, &mut |counts| {
        let mut ranks = [0u8; 5];
        let mut n = 0;
        for r in 0..13 {
            if counts[r] > 0 {
                ranks[n] = r as u8;
                n += 1;
            }
        }
        ranks.reverse(); // DESCENDING kicker order — must match evaluate7/evaluate5 packing
        let mask: u16 = ranks.iter().fold(0u16, |m, &r| m | (1 << r));
        let packed = if straight[mask as usize] != 0xff {
            pack(CAT_STRAIGHT_FLUSH, &[straight[mask as usize]])
        } else {
            pack(CAT_FLUSH, &ranks)
        };
        flush_values.push((packed, ()));
        values.push(packed);
    });
    values.sort_unstable();
    values.dedup();
    assert_eq!(
        values.len(),
        7462,
        "dense 5-card scale must have exactly 7462 classes"
    );

    let rank_of = |packed: u32| -> u16 { (values.partition_point(|&v| v < packed) + 1) as u16 };

    // ---- flush-context map: packed → dense rank ----
    let mut flush_map = LinearMap::new(flush_values.len());
    for (packed, _) in &flush_values {
        flush_map.insert(*packed as u64, rank_of(*packed));
    }

    // ---- 7-rank multiset → best non-flush dense rank ----
    let mut seven_map = LinearMap::new(60_000);
    for_each_multiset(7, 4, &mut |counts| {
        let mut prod: u64 = 1;
        for r in 0..13 {
            for _ in 0..counts[r] {
                prod = prod.wrapping_mul(PRIMES[r]);
            }
        }
        let best = best_nonflush_packed_from_counts(counts);
        seven_map.insert(prod, rank_of(best));
    });
    Tables {
        straight,
        seven_map,
        flush_map,
        dense: values,
    }
}

fn tables() -> &'static Tables {
    TABLES.get_or_init(build_tables)
}

#[inline]
fn dense_rank(packed: u32) -> u16 {
    let t = tables();
    (t.dense.partition_point(|&v| v < packed) + 1) as u16
}

/// Evaluate a 7-card hand → dense rank 1..=7462 (1 weakest, 7462 royal flush).
///
/// Hot path (P1 gate): ONE pass over the 7 cards builds rank-prime product,
/// per-suit rank bitmasks and per-suit counts together (no re-iteration). Flush
/// detection is a straight-line suit-count select (no per-suit loop with
/// early returns), so the common non-flush path is branch-predictor friendly.
#[inline]
pub fn evaluate7(c: &[Card; 7]) -> u16 {
    let t = tables();
    let mut suit_mask = [0u16; 4];
    let mut suit_count = [0u8; 4];
    let mut prod: u64 = 1;
    // Fixed-size array: lengths asserted by the type; no bounds checks needed.
    for i in 0..7 {
        let idx = c[i].0;
        let rank = idx >> 2; // rank = idx/4 (power of two: shift, no divide)
        let suit = (idx & 3) as usize; // suit = idx%4 (power of two: mask)
        suit_mask[suit] |= 1u16 << rank;
        suit_count[suit] += 1;
        prod = prod.wrapping_mul(PRIMES[rank as usize]);
    }
    // flush path (at most one suit can hold ≥ 5 of 7 cards): straight-line
    // select over the counts built above.
    let flush_suit = if suit_count[0] >= 5 {
        0
    } else if suit_count[1] >= 5 {
        1
    } else if suit_count[2] >= 5 {
        2
    } else if suit_count[3] >= 5 {
        3
    } else {
        return t.seven_map.get(prod);
    };
    let m = suit_mask[flush_suit];
    let st = t.straight[m as usize];
    if st != 0xff {
        return t.flush_map.get(pack(CAT_STRAIGHT_FLUSH, &[st]) as u64);
    }
    flush_top5(m, t)
}

/// Top-5 flush kicker lookup shared by [`evaluate7`] (out of line so the
/// non-flush fast path stays small and inlinable).
#[inline(never)]
fn flush_top5(m: u16, t: &Tables) -> u16 {
    let mut ks = [0u8; 5];
    let mut n = 0;
    for r in (0..13).rev() {
        if m & (1 << r) != 0 {
            ks[n] = r as u8;
            n += 1;
            if n == 5 {
                break;
            }
        }
    }
    t.flush_map.get(pack(CAT_FLUSH, &ks) as u64)
}

/// Batch evaluation exposing instruction-level parallelism: hands are
/// independent, so the CPU pipelines N scalar evals (shared table lookups
/// autovectorize under `target-cpu=native`).
#[inline]
pub fn evaluate7_batch<const N: usize>(hands: &[[Card; 7]; N], out: &mut [u16; N]) {
    debug_assert_eq!(hands.len(), out.len());
    for i in 0..N {
        out[i] = evaluate7(&hands[i]);
    }
}

/// Evaluate exactly 5 cards → dense rank.
pub fn evaluate5(c: &[Card; 5]) -> u16 {
    let t = tables();
    let mut suit_mask = [0u16; 4];
    let mut counts = [0u8; 13];
    for &card in c {
        suit_mask[card.suit() as usize] |= 1 << card.rank();
        counts[card.rank() as usize] += 1;
    }
    for s in 0..4 {
        if suit_mask[s].count_ones() == 5 {
            let m = suit_mask[s];
            let st = t.straight[m as usize];
            if st != 0xff {
                return dense_rank(pack(CAT_STRAIGHT_FLUSH, &[st]));
            }
            let mut ranks = [0u8; 5];
            let mut n = 0;
            for r in (0..13).rev() {
                if m & (1 << r) != 0 {
                    ranks[n] = r as u8;
                    n += 1;
                }
            }
            return dense_rank(pack(CAT_FLUSH, &ranks));
        }
    }
    dense_rank(best_nonflush_packed_from_counts(&counts))
}

/// Best 5-card subset of 7 (off hot path; exhaustive over the 21 subsets).
pub fn best5(c: &[Card; 7]) -> ([Card; 5], u16) {
    const COMBOS: [[usize; 5]; 21] = [
        [0, 1, 2, 3, 4],
        [0, 1, 2, 3, 5],
        [0, 1, 2, 3, 6],
        [0, 1, 2, 4, 5],
        [0, 1, 2, 4, 6],
        [0, 1, 2, 5, 6],
        [0, 1, 3, 4, 5],
        [0, 1, 3, 4, 6],
        [0, 1, 3, 5, 6],
        [0, 1, 4, 5, 6],
        [0, 2, 3, 4, 5],
        [0, 2, 3, 4, 6],
        [0, 2, 3, 5, 6],
        [0, 2, 4, 5, 6],
        [0, 3, 4, 5, 6],
        [1, 2, 3, 4, 5],
        [1, 2, 3, 4, 6],
        [1, 2, 3, 5, 6],
        [1, 2, 4, 5, 6],
        [1, 3, 4, 5, 6],
        [2, 3, 4, 5, 6],
    ];
    let mut best: Option<([Card; 5], u16)> = None;
    for combo in COMBOS {
        let five: [Card; 5] = [
            c[combo[0]],
            c[combo[1]],
            c[combo[2]],
            c[combo[3]],
            c[combo[4]],
        ];
        let v = evaluate5(&five);
        if best.as_ref().is_none_or(|(_, bv)| v > *bv) {
            best = Some((five, v));
        }
    }
    best.expect("21 subsets always exist")
}

/// Partial-board strength key: best non-flush packed value over current cards.
/// Used by the flop/turn EHS proxy (decision D-003): exact off-river equity
/// (including runouts) is ~10^4× too slow for the P4 budget; this proxy is
/// analytic, pure and O(1326) per call.
fn partial_packed(cards: impl Iterator<Item = Card>) -> u32 {
    let mut counts = [0u8; 13];
    for c in cards {
        counts[c.rank() as usize] += 1;
    }
    best_nonflush_packed_from_counts(&counts)
}

/// Current-board strength vs uniform (flop/turn proxy; river = exact equity):
/// fraction of villain combos whose current partial-strength key hero beats or ties.
pub fn strength_now(hero: Hand2, board: &[Card]) -> f64 {
    let [ha, hb] = hero.cards();
    let mut seen = [false; 52];
    seen[ha.idx() as usize] = true;
    seen[hb.idx() as usize] = true;
    for c in board {
        seen[c.idx() as usize] = true;
    }
    let hero_key = partial_packed(hero.cards().into_iter().chain(board.iter().copied()));
    let mut score = 0.0f64;
    let mut n = 0.0f64;
    for i in 0..52u8 {
        if seen[i as usize] {
            continue;
        }
        for j in (i + 1)..52u8 {
            if seen[j as usize] {
                continue;
            }
            let vk = partial_packed([Card(i), Card(j)].into_iter().chain(board.iter().copied()));
            n += 1.0;
            if hero_key >= vk {
                score += 1.0;
            }
        }
    }
    if n == 0.0 { 0.5 } else { score / n }
}

/// Exact equity of hero vs a Range: enumeration, no MC. On a complete 5-card board
/// this is the river-encode workhorse (~O(1326) evaluate7). On shorter boards it
/// enumerates all runout completions exactly — correct but expensive, offline only.
pub fn equity_exact(hero: Hand2, villain: &Range, board: &[Card]) -> (f64, f64) {
    assert!(board.len() <= 5, "board longer than 5");
    let [ha, hb] = hero.cards();
    let mut dead = [false; 52];
    dead[ha.idx() as usize] = true;
    dead[hb.idx() as usize] = true;
    for c in board {
        dead[c.idx() as usize] = true;
    }
    let mut board_full = [Card(0); 5];
    for (i, c) in board.iter().enumerate() {
        board_full[i] = *c;
    }
    let need = 5 - board.len();

    let mut win = 0.0f64;
    let mut tie = 0.0f64;
    let mut total = 0.0f64;

    if need == 0 {
        let hr = evaluate7(&[
            ha,
            hb,
            board_full[0],
            board_full[1],
            board_full[2],
            board_full[3],
            board_full[4],
        ]);
        for combo in villain.iter() {
            let [va, vb] = Hand2::from_combo(combo).cards();
            if dead[va.idx() as usize] || dead[vb.idx() as usize] {
                continue;
            }
            let vr = evaluate7(&[
                va,
                vb,
                board_full[0],
                board_full[1],
                board_full[2],
                board_full[3],
                board_full[4],
            ]);
            total += 1.0;
            if hr > vr {
                win += 1.0;
            } else if hr == vr {
                tie += 1.0;
            }
        }
    } else {
        for combo in villain.iter() {
            let [va, vb] = Hand2::from_combo(combo).cards();
            if dead[va.idx() as usize] || dead[vb.idx() as usize] {
                continue;
            }
            // completions over (unseen \ villain)
            let mut pool: Vec<Card> = Vec::with_capacity(52);
            for &c in ALL_CARDS.iter() {
                if !dead[c.idx() as usize] && c.idx() != va.idx() && c.idx() != vb.idx() {
                    pool.push(c);
                }
            }
            let mut idx = [0usize; 5];
            let mut n_local = 0.0f64;
            let mut w_local = 0.0f64;
            let mut t_local = 0.0f64;
            fill_rec(
                &pool,
                need,
                0,
                &mut idx,
                &mut board_full,
                board.len(),
                &mut |bf: &[Card; 5]| {
                    let hr = evaluate7(&[ha, hb, bf[0], bf[1], bf[2], bf[3], bf[4]]);
                    let vr = evaluate7(&[va, vb, bf[0], bf[1], bf[2], bf[3], bf[4]]);
                    n_local += 1.0;
                    if hr > vr {
                        w_local += 1.0;
                    } else if hr == vr {
                        t_local += 1.0;
                    }
                },
            );
            total += n_local;
            win += w_local;
            tie += t_local;
        }
    }
    if total == 0.0 {
        return (0.0, 0.0);
    }
    (win / total, tie / total)
}

#[allow(clippy::too_many_arguments)]
fn fill_rec(
    pool: &[Card],
    need: usize,
    start: usize,
    _idx: &mut [usize; 5],
    out: &mut [Card; 5],
    fill_at: usize,
    f: &mut impl FnMut(&[Card; 5]),
) {
    if need == 0 {
        f(out);
        return;
    }
    if start + need > pool.len() {
        return;
    }
    for i in start..=pool.len() - need {
        out[fill_at] = pool[i];
        fill_rec(pool, need - 1, i + 1, _idx, out, fill_at + 1, f);
    }
}

/// MC equity — EXISTS ONLY FOR OFFLINE TABLE BUILDING (SPECS/02 §3) and nothing else.
/// Callers restricted by review to cham-engine's table builder and cham-opponents'
/// chart builder.
pub fn equity_mc(
    hero: Hand2,
    villain: &Range,
    board: &[Card],
    iters: u32,
    rng: &mut Rng,
) -> (f64, f64) {
    let [ha, hb] = hero.cards();
    let mut dead = [false; 52];
    dead[ha.idx() as usize] = true;
    dead[hb.idx() as usize] = true;
    for c in board {
        dead[c.idx() as usize] = true;
    }
    let mut pool: Vec<u8> = Vec::with_capacity(52);
    for i in 0..52u8 {
        if !dead[i as usize] {
            pool.push(i);
        }
    }
    let mut board_full = [Card(0); 5];
    for (i, c) in board.iter().enumerate() {
        board_full[i] = *c;
    }
    let need = 5 - board.len();

    let mut win = 0u64;
    let mut tie = 0u64;
    let mut used = 0u64;
    for _ in 0..iters {
        let k = 2 + need;
        for i in 0..k {
            let j = rng.gen_range(i..pool.len());
            pool.swap(i, j);
        }
        let va = Card(pool[0]);
        let vb = Card(pool[1]);
        if !villain.get(Hand2::new(va, vb).combo_id()) {
            continue; // rejection sampling (offline use only)
        }
        for s in 0..need {
            board_full[board.len() + s] = Card(pool[2 + s]);
        }
        let hr = evaluate7(&[
            ha,
            hb,
            board_full[0],
            board_full[1],
            board_full[2],
            board_full[3],
            board_full[4],
        ]);
        let vr = evaluate7(&[
            va,
            vb,
            board_full[0],
            board_full[1],
            board_full[2],
            board_full[3],
            board_full[4],
        ]);
        used += 1;
        if hr > vr {
            win += 1;
        } else if hr == vr {
            tie += 1;
        }
    }
    let n = used.max(1) as f64;
    (win as f64 / n, tie as f64 / n)
}
