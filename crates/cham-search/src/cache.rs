//! Content-keyed river subgame cache, L1 (BROAD-PERF-PLAN B5).
//!
//! `Subgame::build` normalizes bet fractions and validates ranges on EVERY
//! river trigger; the built subgame (tree structure, codelists) is a pure
//! function of its inputs, so it memoizes safely: same content hash → same
//! built subgame, and `solve()` stays a pure function of (subgame, ranges,
//! iters, seed) — results are bit-identical whether the subgame came from
//! cache or a fresh build (see `solve_cached_equals_fresh`).
//!
//! Store: `Mutex<HashMap<u64, Arc<Subgame>>>` (std only) plus an LRU order
//! deque under a second mutex. The key is a FxHash-derived u64 over
//! EVERYTHING `Subgame::build` reads: canonical class lists (weight +
//! strength bits), pot, SPR band inputs (pot + stack bits), the action
//! ladder (bet fractions), and the abstraction hash. Bounded: per-entry LRU
//! eviction past 2048 entries (v3 §1.2) so a long match cannot grow the
//! cache unboundedly — and a wide `ab` sweep only drops the coldest entry,
//! never the whole warm generation.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::SearchError;
use crate::subgame::{Class, Subgame};

/// Capacity: past this many entries the LEAST-recently-used entry is evicted
/// (v3 §1.2: raised 256 → 2048 and switched from evict-by-generation/full
/// clear to per-entry LRU — a single `ab` sweep touching more distinct SPR
/// bands than the cap used to throw away the whole warm session; now only
/// the coldest entry goes). ~2048 × ~500 B ≈ 1 MB, negligible vs the fence.
pub const CACHE_CAP: usize = 2048;

struct Cache {
    map: Mutex<HashMap<u64, Arc<Subgame>>>,
    /// LRU order, front = least-recently-used. Kept in lockstep with `map`
    /// under the same mutex; duplicates never stored (see `touch`).
    order: Mutex<std::collections::VecDeque<u64>>,
    hits: AtomicU64,
    misses: AtomicU64,
}

fn global() -> &'static Cache {
    static GLOBAL: OnceLock<Cache> = OnceLock::new();
    GLOBAL.get_or_init(|| Cache {
        map: Mutex::new(HashMap::new()),
        order: Mutex::new(std::collections::VecDeque::new()),
        hits: AtomicU64::new(0),
        misses: AtomicU64::new(0),
    })
}

/// Move `key` to the most-recently-used end (caller holds no locks; both
/// taken in a fixed order: map then order — never the reverse).
fn touch(key: u64) {
    let cache = global();
    let mut order = cache.order.lock().expect("cache order");
    if let Some(pos) = order.iter().position(|&k| k == key) {
        order.remove(pos);
    }
    order.push_back(key);
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
        let hit = Arc::clone(hit);
        touch(key);
        return Ok(hit);
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
    if map.contains_key(&key) {
        // Raced insert between our lookup and this lock: keep first winner.
        let hit = Arc::clone(&map[&key]);
        drop(map);
        touch(key);
        return Ok(hit);
    }
    if map.len() >= CACHE_CAP {
        // LRU eviction: drop the least-recently-used entry only (still pure:
        // every entry is a content-keyed memo, eviction never changes values).
        let mut order = cache.order.lock().expect("cache order");
        while map.len() >= CACHE_CAP {
            match order.pop_front() {
                Some(old) => {
                    map.remove(&old);
                }
                None => break,
            }
        }
        order.push_back(key);
    } else {
        cache.order.lock().expect("cache order").push_back(key);
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

/// Scoped access to the process-global map under its lock. Used by
/// `cache_persist` to snapshot/merge without exposing the `Mutex` itself.
/// Callers must not hold the lock across I/O.
pub(crate) fn with_global_map<R>(f: impl FnOnce(&mut HashMap<u64, Arc<Subgame>>) -> R) -> R {
    let c = global();
    let mut guard = c.map.lock().expect("cache");
    f(&mut guard)
}

/// Test-only reset (keeps unit tests hermetic; never used on live paths).
/// Compiled unconditionally so integration tests can use it.
///
/// MUST be called with [`test_serial_lock`] held: the cache is process-global
/// and Rust runs tests in one binary on parallel threads, so two tests
/// clearing/populating concurrently corrupt each other's assertions (this
/// raced in practice once the LRU bookkeeping widened the populate→save
/// window). The lock is std-only, zero-cost outside tests.
static TEST_SERIAL: Mutex<()> = Mutex::new(());

/// Hold for the whole body of any test that mutates or asserts on the global
/// cache (clear/populate/stats/save/hydrate).
pub fn test_serial_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_SERIAL.lock().expect("test serial lock")
}

pub fn cache_clear_for_tests() {
    let c = global();
    c.map.lock().expect("cache").clear();
    c.order.lock().expect("cache order").clear();
    c.hits.store(0, Ordering::Relaxed);
    c.misses.store(0, Ordering::Relaxed);
}

/// Re-register a key as most-recently-used after an OUT-OF-BAND map insert
/// (e.g. `cache_persist::hydrate_from` merging file entries via
/// `with_global_map`). Keeps LRU order in lockstep with the map without
/// exposing the `Mutex` itself.
pub(crate) fn note_inserted(key: u64) {
    touch(key);
}

/// Current entry count (for persist caps and telemetry).
pub fn cache_len() -> usize {
    global().map.lock().expect("cache").len()
}
