# Parallel trainer: scope and plan (2026-09-29)

## Why this is the highest-leverage remaining work

Every measurement in today's docs is single-threaded on an 8-core box.
`docs/plans/FULL-VS-TINY-2026-09-28.md` concluded that the next real
experiment (full abstraction at 9M iters/expert for equal visits-per-
infoset) is ~30 hours single-threaded. With a proper worker pool that
becomes ~4 hours. The trainer's `--threads N` flag exists but does
nothing (M-6 in the bug report).

## What already exists (and what is missing)

**Already exists:**
- `RegretTable` uses atomic `compare_exchange_weak` for regret updates
  (`regret_add_cfr_plus`) and atomic `fetch_add` for strat/weight/visit
  (`table.rs`). The storage layer is safe for concurrent writers.
- `ThreadMode::Hogwild` and `ThreadMode::Snapbatch` are defined; both
  have working buffer/flush machinery.
- `default_threads(mode)` returns 8 or `available_parallelism()`.
- The trainer accepts `threads: u32` and serializes it into provenance.

**Missing:**
- Nothing ever calls `std::thread::spawn`. The loop at `trainer.rs:312`
  runs `for t in start..total_iters` on one thread.
- `Encoder` is `&mut` in the loop. Each worker needs its own (the
  Encoder has per-iteration caches: `eq_cache`, `fallback_cache`).
- `State`/`ActionSeq` are per-iteration; each worker draws its own.
- RNG: the loop uses `child(cfg.train_seed, "iter{t}")` — deterministic
  per iteration index `t`. Under Hogwild workers run out-of-order, so
  "iteration t" is ambiguous. Standard approach: give each worker its
  own base seed derived from `cfg.train_seed` and `worker_id`, and draw
  iterations from a per-worker counter.
- Averaging weights: `w_t = averaging_weight_gamma(t, total_iters, ...)`.
  Under Hogwild, "t" for averaging purposes should be a monotonic global
  counter (atomic), not the worker's local index.

## Design (minimum viable)

1. **Worker pool.** Spawn `threads` workers with
   `std::thread::scope`. Each gets:
   - a clone of `Arc<RegretTable>` (needs `Arc`, not `&mut`, at the
     call site — currently the loop passes `&mut table`)
   - its own `Encoder` (clone of the config + buckets)
   - its own RNG stream seeded `child(cfg.train_seed, "worker{id}")`
   - its own local iteration counter, starting at `id`
2. **Global iteration counter.** `Arc<AtomicU64>` incremented at the
   top of each worker's iteration; used for the averaging weight so the
   weight schedule is independent of worker interleaving.
3. **Termination.** Each worker runs until the global counter reaches
   `total_iters`. The main thread joins and runs the snapshot cadence
   serially (snapshots must not race).
4. **Determinism contract.** Hogwild is *non-deterministic by design* —
   that is the standard tradeoff (Hogwild! Niu et al. 2011). The
   existing `Deterministic` mode must remain bit-identical; only
   `Hogwild` / `Snapbatch` become parallel.

## Effort estimate

- Change `&mut RegretTable` → `Arc<RegretTable>` in the loop: ~20 lines.
- Worker pool + scope: ~80 lines.
- Per-worker Encoder clone + RNG derivation: ~30 lines.
- Snapshot serialization after join: ~20 lines.
- Fixing the `regret_add_cfr_plus` non-CFR+ path (already atomic) and
  ensuring `entry_or_insert` is safe under concurrent insert (it uses
  `&mut self` today — needs an atomic lock or a two-phase
  check-then-insert): **this is the riskiest part.**

The `entry_or_insert` concurrency question is the real work: the current
signature takes `&mut self`, so it cannot be called from multiple threads
even if the underlying storage is atomic. Options:
- Pre-allocate rows in a single-threaded warmup phase, then Hogwild
  writes only touch existing rows (safe).
- Or add a coarse RwLock around insert only (reads still lock-free).

**Pre-warming is the cleaner path** and is what the existing
`train_from_warm` snapshot path effectively does.

## Recommendation

Implement in this order:
1. **Warmup phase.** Single-threaded first K iterations to grow the
   table; snapshot. (Already exists via `resume_from`.)
2. **Hogwild phase.** Spawn workers that only write to existing rows.
   Rows that appear during Hogwild (new infosets) are written through a
   lock-guarded `entry_or_insert_slow`.
3. **Benchmark.** Measure wall-time speedup at 4 and 8 workers on the
   tiny abstraction at 5M iters. If speedup ≥ 3.5×, ship as the
   `--threads` path.

This is a 2-3 hour job, not a 10-minute one. It is also the only path
that makes the full-abstraction experiment affordable.

## Interim

Until this lands, every experiment runs single-threaded. The 30-hour
full-abstraction run is on the table but is currently a
manual-overnight commitment.
