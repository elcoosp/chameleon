//! GPU-PLAN G1.3 acceptance: the flop EHS kernel matches the CPU reference
//! bit-for-bit on a small set of (board, hole) pairs.
//!
//! Full-corpus consistency is a build-time sample check (`gpu-build --sample`);
//! this test proves the kernel logic on a manageable set.

use cham_core::card::Card;
#[cfg(all(target_os = "macos", feature = "metal"))]
use cham_core::eval::eval_tables;
use cham_core::eval::hole2_index;

#[test]
fn flop_kernel_matches_reference_on_selected_pairs() {
    // Card id = rank * 4 + suit (rank 0..=12 = 2..=A; suit 0..=3 = c,d,h,s).
    const fn rc(rank: u8, suit: u8) -> Card {
        Card(rank * 4 + suit)
    }
    // 3 hand-picked flops covering flush-draw, rainbow, and paired textures.
    let boards: [[Card; 3]; 3] = [
        // monotone spades: 7s 8s 9s — many straight/flush runouts
        [rc(5, 3), rc(6, 3), rc(7, 3)],
        // rainbow: 2c 7d Jh
        [rc(0, 0), rc(5, 1), rc(9, 2)],
        // paired: 5h 5s Kd
        [rc(3, 3), rc(3, 1), rc(11, 2)],
    ];

    // 6 (board, hole) pairs per board, chosen to exclude board cards.
    let mut pairs: Vec<(usize, [Card; 2], u16)> = Vec::new();
    for (bi, board) in boards.iter().enumerate() {
        let board_ids: [u8; 3] = [board[0].0, board[1].0, board[2].0];
        let mut checked = 0usize;
        let mut combo = 0u16;
        while checked < 6 && combo < 1326 {
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
            let hole = [Card(c_lo), Card(c_hi)];
            pairs.push((bi, hole, hole2_index(hole)));
            checked += 1;
            combo += 1;
        }
    }

    #[cfg(all(target_os = "macos", feature = "metal"))]
    {
        use cham_gpu::GpuContext;
        use cham_gpu::kernels::{launch_ehs_flop, pack_board3};

        if !cham_gpu::kernels::can_dispatch() {
            eprintln!(
                "consistency_flop: SKIP — no Metal device ({:?})",
                cham_gpu::probe()
            );
            return;
        }
        let ctx = GpuContext::new().expect("GpuContext::new");
        let tables = eval_tables();

        let packed: Vec<u32> = boards.iter().map(pack_board3).collect();
        let mut out = vec![0u32; packed.len() * 1326];
        launch_ehs_flop(&ctx, &tables, &packed, &mut out).expect("launch_ehs_flop");

        let mut checked = 0usize;
        let mut mismatches = 0usize;
        for (bi, hole, idx) in &pairs {
            let gpu = out[*bi * 1326 + (*idx as usize)];
            let cpu = cham_gpu::reference::ehs_reference(
                &boards[*bi],
                *hole,
                cham_gpu::reference::EhsDenom::Flop,
            ) as u32;
            if gpu != cpu {
                mismatches += 1;
                if mismatches <= 5 {
                    eprintln!(
                        "consistency_flop: mismatch board={} hole_idx={} gpu={} cpu={}",
                        bi, idx, gpu, cpu
                    );
                }
            }
            checked += 1;
        }
        assert_eq!(
            mismatches, 0,
            "GPU vs CPU flop mismatches: {}/{} pairs",
            mismatches, checked
        );
        eprintln!(
            "consistency_flop: PASS — {}/{} pairs bit-equal",
            checked, checked
        );
    }
    #[cfg(not(all(target_os = "macos", feature = "metal")))]
    {
        let _ = pairs;
        eprintln!("consistency_flop: SKIP — metal unavailable");
    }
}
