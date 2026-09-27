//! GPU-PLAN G0.2 acceptance: 1M-hand bit-exact consistency between CPU
//! `evaluate7` and the MSL `eval7_kernel`. Permanent P7 core.

use cham_core::card::Card;
use cham_core::eval::evaluate7;
use cham_core::rng::{next_u32, rng_from_seed};

const CORPUS_SEED: u64 = 0x0000_060C_0FFE;
const N_HANDS: usize = 1_000_000;

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

#[test]
fn consistency_eval7_one_million_hands() {
    let corpus = make_corpus();
    let cpu_out: Vec<u16> = corpus.iter().map(evaluate7).collect();
    assert_eq!(cpu_out.len(), N_HANDS);

    #[cfg(all(target_os = "macos", feature = "metal"))]
    {
        if !cham_gpu::kernels::can_dispatch() {
            eprintln!(
                "consistency_eval7: SKIP — no Metal device ({:?})",
                cham_gpu::probe()
            );
            return;
        }
        let ctx = cham_gpu::GpuContext::new().expect("GpuContext::new");
        eprintln!("consistency_eval7: device = {}", ctx.name());
        let tables = cham_core::eval::eval_tables();
        let packed: Vec<u64> = corpus.iter().map(cham_gpu::kernels::pack_hand).collect();

        let mut gpu_out1 = vec![0u16; N_HANDS];
        cham_gpu::kernels::launch_eval7(&ctx, &tables, &packed, &mut gpu_out1).expect("run 1");
        let mut gpu_out2 = vec![0u16; N_HANDS];
        cham_gpu::kernels::launch_eval7(&ctx, &tables, &packed, &mut gpu_out2).expect("run 2");
        assert_eq!(gpu_out1, gpu_out2, "run-to-run determinism");

        let mut mism = 0usize;
        let mut first_bad: Option<(usize, u16, u16)> = None;
        for i in 0..N_HANDS {
            if cpu_out[i] != gpu_out1[i] {
                mism += 1;
                if first_bad.is_none() {
                    first_bad = Some((i, cpu_out[i], gpu_out1[i]));
                }
            }
        }
        assert_eq!(
            mism, 0,
            "mismatches {}/{} first={:?}",
            mism, N_HANDS, first_bad
        );
        eprintln!(
            "consistency_eval7: PASS — {}/{} bit-equal",
            N_HANDS, N_HANDS
        );
    }
    #[cfg(feature = "wgpu")]
    {
        match cham_gpu::wgpu_backend::WgpuContext::new(&cham_core::eval::eval_tables()) {
            Ok(ctx) => {
                eprintln!("consistency_eval7: wgpu backend = {}", ctx.name());
                let packed: Vec<u64> = corpus.iter().map(cham_gpu::kernels::pack_hand).collect();
                let mut gpu_out1 = vec![0u16; N_HANDS];
                ctx.dispatch_eval7(&packed, &mut gpu_out1)
                    .expect("wgpu run 1");
                let mut gpu_out2 = vec![0u16; N_HANDS];
                ctx.dispatch_eval7(&packed, &mut gpu_out2)
                    .expect("wgpu run 2");
                assert_eq!(gpu_out1, gpu_out2, "wgpu run-to-run determinism");
                let mut mism = 0usize;
                for i in 0..N_HANDS {
                    if cpu_out[i] != gpu_out1[i] {
                        mism += 1;
                    }
                }
                assert_eq!(mism, 0, "wgpu vs CPU mismatches: {}/{}", mism, N_HANDS);
                eprintln!(
                    "consistency_eval7: wgpu PASS — {}/{} bit-equal",
                    N_HANDS, N_HANDS
                );
            }
            Err(e) => {
                // M-16 fix (2026-09-27): honoring an env var lets CI
                // (gpu.yml) require a wgpu adapter and fail loudly when the
                // "cross-platform correctness guarantee" would otherwise
                // silently vanish. Default (env unset) keeps the dev-loop
                // skip so a laptop without the adapter isn't blocked.
                if std::env::var("CHAM_GPU_REQUIRE_WGPU").as_deref() == Ok("1") {
                    panic!(
                        "consistency_eval7: wgpu REQUIRED (CHAM_GPU_REQUIRE_WGPU=1) but \
                         adapter init failed: {e}"
                    );
                }
                eprintln!("consistency_eval7: wgpu SKIP — {e}");
            }
        }
    }
    #[cfg(not(all(target_os = "macos", feature = "metal")))]
    {
        if !cfg!(feature = "wgpu") {
            eprintln!("consistency_eval7: SKIP — metal unavailable, wgpu feature off");
        }
    }
}

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
    let p = cham_gpu::kernels::pack_hand(&h);
    for (i, c) in h.iter().enumerate() {
        let back = ((p >> (6 * i as u64)) & 0x3F) as u8;
        assert_eq!(back, c.0);
    }
}
