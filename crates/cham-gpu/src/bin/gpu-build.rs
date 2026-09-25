//! GPU-PLAN G1.2b: turn EHS table builder.
//!
//! Enumerates all C(52, 4) = 270,725 turn boards (or the first `--limit`
//! in lexicographic order), dispatches the EHS kernel in batches, and
//! streams the u32 output to disk (blake3 in parallel). Writes a JSON
//! manifest alongside.
//!
//! Usage:
//!   gpu-build --kind turn --limit 20
//!   gpu-build --kind turn --limit 0         # full build, ~1.44 GB, hours
//!   gpu-build --kind turn --limit 20 --no-check
//!
//! `--limit 0` means "all boards" (270,725 for turn). The default is a
//! small sanity run (100) so CI and local dev don't sit on the GPU for
//! hours.

use std::env;
use std::path::PathBuf;

#[cfg(all(target_os = "macos", feature = "metal"))]
use std::fs::{self, File};
#[cfg(all(target_os = "macos", feature = "metal"))]
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(all(target_os = "macos", feature = "metal"))]
use std::time::Instant;

#[cfg(all(target_os = "macos", feature = "metal"))]
use cham_core::card::Card;

/// Total number of 4-card turn boards: C(52, 4).
#[cfg(all(target_os = "macos", feature = "metal"))]
const BOARDS_TURN: usize = 270_725;
/// Number of 2-card holes: C(52, 2).
#[cfg(all(target_os = "macos", feature = "metal"))]
const HOLES: usize = 1326;
/// Denominator per plan Part 0 / EhsDenom::Turn.
#[cfg(all(target_os = "macos", feature = "metal"))]
const DENOM_TURN: u64 = 45_540;

#[cfg_attr(not(all(target_os = "macos", feature = "metal")), allow(dead_code))]
struct Args {
    kind: String,
    /// 0 = all boards; otherwise the first N boards in lexicographic order.
    limit: usize,
    out_dir: PathBuf,
    batch: usize,
    /// Number of (board, hole) pairs to re-verify against the CPU reference.
    /// 0 = skip.
    sample: usize,
    /// Resume an in-progress build: read the existing .bin, hash its prefix,
    /// truncate any partial trailing board, and continue appending. No-op if
    /// the file is absent.
    resume: bool,
}

fn parse_args() -> Args {
    let mut a = Args {
        kind: "turn".into(),
        limit: 100,
        out_dir: PathBuf::from("artifacts/gpu-tables"),
        batch: 512,
        sample: 20,
        resume: false,
    };
    let argv: Vec<String> = env::args().collect();
    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--kind" => {
                i += 1;
                a.kind = argv[i].clone();
            }
            "--limit" => {
                i += 1;
                a.limit = argv[i].parse().unwrap_or(100);
            }
            "--out" => {
                i += 1;
                a.out_dir = PathBuf::from(&argv[i]);
            }
            "--batch" => {
                i += 1;
                a.batch = argv[i].parse().unwrap_or(512);
            }
            "--sample" => {
                i += 1;
                a.sample = argv[i].parse().unwrap_or(20);
            }
            "--no-check" => {
                a.sample = 0;
            }
            "--resume" => {
                a.resume = true;
            }
            "--help" | "-h" => {
                println!(
                    "usage: gpu-build [--kind turn] [--limit N] [--out DIR] [--batch N] [--sample N] [--no-check] [--resume]"
                );
                std::process::exit(0);
            }
            other => {
                eprintln!("gpu-build: unknown arg '{other}'");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    a
}

#[cfg(all(target_os = "macos", feature = "metal"))]
fn enumerate_boards(limit: usize) -> Vec<[Card; 4]> {
    let n = if limit == 0 {
        BOARDS_TURN
    } else {
        limit.min(BOARDS_TURN)
    };
    let mut v = Vec::with_capacity(n);
    'outer: for a in 0u8..52 {
        for b in (a + 1)..52 {
            for c in (b + 1)..52 {
                for d in (c + 1)..52 {
                    v.push([Card(a), Card(b), Card(c), Card(d)]);
                    if v.len() == n {
                        break 'outer;
                    }
                }
            }
        }
    }
    v
}

#[cfg(all(target_os = "macos", feature = "metal"))]
fn main() -> anyhow::Result<()> {
    use cham_core::eval::eval_tables;
    use cham_gpu::GpuContext;
    use cham_gpu::kernels::{can_dispatch, launch_ehs_turn, pack_board4};

    let a = parse_args();
    if a.kind != "turn" {
        anyhow::bail!("only --kind turn is implemented (got '{}')", a.kind);
    }
    if !can_dispatch() {
        anyhow::bail!("no Metal device: {:?}", cham_gpu::probe());
    }

    println!(
        "gpu-build: kind={} limit={} batch={} out={}",
        a.kind,
        if a.limit == 0 { BOARDS_TURN } else { a.limit },
        a.batch,
        a.out_dir.display()
    );

    let ctx = GpuContext::new()?;
    let tables = eval_tables();
    fs::create_dir_all(&a.out_dir)?;

    let boards = enumerate_boards(a.limit);
    let n_boards = boards.len();
    // (packed is built below, once start_board is known.)

    let bin_path = a.out_dir.join(format!("{}.bin", a.kind));
    let per_board = (HOLES * 4) as u64;

    // ---- resume: inspect the existing .bin if --resume was given ----
    let (start_board, mut hasher) = if a.resume && bin_path.exists() {
        let size = fs::metadata(&bin_path)?.len();
        let complete_boards = (size / per_board) as usize;
        let usable_bytes = (complete_boards as u64) * per_board;
        if size != usable_bytes {
            eprintln!(
                "  resume: truncating {} bytes of a partial trailing board",
                size - usable_bytes
            );
            let f = std::fs::OpenOptions::new().write(true).open(&bin_path)?;
            f.set_len(usable_bytes)?;
        }
        eprintln!(
            "  resume: hashing {} existing bytes ({} complete boards)...",
            usable_bytes, complete_boards
        );
        let mut f = File::open(&bin_path)?;
        let mut h = blake3::Hasher::new();
        let mut buf = vec![0u8; 4 * 1024 * 1024];
        let mut remaining = usable_bytes;
        while remaining > 0 {
            let take = (buf.len() as u64).min(remaining) as usize;
            f.read_exact(&mut buf[..take])?;
            h.update(&buf[..take]);
            remaining -= take as u64;
        }
        (complete_boards, h)
    } else {
        (0, blake3::Hasher::new())
    };

    // ---- open for append (resume) or truncate (fresh) ----
    let file = if start_board > 0 {
        std::fs::OpenOptions::new().append(true).open(&bin_path)?
    } else {
        File::create(&bin_path)?
    };
    let mut w = file;
    if start_board >= n_boards {
        eprintln!(
            "  resume: already complete ({} of {} boards present)",
            start_board, n_boards
        );
    }

    // Only dispatch from `start_board` onwards.
    let packed: Vec<u32> = boards[start_board.min(n_boards)..]
        .iter()
        .map(pack_board4)
        .collect();
    let n_new = packed.len();
    let mut out_buf: Vec<u32> = vec![0u32; a.batch * HOLES];
    let t0 = Instant::now();
    let mut done = 0usize;
    while done < n_new {
        let take = a.batch.min(n_new - done);
        launch_ehs_turn(
            &ctx,
            &tables,
            &packed[done..done + take],
            &mut out_buf[..take * HOLES],
        )?;
        // One batch -> one contiguous byte buffer -> one write syscall.
        let mut bytes = Vec::with_capacity(take * HOLES * 4);
        for v in &out_buf[..take * HOLES] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        hasher.update(&bytes);
        w.write_all(&bytes)?;
        done += take;
        let total_done = start_board + done;
        let el = t0.elapsed().as_secs_f64();
        eprintln!(
            "  {}/{} boards ({:.1}/s)",
            total_done,
            n_boards,
            done as f64 / el
        );
    }
    w.flush()?;
    drop(w);
    let wall_s = t0.elapsed().as_secs_f64();
    let hash = hasher.finalize().to_hex().to_string();

    // Sample re-verification against the CPU reference.
    let mut sample_passed = true;
    let mut sample_details: Vec<serde_json::Value> = Vec::new();
    if a.sample > 0 {
        use cham_gpu::reference::{EhsDenom, ehs_reference};
        let mut f = File::open(&bin_path)?;
        let mut seed: u64 = 0xE45_2026_0E11;
        for k in 0..a.sample {
            // xorshift the seed
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let board_i = (seed as usize) % n_boards;
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let hole_i = (seed as usize) % HOLES;
            // Reconstruct the hole's cards from its index.
            let hole = hole_from_index(hole_i as u16);
            let board = &boards[board_i];
            // Skip overlap (kernel leaves 0 for those)
            if board.iter().any(|c| c == &hole[0] || c == &hole[1]) {
                sample_details.push(serde_json::json!({
                    "k": k, "board_i": board_i, "hole_i": hole_i,
                    "skipped": "hole overlaps board"
                }));
                continue;
            }
            let off = ((board_i as u64) * (HOLES as u64) + (hole_i as u64)) * 4;
            let mut buf = [0u8; 4];
            f.seek(SeekFrom::Start(off))?;
            f.read_exact(&mut buf)?;
            let gpu_val = u32::from_le_bytes(buf);
            let cpu_val = ehs_reference(board, hole, EhsDenom::Turn) as u32;
            let ok = gpu_val == cpu_val;
            if !ok {
                sample_passed = false;
            }
            sample_details.push(serde_json::json!({
                "k": k, "board_i": board_i, "hole_i": hole_i,
                "gpu": gpu_val, "cpu": cpu_val, "ok": ok
            }));
        }
        eprintln!(
            "  sample: {} checks, {}",
            a.sample,
            if sample_passed { "PASS" } else { "FAIL" }
        );
    }

    // Manifest
    let git_rev = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    let bytes = (n_boards as u64) * (HOLES as u64) * 4;
    let evals = (n_boards as u64) * (HOLES as u64) * DENOM_TURN;
    let boards_new = n_boards.saturating_sub(start_board);
    let manifest = serde_json::json!({
        "kind": a.kind,
        "tool": "gpu-build",
        "tool_version": env!("CARGO_PKG_VERSION"),
        "git_rev": git_rev,
        "blake3": hash,
        "bytes": bytes,
        "boards": n_boards,
        "holes": HOLES,
        "denom": DENOM_TURN,
        "encoding": "u32_numerator_2wins_plus_ties",
        "seed": serde_json::Value::Null,
        "wall_s": wall_s,
        "evals": evals,
        "boards_per_s": (boards_new as f64) / wall_s,
        "throughput_evals_per_s": ((boards_new as u64) * (HOLES as u64) * DENOM_TURN) as f64 / wall_s,
        "resumed_from_board": start_board,
        "boards_written_this_run": boards_new,
        "sample_check": {
            "n": a.sample,
            "pass": sample_passed,
            "details": sample_details,
        },
        "partial": a.limit != 0,
        "complete": start_board + boards_new >= n_boards,
    });
    let manifest_path = a.out_dir.join(format!("{}.json", a.kind));
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    println!(
        "gpu-build: wrote {} ({} bytes, blake3 {})",
        bin_path.display(),
        bytes,
        &hash[..16]
    );
    let evals = (n_boards as u64) * (HOLES as u64) * DENOM_TURN;
    println!(
        "gpu-build: {} boards in {:.1}s → {:.2e} evals/s ({:.1} boards/s)",
        n_boards,
        wall_s,
        evals as f64 / wall_s,
        (n_boards as f64) / wall_s,
    );
    let full_s = (BOARDS_TURN as f64) / ((n_boards as f64) / wall_s);
    println!(
        "gpu-build: projected full build at this rate: {:.1} h",
        full_s / 3600.0
    );
    if a.sample > 0 && !sample_passed {
        anyhow::bail!("sample re-verification against CPU reference FAILED");
    }
    Ok(())
}

/// Reconstruct a 2-card hole from its `hole2_index` value.
#[cfg(all(target_os = "macos", feature = "metal"))]
fn hole_from_index(idx: u16) -> [Card; 2] {
    let mut hi: u32 = 1;
    while (hi * (hi - 1)) / 2 <= idx as u32 && hi < 52 {
        hi += 1;
    }
    hi -= 1;
    let lo = (idx as u32) - (hi * (hi - 1)) / 2;
    [Card(lo as u8), Card(hi as u8)]
}

#[cfg(not(all(target_os = "macos", feature = "metal")))]
fn main() -> anyhow::Result<()> {
    let _ = parse_args();
    eprintln!("gpu-build: metal feature not enabled or not on macOS — nothing to do");
    Ok(())
}
