# Warmfix at 5M: BB improves 184, SB worsens 671, mean worse by 244 (2026-09-29)

## The result

The insert-only warmup fix (commit 1fa3762) was already measured at 20M
(see `WARMUP-FIX-RESULT-2026-09-29.md`: BB −616, SB +235, mean −190 —
a net win). The 5M side had never been measured. Result:

| variant | SB | BB | mean | wall |
|---|---:|---:|---:|---:|
| **5M no-fix** | 13 977 | 12 858 | **13 417** | 47 min (serial) |
| **5M warmfix** | 14 648 | 12 674 | 13 661 | ~10 min (parallel) |
| **Δ** | **+671** | **−184** | **+244** | — |

1000-deal LBR, depth 100, seed 7, tiny robust.

Wall is not comparable to the "47 min" 5M number: that was the
pre-parallel serial run. The warmfix 5M ran on the parallel trainer
(~10 min at 4 workers).

## The pattern

Warmfix helps the side that needs mixing (BB) and hurts the side that
benefits from sharpening (SB). Both effects are directionally consistent
with the RM+ freeze model:

- **BB's equilibrium needs mixing.** Warmfix preserves more of the
  pre-freeze mixed iterate, so BB benefits.
- **SB's equilibrium is closer to pure.** Its sharpening was making SB
  better, and warmfix's insert-only discipline (no CFR+ overwrite of
  accumulated strategy mass on rows visited during warmup) slightly
  disturbs that sharpening.

The two sides of the fix are therefore **asymmetric in the same
direction as the freeze itself**: BB gets better at the cost of SB.

## Comparison with 20M

| iters | Δ SB | Δ BB | Δ mean |
|---|---:|---:|---:|
| 5M  | **+671** | −184 | +244 |
| 20M | +235 | **−616** | −190 |

At 5M, warmfix is a net loss on mean (the SB cost dominates). At 20M,
warmfix is a net win (the BB recovery dominates).

## Implication

**Warmfix should not be the default for short runs.** It only pays for
itself when the freeze is the dominant constraint, which at tiny
abstraction is roughly 10M+ iters.

This strengthens the case that the freeze and the warmup discipline are
coupled: warmup matters when the freeze exists, not before.

## Artifacts

- `artifacts/par-5M-warmfix/robust-7/policy/policy.bin`
- `artifacts/par-5M-warmfix/robust-7/table.snap`
- `artifacts/par-5M-warmfix-lbr.log`
- `artifacts/par-5M-warmfix-lbr.stderr.log`
- `artifacts/par-5M-warmfix-lbr.criterion.log`
- `artifacts/par-5M-warmfix.log`

## Note on the M-6 fix (landed da90405)

The 5M warmfix was trained with the stale binary — this was BEFORE
commit `da90405` landed, so its provenance still records `threads: 1`
even though the parallel trainer ran. Future runs (the queued
freeze-diag, the running dcfr-alpha) will record the actual pool size.

## Related

- `WARMUP-FIX-RESULT-2026-09-29.md` — the 20M version
- `AVG-DELAY-DELAY0-RESULT-2026-09-29.md` — same directional pattern
  (mixed-iterations-average helps BB, hurts SB)
- `RM-PLUS-FREEZE-2026-09-29.md` — the mechanism
