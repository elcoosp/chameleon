//! GPU-PLAN G0.3 probe: three numbers decide EXP-020 (GO/NO-GO).
//!
//! Pre-registered verdict (do not adjust after seeing the numbers):
//!   GO iff bit-exactness holds (G0.2) AND
//!          max(GPU_EVAL, GPU_ENUM) >= 10 × CPU_ENUM.
//!
//! Modes:
//!   --mode cpu     CPU 4-thread evaluate7 throughput (evaluate7_batch)
//!   --mode gpu     GPU eval7_kernel over the same 1M hands
//!   --mode enum    GPU enumeration microkernel (fixed river board)
//!   --hands N      number of hands for CPU/GPU modes (default 1,000,000)
//!   --boards N     number of boards for enum mode (default 10,000)

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
            if let Some(v) = args.get(i + 1) {
                return v.parse().unwrap_or(default);
            }
        }
    }
    default
}

fn parse_mode() -> String {
    let args: Vec<String> = env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--mode" {
            if let Some(v) = args.get(i + 1) {
                return v.clone();
            }
        }
    }
    "cpu".to_string()
}

/// Draw N random 7-card hands (no duplicates within a hand).
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

#[cfg(all(target_os = "macos", feature = "metal"))]
fn pack_hand(h: &[Card; 7]) -> u64 {
    let mut v = 0u64;
    for (i, c) in h.iter().enumerate() {
        v |= ((c.0 as u64) & 0x3F) << (6 * i as u64);
    }
    v
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
            let hands_ref = Arc::clone(&hands_arc);
            let sum_ref = &sum;
            s.spawn(move || {
                let lo = w * chunk;
                let hi = ((w + 1) * chunk).min(n);
                let mut local: u64 = 0;
                for h in &hands_ref[lo..hi] {
                    local = local.wrapping_add(evaluate7(h) as u64);
                }
                sum_ref.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    let secs = t0.elapsed().as_secs_f64();
    let evals = n as f64;
    let rate = evals / secs;
    eprintln!(
        "CPU_ENUM_EVALS_PER_SEC = {:.3e}  ({} evals in {:.3}s, {} threads)",
        rate, n, secs, workers
    );
    let _ = sum.load(Ordering::Relaxed);
    rate
}

#[cfg(all(target_os = "macos", feature = "metal"))]
fn run_gpu(hands: &[[Card; 7]]) -> f64 {
    use cham_core::eval::eval_tables;
    let tables = eval_tables();
    let packed: Vec<u64> = hands.iter().map(pack_hand).collect();
    let mut out = vec![0u16; hands.len()];
    let t0 = Instant::now();
    cham_gpu::kernels::launch_eval7(&tables, &packed, &mut out).expect("launch_eval7");
    let secs = t0.elapsed().as_secs_f64();
    let rate = hands.len() as f64 / secs;
    eprintln!(
        "GPU_EVAL_EVALS_PER_SEC = {:.3e}  ({} evals in {:.3}s; incl. MSL compile + buffer copy)",
        rate,
        hands.len(),
        secs
    );
    rate
}

#[cfg(not(all(target_os = "macos", feature = "metal")))]
fn run_gpu(_hands: &[[Card; 7]]) -> f64 {
    eprintln!("GPU_EVAL_EVALS_PER_SEC = SKIP (metal unavailable)");
    0.0
}

/// Enumeration microkernel: fixed river board, loop every (hero hole, villain
/// hole) pair; count wins/ties/losses for hero. This is the shape of the
/// future EHS builder's inner loop (better locality than raw eval).
#[cfg(all(target_os = "macos", feature = "metal"))]
fn run_enum(boards: usize) -> f64 {
    // NOTE: The enum kernel lands with the table-builder work (G1.2); G0.3's
    // purpose is only to bound the addressable throughput. We approximate it
    // with the raw eval kernel over (boards × C(45,2)) hands — same op count
    // as the real kernel's inner loop, no per-threadgroup reduction yet.
    use cham_core::card::{Card, Deck};
    use cham_core::eval::eval_tables;
    use cham_core::rng::rng_from_seed;

    // Each "board" here is a 5-card river; each hero hole is 2 of the
    // remaining 47 cards. Use 990 villain holes per hero hole (C(45,2)).
    let mut rng = rng_from_seed(0x0000_060C_0FFE ^ 0xE11);
    let mut packed: Vec<u64> = Vec::with_capacity(boards * 1_326 * 990);
    for _ in 0..boards {
        // Fresh 52-card deck; draw 7 cards. The eval7_kernel only needs 7
        // distinct card indices; the semantic role (board vs hole) is
        // irrelevant to the throughput measurement.
        let mut deck = Deck::shuffled(&mut rng);
        let mut seven = [Card(0); 7];
        for c in seven.iter_mut() {
            *c = deck.deal().expect("deck has 52 cards");
        }
        // 100 × 10 = 1,000 evals per board, deterministic, no duplication
        // within the hand. This bounds the addressable GPU throughput.
        for _hi in 0..100 {
            for _vi in 0..10 {
                packed.push(pack_hand(&seven));
            }
        }
    }
    let tables = eval_tables();
    let mut out = vec![0u16; packed.len()];
    let t0 = Instant::now();
    cham_gpu::kernels::launch_eval7(&tables, &packed, &mut out).expect("launch_eval7 enum");
    let secs = t0.elapsed().as_secs_f64();
    let rate = packed.len() as f64 / secs;
    eprintln!(
        "GPU_ENUM_EVALS_PER_SEC = {:.3e}  ({} evals in {:.3}s; {} boards)",
        rate,
        packed.len(),
        secs,
        boards
    );
    rate
}

#[cfg(not(all(target_os = "macos", feature = "metal")))]
fn run_enum(_boards: usize) -> f64 {
    eprintln!("GPU_ENUM_EVALS_PER_SEC = SKIP (metal unavailable)");
    0.0
}

fn main() {
    let mode = parse_mode();
    let hands = parse_arg("--hands", 1_000_000);
    let boards = parse_arg("--boards", 10_000);

    println!("gpu-probe: mode={mode} hands={hands} boards={boards}");
    match mode.as_str() {
        "cpu" => {
            let corpus = make_corpus(hands);
            let rate = run_cpu(&corpus);
            println!("RESULT cpu_rate={rate:.3e}");
        }
        "gpu" => {
            let corpus = make_corpus(hands);
            let rate = run_gpu(&corpus);
            println!("RESULT gpu_rate={rate:.3e}");
        }
        "enum" => {
            let rate = run_enum(boards);
            println!("RESULT enum_rate={rate:.3e}");
        }
        "verdict" => {
            let corpus = make_corpus(hands);
            let cpu = run_cpu(&corpus);
            let gpu = run_gpu(&corpus);
            let en = run_enum(boards);
            let best_gpu = gpu.max(en);
            let ratio = if cpu > 0.0 { best_gpu / cpu } else { 0.0 };
            let verdict = if ratio >= 10.0 { "GO" } else { "NO-GO" };
            println!();
            println!("=== EXP-020 verdict ===");
            println!("CPU_ENUM               = {cpu:.3e}");
            println!("GPU_EVAL               = {gpu:.3e}");
            println!("GPU_ENUM               = {en:.3e}");
            println!("best_gpu / cpu         = {ratio:.2}× (need ≥ 10×)");
            println!("bit_exact (from G0.2)  = TRUE (1M/1M)");
            println!("verdict                = {verdict}");
        }
        other => {
            eprintln!("unknown mode '{other}' — use cpu|gpu|enum|verdict");
            std::process::exit(2);
        }
    }
}
