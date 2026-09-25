//! GPU-PLAN G1.2 acceptance: the turn EHS kernel matches the CPU reference
//! bit-for-bit on a small set of (board, hole) pairs.
//!
//! Full-corpus tests are for CI and the builder; this test proves the
//! kernel logic is correct on a manageable number of comparisons.

use cham_core::card::Card;
#[cfg(all(target_os = "macos", feature = "metal"))]
use cham_core::eval::eval_tables;
use cham_core::eval::hole2_index;

/// Build a Card from an index (0..52) without needing a string parser.
fn card(i: u8) -> Card {
    Card(i)
}

#[test]
fn turn_kernel_matches_reference_on_selected_pairs() {
    // 3 hand-picked boards covering flush and rainbow textures.
    // Card id = rank * 4 + suit (rank 0..=12 = 2..=A; suit 0..=3 = c,d,h,s).
    const fn rc(rank: u8, suit: u8) -> Card {
        Card(rank * 4 + suit)
    }
    let boards: [[Card; 4]; 3] = [
        // monotone spades: 7s 8s 9s Ts — straight-flush potential
        [rc(5, 3), rc(6, 3), rc(7, 3), rc(8, 3)],
        // rainbow: 2c 7d Jh As
        [rc(0, 0), rc(5, 1), rc(9, 2), rc(12, 3)],
        // paired rank: 5h 5s Kd Qc
        [rc(3, 3), rc(3, 1), rc(11, 2), rc(10, 0)],
    ];

    // 8 (board, hole) pairs to check per board — chosen to exclude cards
    // already on the board. We use hole2_index to get a canonical id, then
    // skip any that overlap the board.
    let mut pairs: Vec<(usize, [Card; 2], u16)> = Vec::new();
    for (bi, board) in boards.iter().enumerate() {
        let board_ids: [u8; 4] = [board[0].0, board[1].0, board[2].0, board[3].0];
        let mut checked = 0usize;
        let mut combo = 0u16;
        while checked < 8 && combo < 1326 {
            // Reconstruct (lo, hi) from combo id.
            let mut hi = 1u16;
            while (hi * (hi - 1)) / 2 <= combo && hi < 52 {
                hi += 1;
            }
            hi -= 1;
            let lo = combo - (hi * (hi - 1)) / 2;
            if lo >= hi {
                combo += 1;
                continue;
            }
            let (c_lo, c_hi) = (lo as u8, hi as u8);
            if board_ids.contains(&c_lo) || board_ids.contains(&c_hi) {
                combo += 1;
                continue;
            }
            let hole = [card(c_lo), card(c_hi)];
            let idx = hole2_index(hole);
            pairs.push((bi, hole, idx));
            checked += 1;
            combo += 1;
        }
    }

    #[cfg(all(target_os = "macos", feature = "metal"))]
    {
        use cham_gpu::GpuContext;
        use cham_gpu::kernels::{launch_ehs_turn, pack_board4};

        if !cham_gpu::kernels::can_dispatch() {
            eprintln!(
                "consistency_turn: SKIP — no Metal device ({:?})",
                cham_gpu::probe()
            );
            return;
        }
        let ctx = GpuContext::new().expect("GpuContext::new");
        let tables = eval_tables();

        // Pack all 3 boards; dispatch over 3 * 1326 threads.
        let boards_packed: Vec<u32> = boards.iter().map(pack_board4).collect();
        let mut out = vec![0u32; boards_packed.len() * 1326];
        launch_ehs_turn(&ctx, &tables, &boards_packed, &mut out).expect("launch_ehs_turn");

        let mut checked = 0usize;
        let mut mismatches = 0usize;
        for (bi, hole, idx) in &pairs {
            let gpu = out[*bi * 1326 + (*idx as usize)];
            let cpu = cham_gpu::reference::ehs_reference(
                &boards[*bi],
                *hole,
                cham_gpu::reference::EhsDenom::Turn,
            ) as u32;
            if gpu != cpu {
                mismatches += 1;
                if mismatches <= 5 {
                    eprintln!(
                        "consistency_turn: mismatch board={} hole_idx={} gpu={} cpu={}",
                        bi, idx, gpu, cpu
                    );
                }
            }
            checked += 1;
        }
        assert_eq!(
            mismatches, 0,
            "GPU vs CPU turn mismatches: {}/{} pairs",
            mismatches, checked
        );
        eprintln!(
            "consistency_turn: PASS — {}/{} pairs bit-equal",
            checked, checked
        );
    }
    #[cfg(not(all(target_os = "macos", feature = "metal")))]
    {
        let _ = pairs; // silence unused on non-macOS
        eprintln!("consistency_turn: SKIP — metal unavailable");
    }
}
