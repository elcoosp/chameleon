//! CPU reference enumerator for EHS (GPU-PLAN G1.1).
//!
//! `ehs_reference` is the slow-but-obviously-correct oracle the future GPU
//! kernel is checked against (P7). It uses `cham_core::eval::evaluate7` and
//! the standard "2*wins + ties" numerator over a denominator that depends
//! on the street (see `EhsDenom` below).
//!
//! Not used by any hot path: the builders are offline tooling, and the
//! reference only runs at test time and in `gpu-probe --mode cpu-enum`.

use cham_core::card::Card;
use cham_core::eval::evaluate7;

/// Denominator (`numerator / denom` is EHS) per street.
///
/// - River: 990 opponent 2-card combos from the 45 remaining (after hero's 2)
/// - Turn:  46 rivers × 990 opponent combos = 45,540
/// - Flop:  C(47, 2) = 1081 (turn, river) runouts × 990 opponent combos
///   = 1,070,190
///
/// These match the plan's Part 0 table exactly (SPECS/06 §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EhsDenom {
    River,
    Turn,
    Flop,
}

impl EhsDenom {
    pub fn value(self) -> u64 {
        match self {
            EhsDenom::River => 990,
            EhsDenom::Turn => 45_540,
            EhsDenom::Flop => 1_070_190,
        }
    }
}

/// Exact EHS numerator (`2 * wins + ties`) for one (board, hero hole) pair
/// on the given street. Denominator is `EhsDenom::value(street)`.
///
/// The board has 5 (river), 4 (turn), or 3 (flop) cards; hero has 2. The
/// remaining cards (turn/flop: unknown runouts; all streets: opponent
/// hands) are enumerated exactly, no sampling.
pub fn ehs_reference(board: &[Card], hole: [Card; 2], street: EhsDenom) -> u64 {
    // Sanity: 5/4/3 board cards per street.
    let want_board = match street {
        EhsDenom::River => 5,
        EhsDenom::Turn => 4,
        EhsDenom::Flop => 3,
    };
    assert_eq!(board.len(), want_board, "board length vs street mismatch");
    assert!(
        !board.contains(&hole[0]) && !board.contains(&hole[1]) && hole[0] != hole[1],
        "hole cards must be distinct from each other and from the board"
    );

    // Build the set of dead cards (board + hero hole) once.
    let mut dead = [false; 52];
    for c in board {
        dead[c.idx() as usize] = true;
    }
    dead[hole[0].idx() as usize] = true;
    dead[hole[1].idx() as usize] = true;

    // Remaining = all non-dead cards. Order is card-id ascending, which is
    // the SAME order the GPU kernel will use (documented invariant).
    let mut remaining: Vec<Card> = Vec::with_capacity(52);
    for i in 0u8..52 {
        if !dead[i as usize] {
            remaining.push(Card(i));
        }
    }

    match street {
        EhsDenom::River => {
            // 7-card hero hand = board(5) + hole(2).
            let hero7: [Card; 7] = [
                board[0], board[1], board[2], board[3], board[4], hole[0], hole[1],
            ];
            let hero_rank = evaluate7(&hero7);
            count_pairs(
                &remaining,
                &[board[0], board[1], board[2], board[3], board[4]],
                hero_rank,
            )
        }
        EhsDenom::Turn => {
            // For each river card r, evaluate hero's 7 (board + r + hole),
            // then count opp pairs from the remaining 45.
            let mut numerator: u64 = 0;
            let n = remaining.len(); // 46
            for ri in 0..n {
                let river = remaining[ri];
                let hero7: [Card; 7] = [
                    board[0], board[1], board[2], board[3], river, hole[0], hole[1],
                ];
                let hero_rank = evaluate7(&hero7);
                // Opp cards are all remaining EXCEPT the river.
                let mut opp_pool: Vec<Card> = Vec::with_capacity(n - 1);
                for (k, c) in remaining.iter().enumerate() {
                    if k != ri {
                        opp_pool.push(*c);
                    }
                }
                numerator += count_pairs(
                    &opp_pool,
                    &[board[0], board[1], board[2], board[3], river],
                    hero_rank,
                );
            }
            numerator
        }
        EhsDenom::Flop => {
            // For each (turn, river) runout pair, evaluate hero's 7 (board +
            // 2 runouts + hole), then count opp pairs from the remaining 45.
            let mut numerator: u64 = 0;
            let n = remaining.len(); // 47
            for ti in 0..n {
                for ri in (ti + 1)..n {
                    let t = remaining[ti];
                    let r = remaining[ri];
                    let hero7: [Card; 7] = [board[0], board[1], board[2], t, r, hole[0], hole[1]];
                    let hero_rank = evaluate7(&hero7);
                    let mut opp_pool: Vec<Card> = Vec::with_capacity(n - 2);
                    for (k, c) in remaining.iter().enumerate() {
                        if k != ti && k != ri {
                            opp_pool.push(*c);
                        }
                    }
                    numerator +=
                        count_pairs(&opp_pool, &[board[0], board[1], board[2], t, r], hero_rank);
                }
            }
            numerator
        }
    }
}

/// Enumerate all 2-card combos from `pool`, evaluate each villain hand as
/// `board5 + [pool[i], pool[j]]` against `hero_rank`, and return
/// `2 * wins + ties`.
///
/// `board5` is the 5-card community prefix already fixed for this call:
/// - river: the 5 board cards
/// - turn:  4 board cards + the specific river being enumerated
/// - flop:  3 board cards + the specific (turn, river) being enumerated
///
/// `pool` is the 45 non-hero, non-community cards. Order is card-id
/// ascending (the GPU kernel uses the same order).
fn count_pairs(pool: &[Card], board5: &[Card; 5], hero_rank: u16) -> u64 {
    debug_assert_eq!(pool.len(), 45, "opponent pool must be 45 at every street");
    let mut wins: u64 = 0;
    let mut ties: u64 = 0;
    for i in 0..pool.len() {
        for j in (i + 1)..pool.len() {
            let villain7: [Card; 7] = [
                board5[0], board5[1], board5[2], board5[3], board5[4], pool[i], pool[j],
            ];
            let vr = evaluate7(&villain7);
            if hero_rank > vr {
                wins += 1;
            } else if hero_rank == vr {
                ties += 1;
            }
        }
    }
    2 * wins + ties
}

#[cfg(test)]
mod tests {
    use super::*;
    use cham_core::card::Card;

    fn c(s: &str) -> Card {
        Card::parse(s).expect("card")
    }

    /// Royal flush fully on the board: EVERY player makes the same royal,
    /// so every one of the C(45,2) = 990 villain pairs ties.
    /// numerator = 2*0 wins + 990 ties = 990.
    #[test]
    fn royal_flush_on_board_all_ties() {
        let board = [c("As"), c("Ks"), c("Qs"), c("Js"), c("Ts")];
        let hole = [c("2h"), c("3d")];
        let num = ehs_reference(&board, hole, EhsDenom::River);
        assert_eq!(num, 990, "royal on board → every pair ties");
    }

    /// Hero has a royal via his hole card; the completing card is dead
    /// (in hero's hole), so no villain can tie or beat.
    /// Board: As Ks Qs Js 7h. Hero: Ts 2c (royal in spades).
    /// Every one of the 990 pairs loses → numerator = 2 * 990 = 1980.
    ///
    /// Verified independently in Python (see worklog G1.1 entry).
    #[test]
    fn royal_via_hole_no_villain_ties() {
        let board = [c("As"), c("Ks"), c("Qs"), c("Js"), c("7h")];
        let hole = [c("Ts"), c("2c")];
        let num = ehs_reference(&board, hole, EhsDenom::River);
        assert_eq!(num, 1980, "completing card is dead → hero wins every pair");
    }

    /// Wheel straight on the board; hero has blanks and cannot improve.
    /// Board: As 2c 3d 4h 5s. Hero: Kh Qd.
    ///
    /// Hero's best 5 is the wheel (A2345, 5-high straight).
    /// - Villains holding at least one 6 make 23456 (6-high straight) and
    ///   beat hero: C(4,1)*C(41,1) + C(4,2) = 164 + 6 = 170 pairs.
    /// - Every other pair plays the same wheel and ties hero: 820 pairs.
    /// - Hero never wins outright.
    ///   numerator = 2*0 (hero wins) + 820 (ties) = 820.
    ///
    /// Two independent implementations (Rust reference + a Python evaluator
    /// written for verification) both produce 820 — recorded in worklog G1.1.
    #[test]
    fn wheel_on_board_hero_never_wins_only_ties() {
        let board = [c("As"), c("2c"), c("3d"), c("4h"), c("5s")];
        let hole = [c("Kh"), c("Qd")];
        let num = ehs_reference(&board, hole, EhsDenom::River);
        assert_eq!(num, 820, "hero ties 820, loses 170, wins 0");
    }

    #[test]
    fn denom_values_match_plan() {
        assert_eq!(EhsDenom::River.value(), 990);
        assert_eq!(EhsDenom::Turn.value(), 45_540);
        assert_eq!(EhsDenom::Flop.value(), 1_070_190);
    }
}
