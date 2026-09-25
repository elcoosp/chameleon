//! GPU-PLAN G0.2 acceptance: 1M-hand bit-exact consistency between CPU
//! `evaluate7` and the MSL `eval7_kernel`. This is the permanent P7 core.
//!
//! On non-macOS or when the `metal` feature is off, the test prints a SKIP
//! line and returns — the crate's whole point is bit-exact-on-GPU, but the
//! GPU must be optional everywhere else.

use cham_core::card::Card;
use cham_core::eval::evaluate7;
#[cfg(all(target_os = "macos", feature = "metal"))]
use cham_core::eval::{EvalTables, eval_tables};
use cham_core::rng::{next_u32, rng_from_seed};

/// Pinned seed for the 1M-hand corpus (docs/GPU-PLAN.md G0.2 step 3).
const CORPUS_SEED: u64 = 0x0000_060C_0FFE;
const N_HANDS: usize = 1_000_000;

/// Draw `N_HANDS` distinct 7-card hands deterministically from CORPUS_SEED.
/// Duplicates across hands are allowed (they add coverage of the same
/// inputs); duplicates within one hand are not (rejection-sampled per hand).
fn make_corpus() -> Vec<[Card; 7]> {
    let mut rng = rng_from_seed(CORPUS_SEED);
    let mut hands = Vec::with_capacity(N_HANDS);
    for _ in 0..N_HANDS {
        let mut used = [false; 52];
        let mut hand = [Card(0); 7];
        for slot in hand.iter_mut() {
            loop {
                let x = (next_u32(&mut rng) % 52) as u8;
                if !used[x as usize] {
                    used[x as usize] = true;
                    *slot = Card(x);
                    break;
                }
            }
        }
        hands.push(hand);
    }
    hands
}

/// Pack 7 card indices into a u64 (6 bits/card; 42 bits used). Must match
/// cham-gpu's `kernels::pack_hand` and the MSL unpack loop.
fn pack_hand(hand: &[Card; 7]) -> u64 {
    let mut v = 0u64;
    for (i, c) in hand.iter().enumerate() {
        v |= ((c.0 as u64) & 0x3F) << (6 * i as u64);
    }
    v
}

#[test]
fn consistency_eval7_one_million_hands() {
    // ---- CPU reference: 1M evals via cham-core ----
    let corpus = make_corpus();
    let cpu_out: Vec<u16> = corpus.iter().map(evaluate7).collect();
    assert_eq!(cpu_out.len(), N_HANDS);

    // ---- GPU dispatch (skipped on non-macOS / feature off) ----
    #[cfg(all(target_os = "macos", feature = "metal"))]
    {
        if !cham_gpu::kernels::can_dispatch() {
            eprintln!(
                "consistency_eval7: SKIP — no Metal device present (reason: {:?})",
                cham_gpu::probe()
            );
            return;
        }
        let tables: EvalTables<'static> = eval_tables();
        let packed: Vec<u64> = corpus.iter().map(pack_hand).collect();

        // Run 1: primary consistency check.
        let mut gpu_out1 = vec![0u16; N_HANDS];
        cham_gpu::kernels::launch_eval7(&tables, &packed, &mut gpu_out1)
            .expect("launch_eval7 run 1");

        // Run 2: run-to-run determinism (docs/GPU-PLAN.md G0.2 step 4).
        let mut gpu_out2 = vec![0u16; N_HANDS];
        cham_gpu::kernels::launch_eval7(&tables, &packed, &mut gpu_out2)
            .expect("launch_eval7 run 2");

        // Determinism first (cheap): the two GPU runs must agree byte-for-byte.
        assert_eq!(
            gpu_out1, gpu_out2,
            "run-to-run determinism: GPU outputs differ between two identical launches"
        );

        // Then the P7 core: CPU bit-exact vs GPU.
        let mut mismatches = 0usize;
        let mut first_bad: Option<(usize, u16, u16)> = None;
        for i in 0..N_HANDS {
            if cpu_out[i] != gpu_out1[i] {
                mismatches += 1;
                if first_bad.is_none() {
                    first_bad = Some((i, cpu_out[i], gpu_out1[i]));
                }
            }
        }
        assert_eq!(
            mismatches,
            0,
            "GPU vs CPU mismatches: {}/{} (first: idx={:?}, cpu={:?}, gpu={:?})",
            mismatches,
            N_HANDS,
            first_bad.map(|t| t.0),
            first_bad.map(|t| t.1),
            first_bad.map(|t| t.2)
        );
        eprintln!(
            "consistency_eval7: PASS — {}/{} hands bit-equal (2 GPU runs identical)",
            N_HANDS, N_HANDS
        );
    }
    #[cfg(not(all(target_os = "macos", feature = "metal")))]
    {
        eprintln!(
            "consistency_eval7: SKIP — metal unavailable (os={}, feature={})",
            std::env::consts::OS,
            if cfg!(feature = "metal") { "on" } else { "off" }
        );
    }
}

/// Tiny smoke test that needs no GPU at all: run the CPU side only, and
/// verify pack_hand produces a value whose unpack recovers the input.
#[test]
fn pack_unpack_roundtrip() {
    let h: [Card; 7] = [
        Card(0),
        Card(1),
        Card(2),
        Card(3),
        Card(4),
        Card(5),
        Card(6),
    ];
    let p = pack_hand(&h);
    for (i, c) in h.iter().enumerate() {
        let back = ((p >> (6 * i as u64)) & 0x3F) as u8;
        assert_eq!(back, c.0, "pack/unpack roundtrip at slot {i}");
    }
}
