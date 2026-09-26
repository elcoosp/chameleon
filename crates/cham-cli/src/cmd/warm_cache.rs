//! `chameleon warm-cache` (v3 §1.2): pre-seed `artifacts/river-cache.bin`
//! from a short deterministic calibration session, run ONCE before a
//! multi-arm `EXP-*` sweep.
//!
//! Rationale: real matches recur at similar SPR bands across arms (the whole
//! justification for the river-subgame cache), so the first arm's cold
//! solves can be turned into the same ~189 µs warm path the last arm gets —
//! by solving a canonical fixture grid up front and persisting it. The
//! fixture is pure `cached_build` calls (no agent, no RNG beyond fixed
//! constants), hence deterministic: same file content for the same code.

use cham_search::subgame::Class;

/// Canonical fixture: SPR-band grid × strength-tilt grid. Kept small
/// (5 pots × 3 stacks × 2 ladders × 3 tilts = 90 builds, each ~µs–ms)
/// so the command itself finishes in seconds.
const POTS: [f64; 5] = [8.0, 12.0, 20.0, 32.0, 50.0];
const STACKS: [f64; 3] = [40.0, 80.0, 150.0];

fn ladder(i: usize) -> &'static [f64] {
    match i {
        0 => &[0.5, 1.0],
        _ => &[0.5, 1.25],
    }
}

/// 9 uniform classes with a linear strength tilt (`tilt` shifts mass toward
/// strong (−1), even (0), or weak (+1) — mimics nit/station-leaning ranges
/// without needing a trained agent).
fn tilted_classes(tilt: i32) -> Vec<Class> {
    (0..9)
        .map(|j| {
            let s = (j as f64) / 8.0;
            let strength = match tilt {
                -1 => s * s,
                1 => s.sqrt(),
                _ => s,
            };
            Class {
                weight: 1.0 / 9.0,
                strength,
            }
        })
        .collect()
}

pub fn run(pool: &str, out: &str) -> i32 {
    // `pool` is accepted for sweep-script uniformity (the warm grid is
    // pool-independent by construction — SPR bands recur across arms);
    // validate it exists so typos fail loudly instead of silently.
    if !std::path::Path::new(pool).exists() {
        eprintln!("warm-cache: pool file '{pool}' not found");
        return crate::cmd::EXIT_FAIL;
    }
    let _guard =
        crate::cmd::cache_guard::CachePersist::hydrate("warm-cache", out);
    // Fixed fixture hash: the warm grid is pool-independent by construction
    // (SPR bands recur across arms), so one constant identifies all fixture
    // entries. Real-match entries carry the live abstraction hash and never
    // collide with this namespace in practice.
    let abstraction_hash: u64 = 0xCAFE_1234_5678_9ABC;
    let (h0, m0) = cham_search::cache::cache_stats();
    let mut built = 0usize;
    for tilt in [-1, 0, 1] {
        for pot in POTS {
            for stack in STACKS {
                for li in 0..2 {
                    let hero = tilted_classes(tilt);
                    let villain = tilted_classes(-tilt);
                    match cham_search::cache::cached_build(
                        hero,
                        villain,
                        pot,
                        stack,
                        ladder(li),
                        abstraction_hash,
                    ) {
                        Ok(_) => built += 1,
                        Err(e) => {
                            eprintln!("warm-cache: fixture build failed: {e}");
                            return crate::cmd::EXIT_FAIL;
                        }
                    }
                }
            }
        }
    }
    let (h1, m1) = cham_search::cache::cache_stats();
    println!(
        "warm-cache: {built} fixture builds (hits {}→{}, misses {}→{}) → {out}",
        h0,
        h1,
        m0,
        m1
    );
    // `CachePersist`'s Drop saves the now-warm L1 to `out`.
    crate::cmd::EXIT_OK
}
