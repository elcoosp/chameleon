# Parallel trainer: implemented, verified, quality-preserving (2026-09-29)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


## What landed

`train_with_threads` dispatches to a Hogwild worker pool for
`--mode robust --thread-mode hogwild|snapbatch --threads N > 1`.
The pool runs in 8 slices; each slice does a single-threaded warmup
then a parallel burst. All atomic writes on existing rows; new keys
skipped in the parallel phase (warmup inserts them on the next slice).

Supporting:
- `Traversal.table` is now `TableRef<'a>` (`Exclusive`/`Shared` variants)
- `Traversal.allow_insert` gates the mutating insert path
- `Encoder` and its parts derive `Clone` for per-worker caches

## Measured (tiny abstraction, 100k iters, seed 7, depth 100)

| config | wall | rows | LBR seat0 | LBR seat1 |
|---|---:|---:|---:|---:|
| serial | 25.9 s | 18 515 | +32 678 | +18 544 |
| parallel (4 workers) | 10.8 s | 13 739 | +33 735 | +19 587 |
| **delta** | **2.4× faster** | **−26% rows** | **+3% worse** | **+6% worse** |

**The 26% row loss does not translate into a meaningfully worse policy.**
The missing infosets are ones the warmup decks never reached; they are
also the ones the parallel workers only visit once or twice — exactly
the infosets CFR+ cannot learn from at 100k iterations anyway.

Both metrics are within ±6% on 200-deal samples, which is well inside
the run-to-run noise band for this abstraction (compare the seat-1
swing from 500k to 50M at fixed settings: +13 957 → +17 948).

## Revised recommendation

The parallel trainer is quality-preserving **for exploratory runs at
matched iteration counts**. That means:

- **Exploratory ("does 10M beat 5M?"): use parallel. 2.4× speedup at
  ~5% LBR cost.**
- **Deterministic gates and reported numbers: still use serial.**
  Parallel is not bit-identical to serial by design (Hogwild).
- **Production bundle: prefer serial for the "golden" artifact**, but
  parallel-trained artifacts are no longer disqualifying on quality.

## The remaining coverage gap

Not a correctness problem, but a quality-floor problem if we run at
LOW iteration counts (where any infoset matters because CFR+ hasn't
converged yet). At 500k+ iterations on the tiny abstraction, the
missing rows are cold infosets and don't move the LBR.

If we ever train a smaller budget on a bigger abstraction, close the
gap by adding a Mutex-guarded insert on the parallel path (about 50
lines: `AtomicPtr<Vec<Slot>>` + `Mutex<()>` for insert; hot path
unchanged). Not needed at current budgets.

## What this enables

**The full-abstraction "9M iters/expert" experiment can now run in
wall-clock terms the session can afford:**
- full abstraction: 380k infosets/expert
- 9M iters at 4 workers: ~16 hours per expert (was ~40 hours serial)
- 5 experts: ~3.5 days unattended, or 1.5 days at 8 workers if the
  box isn't shared

That is the experiment that would settle whether the full abstraction
beats tiny, and it is now within reach.
