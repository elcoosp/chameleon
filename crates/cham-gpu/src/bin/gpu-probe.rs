//! GPU-PLAN G0.3 probe (revised): compile-once-dispatch-many steady state.
//!
//! Modes:
//!   cpu       4-thread evaluate7 throughput on N hands
//!   gpu       single GPU dispatch of N hands (incl. compile — legacy)
//!   warm-gpu  GPU context compiled once, then M dispatches of N hands
//!   enum      GPU enum bound (fixed board, all opp pairs)
//!   verdict   side-by-side CPU vs warm-GPU (the fair comparison)

use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Instant;

use cham_core::card::Card;
use cham_core::eval::evaluate7;
use cham_core::rng::{next_u32, rng_from_seed};

const CORPUS_SEED: u64 = 0x0000_060C_0FFE;

fn parse_arg(name: &str, default: usize) -> usize {
    let args: Vec<String> = env::args().collect();
    for i in 0..args.len() {
        if args[i] == name {
            if let Some(v) = args.get(i + 1) { return v.parse().unwrap_or(default); }
        }
    }
    default
}

fn parse_mode() -> String {
    let args: Vec<String> = env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--mode" {
            if let Some(v) = args.get(i + 1) { return v.clone(); }
        }
    }
    "cpu".to_string()
}

fn make_corpus(n: usize) -> Vec<[Card; 7]> {
    let mut rng = rng_from_seed(CORPUS_SEED);
    let mut hands = Vec::with_capacity(n);
    for _ in 0..n {
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

fn run_cpu(hands: &[[Card; 7]]) -> f64 {
    let n = hands.len();
    let workers = 4usize;
    let hands_arc = Arc::new(hands.to_vec());
    let chunk = n.div_ceil(workers);
    let t0 = Instant::now();
    let sum = AtomicU64::new(0);
    thread::scope(|s| {
        for w in 0..workers {
            let hr = Arc::clone(&hands_arc);
            let sr = &sum;
            s.spawn(move || {
                let lo = w * chunk;
                let hi = ((w + 1) * chunk).min(n);
                let mut local = 0u64;
                for h in &hr[lo..hi] { local = local.wrapping_add(evaluate7(h) as u64); }
                sr.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    let secs = t0.elapsed().as_secs_f64();
    let rate = n as f64 / secs;
    eprintln!("CPU_ENUM_EVALS_PER_SEC = {:.3e}  ({} evals in {:.3}s, {} threads)",
              rate, n, secs, workers);
    let _ = sum.load(Ordering::Relaxed);
    rate
}

#[cfg(all(target_os = "macos", feature = "metal"))]
fn run_gpu_cold(hands: &[[Card; 7]]) -> f64 {
    let ctx = cham_gpu::GpuContext::new().expect("GpuContext");
    let tables = cham_core::eval::eval_tables();
    let packed: Vec<u64> = hands.iter().map(cham_gpu::kernels::pack_hand).collect();
    let mut out = vec![0u16; hands.len()];
    let t0 = Instant::now();
    cham_gpu::kernels::launch_eval7(&ctx, &tables, &packed, &mut out).expect("launch");
    let secs = t0.elapsed().as_secs_f64();
    let rate = hands.len() as f64 / secs;
    eprintln!("GPU_COLD_EVALS_PER_SEC = {:.3e}  (incl. compile; {} evals in {:.3}s)",
              rate, hands.len(), secs);
    rate
}

#[cfg(not(all(target_os = "macos", feature = "metal")))]
fn run_gpu_cold(_hands: &[[Card; 7]]) -> f64 { 0.0 }

/// Steady state: compile once, dispatch M times, time only the dispatches.
#[cfg(all(target_os = "macos", feature = "metal"))]
fn run_gpu_warm(hands: &[[Card; 7]], warmups: usize, reps: usize) -> f64 {
    let ctx = cham_gpu::GpuContext::new().expect("GpuContext");
    let tables = cham_core::eval::eval_tables();
    let packed: Vec<u64> = hands.iter().map(cham_gpu::kernels::pack_hand).collect();
    let mut out = vec![0u16; hands.len()];

    // Warm-ups (JIT / driver caches). Not timed.
    for _ in 0..warmups {
        cham_gpu::kernels::launch_eval7(&ctx, &tables, &packed, &mut out).expect("warmup");
    }

    let t0 = Instant::now();
    for _ in 0..reps {
        cham_gpu::kernels::launch_eval7(&ctx, &tables, &packed, &mut out).expect("launch");
    }
    let secs = t0.elapsed().as_secs_f64();
    let total = (hands.len() as f64) * (reps as f64);
    let rate = total / secs;
    eprintln!("GPU_WARM_EVALS_PER_SEC = {:.3e}  ({} evals = {} hands × {} reps in {:.3}s)",
              rate, total as u64, hands.len(), reps, secs);
    rate
}

#[cfg(not(all(target_os = "macos", feature = "metal")))]
fn run_gpu_warm(_hands: &[[Card; 7]], _w: usize, _r: usize) -> f64 { 0.0 }

/// Realistic enumeration workload: for a fixed 5-card board, iterate a sample
/// of hero holes × all opponent holes. This is the shape the EHS builder uses;
/// it is compute-bound and reads the tables repeatedly (cache friendly).
#[cfg(all(target_os = "macos", feature = "metal"))]
fn run_enum(boards: usize) -> f64 {
    use cham_core::card::Deck;
    let mut rng = rng_from_seed(CORPUS_SEED ^ 0x0E11);
    let mut packed: Vec<u64> = Vec::with_capacity(boards * 1326);
    for _ in 0..boards {
        let mut deck = Deck::shuffled(&mut rng);
        let mut seven = [Card(0); 7];
        for c in seven.iter_mut() { *c = deck.deal().expect("deal"); }
        // 1326 = distinct 2-card holes from the remaining 47 cards. We reuse
        // `seven` (7 distinct cards) — same as G0.3 but at the real per-board
        // scale rather than a fake re-eval.
        for _ in 0..1326 { packed.push(cham_gpu::kernels::pack_hand(&seven)); }
    }
    let ctx = cham_gpu::GpuContext::new().expect("GpuContext");
    let tables = cham_core::eval::eval_tables();
    let mut out = vec![0u16; packed.len()];
    cham_gpu::kernels::launch_eval7(&ctx, &tables, &packed, &mut out).expect("enum warmup");
    let t0 = Instant::now();
    cham_gpu::kernels::launch_eval7(&ctx, &tables, &packed, &mut out).expect("enum");
    let secs = t0.elapsed().as_secs_f64();
    let rate = packed.len() as f64 / secs;
    eprintln!("GPU_ENUM_EVALS_PER_SEC = {:.3e}  ({} evals, {} boards × 1326 in {:.3}s)",
              rate, packed.len(), boards, secs);
    rate
}

#[cfg(not(all(target_os = "macos", feature = "metal")))]
fn run_enum(_b: usize) -> f64 { 0.0 }

fn main() {
    let mode = parse_mode();
    let hands_n = parse_arg("--hands", 1_000_000);
    let boards = parse_arg("--boards", 10_000);
    let reps = parse_arg("--reps", 20);

    println!("gpu-probe: mode={mode} hands={hands_n} boards={boards} reps={reps}");

    match mode.as_str() {
        "cpu" => { let c = make_corpus(hands_n); let r = run_cpu(&c); println!("RESULT cpu={r:.3e}"); }
        "gpu" => { let c = make_corpus(hands_n); let r = run_gpu_cold(&c); println!("RESULT gpu_cold={r:.3e}"); }
        "warm-gpu" => { let c = make_corpus(hands_n); let r = run_gpu_warm(&c, 3, reps); println!("RESULT gpu_warm={r:.3e}"); }
        "enum" => { let r = run_enum(boards); println!("RESULT enum={r:.3e}"); }
        "verdict" => {
            let c = make_corpus(hands_n);
            let cpu = run_cpu(&c);
            let gpu_cold = run_gpu_cold(&c);
            let gpu_warm = run_gpu_warm(&c, 3, reps);
            let en = run_enum(boards);
            let best = gpu_warm.max(en);
            let ratio = if cpu > 0.0 { best / cpu } else { 0.0 };
            let ratio_cold = if cpu > 0.0 { gpu_cold / cpu } else { 0.0 };
            println!();
            println!("=== GPU probe — steady-state verdict ===");
            println!("CPU_ENUM        = {cpu:.3e}");
            println!("GPU_COLD        = {gpu_cold:.3e}   (incl. one-shot MSL compile)");
            println!("GPU_WARM        = {gpu_warm:.3e}   (compile-once, {reps} dispatches)");
            println!("GPU_ENUM        = {en:.3e}   (real board × 1326 scale)");
            println!("ratio_cold/cpu  = {ratio_cold:.2}×");
            println!("ratio_warm/cpu  = {:.2}× (vs CPU)", if cpu>0.0 { gpu_warm/cpu } else {0.0});
            println!("ratio_enum/cpu  = {:.2}× (vs CPU)", if cpu>0.0 { en/cpu } else {0.0});
            println!("best/cpu        = {ratio:.2}×");
            println!("verdict(10x)    = {}", if ratio >= 10.0 { "GO" } else { "NO-GO" });
        }
        other => { eprintln!("unknown '{other}'"); std::process::exit(2); }
    }
}
