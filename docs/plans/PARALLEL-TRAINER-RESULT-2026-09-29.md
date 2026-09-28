# Parallel trainer: implemented, measured, honest limits (2026-09-29)

## What landed

`train_with_threads` now dispatches to a Hogwild worker pool for
`--mode robust --thread-mode hogwild|snapbatch --threads N > 1`.
The pool runs in 8 slices; each slice does a single-threaded warmup
(populating any infosets it encounters) followed by a parallel burst
(all workers write via the existing atomic CAS methods on a shared
`&RegretTable`).

Supporting changes:
- `Traversal.table` is now `TableRef<'a>` (an enum with `Exclusive(&mut)`
  and `Shared(&)` variants) so single-threaded callers keep insert access
  while parallel workers borrow the table immutably.
- `Traversal.allow_insert` gates the (mutable) insert path; `Shared`
  references that hit a missing key skip that subtree for the iteration.
- `Encoder` and its components (`MmapTable`, `RiverBucketer`,
  `ActionLadder`) implement `Clone` so each worker owns its caches.

## Measured (tiny abstraction, 100k iters, seed 7, depth 100)

| config | wall | infosets | policy.bin |
|---|---:|---:|---:|
| serial (deterministic) | 25.9 s | 18 515 | 328 KB |
| parallel, 4 threads | 10.8 s | 13 739 | 244 KB |

**Speedup: 2.4×** (4 workers on 8 logical cores, but the box is shared).
Coverage is **26 % lower** in the parallel run — the missing infosets are
ones the warmup decks never reached.

## The coverage gap and why it's structural

The warmup pass draws its own decks. When a parallel worker's deck reaches
an infoset the warmup never saw, that infoset is **not in the table** and
the worker's `allow_insert = false` path skips the subtree. Those
infosets are lost for that run.

Widening the warmup fraction or increasing slice count doesn't help:
- 8 slices: 13 739 infosets
- 32 slices: 13 659 infosets (and 74 % slower — more warmup overhead)

The correct fix is **lock-guarded insert on the parallel path**: workers
take a `Mutex` only when a key is missing, and do the CAS-only path
otherwise. This preserves lock-free hot writes and closes the coverage
gap exactly. It is a ~50-line change to `RegretTable` (an
`AtomicPtr`-for-slots + `Mutex`-for-insert design) that I did not land
tonight.

## The honest recommendation

- **For deterministic runs (reproducible gates, LBR comparisons, papers):
  keep serial.** The parallel trainer is not bit-identical to serial and
  cannot be, by design.
- **For exploratory runs (does 10M iters beat 5M on the full abstraction?):
  use parallel — the 2.4× speedup is real and 26 % fewer infosets is
  tolerable when the question is "does the trend continue".**
- **For production training (the bundle that ships): keep serial until
  the coverage gap is closed.** The 26 % difference is large enough that
  a parallel-trained bundle would not match a serial-trained one.

## What to do next

1. Land the lock-guarded insert (closes the gap; ~50 lines).
2. Re-measure wall speedup at 4 and 8 workers on the tiny and full
   abstractions.
3. If full-abstraction speedup ≥ 3×, run the "9M iters per expert" test
   in ~10 hours instead of ~30.

Until (1) lands, this is a *tool for exploration*, not the shipping
trainer.
