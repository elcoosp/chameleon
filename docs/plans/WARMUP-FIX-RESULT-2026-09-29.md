# Warmup-fix result at 20M (2026-09-29)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


The parallel trainer's warmup used to run full CFR+ updates on rows it
already had from previous slices, overwriting accumulated strategy mass.
Commit 1fa3762 made warmup insert-only. This is the first 20M LBR run
with that fix.

| 20M variant | seat 0 | seat 1 | mean |
|---|---:|---:|---:|
| pre-fix, no eps (1000 deals) | 13 319 | 14 706 | 14 012 |
| **warmup-only fix** (1000 deals) | 13 554 | **14 090** | **13 822** |

Seat 1 improves by 4.2 %; seat 0 is essentially flat (−1.8 %). The mean
improves 1.4 %.

## Reading

The warmup fix is real but small. It removes one of the mechanisms that
degrades BB at high iteration counts, but it does not undo the RM+
freeze — the frozen iterate still dominates the average at 20M. If the
freeze is the primary cause (as the 500k → 20M → 50M soft-row collapse
suggests), then this 4 % is the warmup-overwrite contribution and the
remaining 30 %+ of BB's regression is the freeze.

That is exactly what the eps=0.02 20M run (PID 27970, in flight) will
test: if seat 1 at 20M with the exploration floor lands near the 5M
peak (~12 000), the freeze is the dominant mechanism. If it lands at
~13 500, both mechanisms matter similarly.

## Housekeeping

Both fixes (insert-only warmup, exploration floor) are compatible and
can be enabled together. The warmup fix is unconditional (it's just how
the parallel trainer works now). The exploration floor is opt-in via
CHAM_TRAIN_EPS.
