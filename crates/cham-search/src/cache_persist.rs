//! Persistent river-subgame cache (PERF-BACKLOG B-2).
//!
//! Session-1 hydrates from `artifacts/river-cache.bin`; session-N saves
//! at clean exit. Warm solves stay warm across process restarts, so the
//! first hand of session N+1 sees the same 189 µs lookups that session N
//! saw on its last hand — instead of re-paying the 35 ms cold solve.
//!
//! Format (little-endian):
//!
//! ```text
//! [magic: u32 = 0x5053_4853 ("SHP"+"S")]
//! [version: u32 = 1]
//! [n_entries: u32]
//! for each entry:
//!   [key: u64]
//!   [len: u32]              // postcard(Subgame) byte count
//!   [postcard(Subgame): len bytes]
//! ```
//!
//! Pure file I/O; no mmap needed (≤ 2048 entries × ~500 B ≈ 1 MB).
//! Determinism: postcard's wire format is frozen since 1.0.0, so a
//! `Subgame` serializes to the same bytes across runs. `hydrate_from`
//! MERGES into the process-global L1 (never clears existing entries);
//! a bad magic or truncated file returns an `io::Error`, so callers can
//! ignore persistence failures with a single `if let Ok(..)`.
//!
//! Cache-cap interplay: the L1 evicts per-entry LRU past `CACHE_CAP`
//! (2048). A saved file larger than the cap is truncated at save time to
//! the first `MAX_ENTRIES` in iteration order; hydrate refuses to load
//! more than the cap so `verify --gpu`-style callers can't see a
//! surprising L1 size.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::cache;
use crate::subgame::Subgame;

/// M-11 fix (2026-09-27): validate a hydrated `Subgame` against the same
/// invariants `Subgame::build` enforces. Used by `hydrate` so a tampered
/// `river-cache.bin` cannot inject a subgame whose weights don't sum to 1,
/// whose ranges are empty, or whose floats are non-finite.
fn validate_subgame(sg: &Subgame) -> Result<(), String> {
    if sg.hero_classes.is_empty() {
        return Err("empty hero range".into());
    }
    if sg.villain_classes.is_empty() {
        return Err("empty villain range".into());
    }
    let wh: f64 = sg.hero_classes.iter().map(|c| c.weight).sum();
    if !wh.is_finite() || (wh - 1.0).abs() > 1e-6 {
        return Err(format!("hero weights sum to {wh}, expected 1"));
    }
    let wv: f64 = sg.villain_classes.iter().map(|c| c.weight).sum();
    if !wv.is_finite() || (wv - 1.0).abs() > 1e-6 {
        return Err(format!("villain weights sum to {wv}, expected 1"));
    }
    for c in sg.hero_classes.iter().chain(sg.villain_classes.iter()) {
        if !c.weight.is_finite() || !c.strength.is_finite() {
            return Err(format!("non-finite class {c:?}"));
        }
    }
    if !sg.pot_bb.is_finite() || sg.pot_bb < 0.0 {
        return Err(format!("bad pot_bb {}", sg.pot_bb));
    }
    if !sg.stack_bb.is_finite() || sg.stack_bb <= 0.0 {
        return Err(format!("bad stack_bb {}", sg.stack_bb));
    }
    Ok(())
}

/// Magic bytes at the file head: LE u32 of the ASCII bytes "SHSP"
/// (0x53='S', 0x48='H', 0x53='S', 0x50='P'). The doubled 'S' is a
/// namespace tag for a future sub-cache if one ever needs a distinct
/// container.
const MAGIC: u32 = 0x5053_4853;
/// Version bump on any format change.
const VERSION: u32 = 2;
/// Same cap as the in-process L1 (v3 §1.2: raised 256 → 2048 alongside the
/// LRU switch; `cache::CACHE_CAP` is the single source of truth).
const MAX_ENTRIES: usize = cache::CACHE_CAP;

/// Serialize the process-global cache to `path` (atomic write via `.tmp` +
/// rename). Returns the number of entries written.
///
/// Writes nothing if the cache is empty but still creates a valid
/// zero-entry file — the caller can distinguish "no cache" from "cache
/// had no entries" by the file's existence.
pub fn save_to(path: &Path) -> io::Result<usize> {
    // Snapshot the map under the lock, then write outside it.
    let snapshot: Vec<(u64, Vec<u8>)> = cache::with_global_map(|map| {
        let mut out = Vec::with_capacity(map.len().min(MAX_ENTRIES));
        for (k, sg) in map.iter().take(MAX_ENTRIES) {
            let bytes = postcard::to_allocvec(&**sg).expect("Subgame postcard is infallible");
            out.push((*k, bytes));
        }
        out
    });

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let tmp = path.with_extension("tmp");
    {
        let f = fs::File::create(&tmp)?;
        let mut w = io::BufWriter::new(f);
        w.write_all(&MAGIC.to_le_bytes())?;
        w.write_all(&VERSION.to_le_bytes())?;
        w.write_all(&(snapshot.len() as u32).to_le_bytes())?;
        for (k, bytes) in &snapshot {
            w.write_all(&k.to_le_bytes())?;
            w.write_all(&(bytes.len() as u32).to_le_bytes())?;
            w.write_all(bytes)?;
        }
        w.flush()?;
    }
    fs::rename(&tmp, path)?;
    Ok(snapshot.len())
}

/// Merge entries from `path` into the process-global cache. Returns the
/// number of new entries inserted (duplicates are skipped, so re-hydrating
/// the same file is a no-op). Never clears existing entries.
///
/// On a missing file this returns `Ok(0)` — a first-run caller doesn't
/// need to special-case ENOENT.
pub fn hydrate_from(path: &Path) -> io::Result<usize> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let mut cur = 0usize;
    let take = |cur: &mut usize, n: usize| -> io::Result<&[u8]> {
        if *cur + n > bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "cache file truncated",
            ));
        }
        let s = &bytes[*cur..*cur + n];
        *cur += n;
        Ok(s)
    };

    let magic = u32::from_le_bytes(take(&mut cur, 4)?.try_into().unwrap());
    if magic != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("cache: bad magic 0x{magic:08x}"),
        ));
    }
    let ver = u32::from_le_bytes(take(&mut cur, 4)?.try_into().unwrap());
    if ver != VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("cache: unsupported version {ver}"),
        ));
    }
    let n = u32::from_le_bytes(take(&mut cur, 4)?.try_into().unwrap()) as usize;
    if n > MAX_ENTRIES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("cache: {n} entries exceeds cap {MAX_ENTRIES}"),
        ));
    }
    let mut loaded = Vec::with_capacity(n);
    for _ in 0..n {
        let key = u64::from_le_bytes(take(&mut cur, 8)?.try_into().unwrap());
        let len = u32::from_le_bytes(take(&mut cur, 4)?.try_into().unwrap()) as usize;
        let payload = take(&mut cur, len)?;
        let sg: Subgame = postcard::from_bytes(payload).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("cache: postcard: {e}"))
        })?;
        // M-11 fix (2026-09-27): re-run the same validation `Subgame::build`
        // enforces before we insert a hydrated subgame into the process-global
        // cache. A poisoned `river-cache.bin` used to be trusted on decode,
        // contradicting the "cache HIT never skips validation" contract.
        validate_subgame(&sg).map_err(|reason| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("cache: hydrated subgame failed validation: {reason}"),
            )
        })?;
        loaded.push((key, sg));
    }

    let fresh_keys: Vec<u64> = cache::with_global_map(|map| {
        use std::collections::hash_map::Entry;
        let mut fresh = Vec::new();
        for (k, sg) in loaded {
            if let Entry::Vacant(slot) = map.entry(k) {
                slot.insert(std::sync::Arc::new(sg));
                fresh.push(k);
            }
        }
        fresh
    });
    // Keep the LRU order in lockstep with the out-of-band map inserts
    // (hydrate must not create order-less entries the evictor can't see).
    for k in &fresh_keys {
        cache::note_inserted(*k);
    }
    Ok(fresh_keys.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subgame::Class;

    fn spot(i: u32) -> Subgame {
        let hero: Vec<Class> = (0..9)
            .map(|j| Class {
                weight: 1.0 / 9.0,
                strength: (i as f64 + j as f64) / 20.0,
            })
            .collect();
        let villain: Vec<Class> = (0..9)
            .map(|j| Class {
                weight: 1.0 / 9.0,
                strength: (j as f64) / 8.0,
            })
            .collect();
        Subgame::build(hero, villain, 12.0 + i as f64, 92.0, &[0.5, 1.25]).expect("sg")
    }

    #[test]
    fn roundtrip_three_spots() {
        let _serial = cache::test_serial_lock();
        cache::cache_clear_for_tests();
        // Populate by content key.
        for i in 0..3 {
            let sg = spot(i);
            let key = cache::cache_key(
                &sg.hero_classes,
                &sg.villain_classes,
                sg.pot_bb,
                sg.stack_bb,
                &sg.bet_fracs,
                0xCAFE,
            );
            let _ = key; // just to keep the impl in scope; populate below
        }
        // Real populate:
        for i in 0..3 {
            cache::cached_build(
                spot(i).hero_classes,
                spot(i).villain_classes,
                12.0 + i as f64,
                92.0,
                &[0.5, 1.25],
                0xCAFE,
            )
            .expect("populate");
        }
        let (_, misses_before) = cache::cache_stats();
        assert_eq!(misses_before, 3);

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("cache.bin");
        let n = save_to(&path).expect("save");
        assert_eq!(n, 3);

        cache::cache_clear_for_tests();
        let ins = hydrate_from(&path).expect("hydrate");
        assert_eq!(ins, 3);

        // Any of the three spots now hits (no new miss).
        let (_, m1) = cache::cache_stats();
        let _ = cache::cached_build(
            spot(1).hero_classes,
            spot(1).villain_classes,
            12.0 + 1.0,
            92.0,
            &[0.5, 1.25],
            0xCAFE,
        )
        .expect("hit");
        let (_, m2) = cache::cache_stats();
        assert_eq!(m1, m2, "post-hydrate call must hit, not miss");
    }

    #[test]
    fn hydrate_rejects_bad_magic() {
        let _serial = cache::test_serial_lock();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.bin");
        std::fs::write(&path, b"garbage").expect("write");
        let err = hydrate_from(&path).expect_err("should err");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn hydrate_missing_file_is_ok_zero() {
        let _serial = cache::test_serial_lock();
        let dir = tempfile::tempdir().expect("tempdir");
        let n = hydrate_from(&dir.path().join("nope.bin")).expect("no file");
        assert_eq!(n, 0);
    }

    #[test]
    fn hydrate_truncated_header_errors() {
        let _serial = cache::test_serial_lock();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("cut.bin");
        // 6 bytes is < 12 (magic + version + count).
        std::fs::write(&path, &MAGIC.to_le_bytes()[..4]).expect("write");
        let err = hydrate_from(&path).expect_err("should err");
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn save_empty_still_writes_valid_header() {
        let _serial = cache::test_serial_lock();
        cache::cache_clear_for_tests();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("empty.bin");
        let n = save_to(&path).expect("save");
        assert_eq!(n, 0);
        let ins = hydrate_from(&path).expect("hydrate");
        assert_eq!(ins, 0);
    }
}
