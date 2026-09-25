//! GPU-PLAN G1.2b / G1.3a: EHS table builder for turn and flop.
//!
//! Enumerates every board on the given street (or the first `--limit` in
//! ascending lexicographic order), dispatches the corresponding EHS kernel
//! in batches, streams the u32 numerator to disk (running blake3), and
//! writes a JSON manifest.
//!
//! Usage:
//!   gpu-build --kind turn --limit 100               # small sanity run
//!   gpu-build --kind turn --limit 0                 # full turn, ~1.4 h
//!   gpu-build --kind flop --limit 0                 # full flop, ~2.7 h
//!   gpu-build --kind turn --limit 0 --resume        # continue partial
//!
//! Denominators (numerator/denom is EHS):
//!   turn = 46 * 990      = 45,540
//!   flop = C(47,2) * 990 = 1,070,190
//!
//! Board counts:
//!   turn = C(52, 4) = 270,725
//!   flop = C(52, 3) = 22,100

use std::env;
use std::path::PathBuf;

// -----------------------------------------------------------------------
// Constants (metal-only below this point is data used by the metal main).
// -----------------------------------------------------------------------
#[cfg(all(target_os = "macos", feature = "metal"))]
const BOARDS_TURN: usize = 270_725; // C(52, 4)
#[cfg(all(target_os = "macos", feature = "metal"))]
const BOARDS_FLOP: usize = 22_100; // C(52, 3)
#[cfg(all(target_os = "macos", feature = "metal"))]
const HOLES: usize = 1326; // C(52, 2)
#[cfg(all(target_os = "macos", feature = "metal"))]
const DENOM_TURN: u64 = 45_540; // 46 * 990
#[cfg(all(target_os = "macos", feature = "metal"))]
const DENOM_FLOP: u64 = 1_070_190; // 1081 * 990

// -----------------------------------------------------------------------
// Arg parsing (both mains).
// -----------------------------------------------------------------------
#[cfg_attr(not(all(target_os = "macos", feature = "metal")), allow(dead_code))]
struct Args {
    kind: String,
    /// 0 = all boards; otherwise the first N in lexicographic order.
    limit: usize,
    out_dir: PathBuf,
    batch: usize,
    /// Number of (board, hole) pairs to re-verify against the CPU reference.
    sample: usize,
    /// Resume an in-progress build: read existing .bin, hash prefix, truncate
    /// any partial trailing board, and continue appending.
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
                    "usage: gpu-build [--kind turn|flop] [--limit N] [--out DIR] \
                     [--batch N] [--sample N] [--no-check] [--resume]"
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

// -----------------------------------------------------------------------
// Metal-only implementation.
// -----------------------------------------------------------------------
#[cfg(all(target_os = "macos", feature = "metal"))]
mod imp {
    use super::{Args, BOARDS_FLOP, BOARDS_TURN, DENOM_FLOP, DENOM_TURN, HOLES};
    use cham_core::card::Card;
    use cham_core::eval::eval_tables;
    use cham_gpu::GpuContext;
    use cham_gpu::kernels::{
        can_dispatch, launch_ehs_flop, launch_ehs_turn, pack_board3, pack_board4,
    };
    use std::fs::{self, File};
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::time::Instant;

    /// All boards for `kind`, first `limit` (or all if `limit == 0`), as
    /// vectors of card ids in ascending lexicographic order.
    fn enumerate(kind: &str, limit: usize) -> Vec<Vec<Card>> {
        match kind {
            "turn" => {
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
                                v.push(vec![Card(a), Card(b), Card(c), Card(d)]);
                                if v.len() == n {
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
                v
            }
            "flop" => {
                let n = if limit == 0 {
                    BOARDS_FLOP
                } else {
                    limit.min(BOARDS_FLOP)
                };
                let mut v = Vec::with_capacity(n);
                'outer: for a in 0u8..52 {
                    for b in (a + 1)..52 {
                        for c in (b + 1)..52 {
                            v.push(vec![Card(a), Card(b), Card(c)]);
                            if v.len() == n {
                                break 'outer;
                            }
                        }
                    }
                }
                v
            }
            _ => unreachable!("kind validated by caller"),
        }
    }

    fn pack_one(kind: &str, board: &[Card]) -> u32 {
        match kind {
            "flop" => pack_board3(&[board[0], board[1], board[2]]),
            _ => pack_board4(&[board[0], board[1], board[2], board[3]]),
        }
    }

    fn denom(kind: &str) -> u64 {
        if kind == "flop" {
            DENOM_FLOP
        } else {
            DENOM_TURN
        }
    }

    fn total_boards(kind: &str) -> usize {
        if kind == "flop" {
            BOARDS_FLOP
        } else {
            BOARDS_TURN
        }
    }

    fn launch(
        kind: &str,
        ctx: &GpuContext,
        tables: &cham_core::eval::EvalTables<'_>,
        boards: &[u32],
        out: &mut [u32],
    ) -> Result<(), cham_gpu::kernels::KernelError> {
        if kind == "flop" {
            launch_ehs_flop(ctx, tables, boards, out)
        } else {
            launch_ehs_turn(ctx, tables, boards, out)
        }
    }

    /// Reconstruct the (lo, hi) of a hole2_index value, or `None` if the
    /// index isn't a valid unordered pair.
    fn hole2_lo_hi(idx: u16) -> Option<(u8, u8)> {
        let mut hi: u16 = 1;
        while ((hi as u32) * (hi as u32 - 1)) / 2 <= idx as u32 && hi < 52 {
            hi += 1;
        }
        hi -= 1;
        let lo = idx - (hi * (hi - 1)) / 2;
        if lo >= hi {
            None
        } else {
            Some((lo as u8, hi as u8))
        }
    }

    pub fn run(a: &Args) -> anyhow::Result<()> {
        if !can_dispatch() {
            anyhow::bail!("no Metal device: {:?}", cham_gpu::probe());
        }
        let t_all = total_boards(&a.kind);
        let d = denom(&a.kind);
        println!(
            "gpu-build: kind={} limit={} batch={} out={}",
            a.kind,
            if a.limit == 0 { t_all } else { a.limit },
            a.batch,
            a.out_dir.display()
        );

        let ctx = GpuContext::new()?;
        let tables = eval_tables();
        fs::create_dir_all(&a.out_dir)?;

        let boards = enumerate(&a.kind, a.limit);
        let n_boards = boards.len();

        let bin_path = a.out_dir.join(format!("{}.bin", a.kind));
        let per_board = (HOLES * 4) as u64;

        // ---- resume: inspect existing .bin if --resume was given ----
        let (start_board, mut hasher) = if a.resume && bin_path.exists() {
            let size = fs::metadata(&bin_path)?.len();
            let complete = (size / per_board) as usize;
            let usable = (complete as u64) * per_board;
            if size != usable {
                eprintln!(
                    "  resume: truncating {} bytes of a partial trailing board",
                    size - usable
                );
                let f = fs::OpenOptions::new().write(true).open(&bin_path)?;
                f.set_len(usable)?;
            }
            eprintln!(
                "  resume: hashing {} existing bytes ({} complete boards)...",
                usable, complete
            );
            let mut f = File::open(&bin_path)?;
            let mut h = blake3::Hasher::new();
            let mut buf = vec![0u8; 4 * 1024 * 1024];
            let mut remaining = usable;
            while remaining > 0 {
                let take = (buf.len() as u64).min(remaining) as usize;
                f.read_exact(&mut buf[..take])?;
                h.update(&buf[..take]);
                remaining -= take as u64;
            }
            (complete, h)
        } else {
            (0, blake3::Hasher::new())
        };

        // ---- open for append (resume) or truncate (fresh) ----
        let mut w = if start_board > 0 {
            fs::OpenOptions::new().append(true).open(&bin_path)?
        } else {
            File::create(&bin_path)?
        };
        if start_board >= n_boards {
            eprintln!(
                "  resume: already complete ({} of {} boards present)",
                start_board, n_boards
            );
        }

        // ---- only pack and dispatch the boards we still need ----
        let packed: Vec<u32> = boards[start_board.min(n_boards)..]
            .iter()
            .map(|b| pack_one(&a.kind, b))
            .collect();
        let n_new = packed.len();
        let mut out_buf: Vec<u32> = vec![0u32; a.batch * HOLES];
        let t0 = Instant::now();
        let mut done = 0usize;
        while done < n_new {
            let take = a.batch.min(n_new - done);
            launch(
                &a.kind,
                &ctx,
                &tables,
                &packed[done..done + take],
                &mut out_buf[..take * HOLES],
            )?;
            // One batch -> one contiguous byte buffer -> one write.
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

        // ---- sample re-verification against the CPU reference ----
        let mut sample_passed = true;
        let mut details: Vec<serde_json::Value> = Vec::new();
        if a.sample > 0 {
            use cham_gpu::reference::{EhsDenom, ehs_reference};
            let street = if a.kind == "flop" {
                EhsDenom::Flop
            } else {
                EhsDenom::Turn
            };
            let mut f = File::open(&bin_path)?;
            let mut seed: u64 = 0xE45_2026_0E11;
            let mut checked = 0usize;
            let mut attempts = 0usize;
            while checked < a.sample && attempts < a.sample * 6 {
                attempts += 1;
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let board_i = (seed as usize) % n_boards.max(1);
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let hole_i = (seed as usize) % HOLES;
                let (lo, hi) = match hole2_lo_hi(hole_i as u16) {
                    Some(v) => v,
                    None => continue,
                };
                let board = &boards[board_i];
                if board.iter().any(|c| c.0 == lo || c.0 == hi) {
                    details.push(serde_json::json!({
                        "board_i": board_i, "hole_i": hole_i,
                        "skipped": "hole overlaps board"
                    }));
                    continue;
                }
                let off = (board_i as u64) * per_board + (hole_i as u64) * 4;
                f.seek(SeekFrom::Start(off))?;
                let mut buf = [0u8; 4];
                f.read_exact(&mut buf)?;
                let gpu_val = u32::from_le_bytes(buf);
                let hole = [Card(lo), Card(hi)];
                let cpu_val = ehs_reference(&board[..], hole, street) as u32;
                let ok = gpu_val == cpu_val;
                if !ok {
                    sample_passed = false;
                }
                checked += 1;
                details.push(serde_json::json!({
                    "board_i": board_i, "hole_i": hole_i,
                    "gpu": gpu_val, "cpu": cpu_val, "ok": ok
                }));
            }
            eprintln!(
                "  sample: {} checks, {}",
                checked,
                if sample_passed { "PASS" } else { "FAIL" }
            );
        }

        // ---- manifest ----
        let bytes = (n_boards as u64) * (HOLES as u64) * 4;
        let evals = (n_boards as u64) * (HOLES as u64) * d;
        let boards_new = n_boards.saturating_sub(start_board);
        let git_rev = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".into());
        let manifest = serde_json::json!({
            "kind": a.kind,
            "tool": "gpu-build",
            "tool_version": env!("CARGO_PKG_VERSION"),
            "git_rev": git_rev,
            "blake3": hash,
            "bytes": bytes,
            "boards": n_boards,
            "holes": HOLES,
            "denom": d,
            "encoding": "u32_numerator_2wins_plus_ties",
            "seed": serde_json::Value::Null,
            "wall_s": wall_s,
            "evals": evals,
            "boards_per_s": (boards_new as f64) / wall_s,
            "throughput_evals_per_s": ((boards_new as u64) * (HOLES as u64) * d) as f64 / wall_s,
            "resumed_from_board": start_board,
            "boards_written_this_run": boards_new,
            "sample_check": {
                "n": a.sample,
                "pass": sample_passed,
                "details": details,
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
            &hash[..16.min(hash.len())]
        );
        println!(
            "gpu-build: {} boards in {:.1}s → {:.2e} evals/s ({:.1} boards/s)",
            n_boards,
            wall_s,
            evals as f64 / wall_s,
            (boards_new as f64) / wall_s,
        );
        let full_s = (t_all as f64) / ((boards_new as f64) / wall_s.max(1e-9));
        println!(
            "gpu-build: projected full {kind} build: {h:.1} h",
            kind = a.kind,
            h = full_s / 3600.0
        );

        if a.sample > 0 && !sample_passed {
            anyhow::bail!("sample re-verification against CPU reference FAILED");
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------
// Entry points.
// -----------------------------------------------------------------------
#[cfg(all(target_os = "macos", feature = "metal"))]
fn main() -> anyhow::Result<()> {
    let a = parse_args();
    if a.kind != "turn" && a.kind != "flop" {
        anyhow::bail!("unknown --kind '{}' (expect turn | flop)", a.kind);
    }
    imp::run(&a)
}

#[cfg(not(all(target_os = "macos", feature = "metal")))]
fn main() -> anyhow::Result<()> {
    let _ = parse_args();
    eprintln!("gpu-build: metal feature off or not on macOS — nothing to do");
    Ok(())
}
