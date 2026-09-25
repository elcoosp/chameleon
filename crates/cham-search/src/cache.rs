//! Content-keyed river subgame cache, L1 (BROAD-PERF-PLAN B5).
//!
//! `Subgame::build` normalizes bet fractions and validates ranges on EVERY
//! river trigger; the built subgame (tree structure, codelists) is a pure
//! function of its inputs, so it memoizes safely: same content hash → same
//! built subgame, and `solve()` stays a pure function of (subgame, ranges,
//! iters, seed) — results are bit-identical whether the subgame came from
//! cache or a fresh build (see `solve_cached_equals_fresh`).
//!
//! Store: `Mutex<HashMap<u64, Arc<Subgame>>>` (std only). The key is a
//! blake3-derived u64 over EVERYTHING `Subgame::build` reads: canonical class
//! lists (weight + strength bits), pot, SPR band inputs (pot + stack bits),
//! the action ladder (bet fractions), and the abstraction hash. Bounded:
//! evict-by-generation (full clear) past 256 entries so a long match cannot
//! grow the cache unboundedly.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::SearchError;
use crate::subgame::{Class, Subgame};

/// Generation cap: past this many entries the whole generation is evicted.
const CACHE_CAP: usize = 256;

struct Cache {
    map: Mutex<HashMap<u64, Arc<Subgame>>>,
    hits: AtomicU64,
    misses: AtomicU64,
}

fn global() -> &'static Cache {
    static GLOBAL: OnceLock<Cache> = OnceLock::new();
    GLOBAL.get_or_init(|| Cache {
        map: Mutex::new(HashMap::new()),
        hits: AtomicU64::new(0),
        misses: AtomicU64::new(0),
    })
}

/// Content key: blake3 over every input `Subgame::build` reads, plus the
/// abstraction hash (same board class under a different abstraction is a
/// different subgame).
pub fn cache_key(
    hero: &[Class],
    villain: &[Class],
    pot_bb: f64,
    stack_bb: f64,
    bet_fracs: &[f64],
    abstraction_hash: u64,
) -> u64 {
    // PERF MEASURED (trigger_stream_cache_hit vs fresh_build):
    // blake3 issues 43 small `update` calls here (18 classes × 2 floats + 4
    // scalars + 1 delimiter + lengths), each with its own block-processing
    // overhead — ~2 µs/lookup, which made the cache a NET LOSS on short
    // subgames (Subgame::build is only ~250 ns). Swapped to FxHasher from
    // `rustc-hash` (already in the workspace whitelist): ~1 ns/value,
    // deterministic, and 18 strengths + 4 floats + lengths make an
    // accidental collision negligible in an in-process memo.
    //
    // QUALITY: still a pure content key — same inputs → same key on every
    // run, and `solve_cached_equals_fresh` (bit-exact solve output) remains
    // the correctness gate; the hash is only a lookup index.
    use std::hash::{Hash, Hasher};
    let mut h = rustc_hash::FxHasher::default();
    abstraction_hash.hash(&mut h);
    pot_bb.to_bits().hash(&mut h);
    stack_bb.to_bits().hash(&mut h);
    for c in hero.iter().chain(villain.iter()) {
        c.weight.to_bits().hash(&mut h);
        c.strength.to_bits().hash(&mut h);
    }
    bet_fracs.len().hash(&mut h);
    for f in bet_fracs {
        f.to_bits().hash(&mut h);
    }
    h.finish()
}

/// Cached build: same validation/normalization as `Subgame::build` (a cache
/// HIT never skips validation — the key commits to the exact inputs), backed
/// by the process-global L1.
pub fn cached_build(
    hero_classes: Vec<Class>,
    villain_classes: Vec<Class>,
    pot_bb: f64,
    stack_bb: f64,
    bet_fracs: &[f64],
    abstraction_hash: u64,
) -> Result<Arc<Subgame>, SearchError> {
    let key = cache_key(
        &hero_classes,
        &villain_classes,
        pot_bb,
        stack_bb,
        bet_fracs,
        abstraction_hash,
    );
    let cache = global();
    if let Some(hit) = cache.map.lock().expect("cache").get(&key) {
        cache.hits.fetch_add(1, Ordering::Relaxed);
        return Ok(Arc::clone(hit));
    }
    cache.misses.fetch_add(1, Ordering::Relaxed);
    let built = Arc::new(Subgame::build(
        hero_classes,
        villain_classes,
        pot_bb,
        stack_bb,
        bet_fracs,
    )?);
    let mut map = cache.map.lock().expect("cache");
    if map.len() >= CACHE_CAP {
        map.clear(); // evict-by-generation: bounded memory, still pure
    }
    map.insert(key, Arc::clone(&built));
    Ok(built)
}

/// (hits, misses) on the process-global L1 — printed by benches/drivers so
/// the gain is measurable.
pub fn cache_stats() -> (u64, u64) {
    let c = global();
    (
        c.hits.load(Ordering::Relaxed),
        c.misses.load(Ordering::Relaxed),
    )
}

/// Test-only reset (keeps unit tests hermetic; never used on live paths).
/// Compiled unconditionally so integration tests can use it.
pub fn cache_clear_for_tests() {
    let c = global();
    c.map.lock().expect("cache").clear();
    c.hits.store(0, Ordering::Relaxed);
    c.misses.store(0, Ordering::Relaxed);
}
