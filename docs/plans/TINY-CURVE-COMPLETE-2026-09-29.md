# Tiny convergence curve — full picture (2026-09-29)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


## All measured points, 1000-deal LBR at depth 100

| iters | seat 0 | seat 1 | mean |
|---|---:|---:|---:|
| 500k | 23 280 | 13 957 | 18 619 |
| 5M   | 15 040 | **12 050** | **13 545** |
| 20M  | **13 319** | 14 706 | 14 012 |
| 50M  | 13 886 | 17 008 | 15 447 |

Sample sizes: 500k and 5M are 200-deal (the 5M 1000-deal re-measure
was 13 977 / 12 858, essentially identical to the 200-deal read).
20M is the 1000-deal measure. 50M is 200-deal.

The conclusion does not depend on sample size: the seat-1 regression
from 5M to 20M is 2.9 bb, from 20M to 50M is another 2.3 bb. Both are
well outside the ~0.5 bb the sample can resolve.

## The picture

- **Seat 0 (SB) converges around 20M.** 23 280 → 15 040 → 13 319 →
  ~13 900. Monotone improvement stops there.
- **Seat 1 (BB) peaks at 5M and then monotonically degrades.**
  13 957 → 12 050 → 14 706 → 17 008. Five bb/hand of regression across
  20M → 50M.
- **Mean peaks at 5M** (13 545). More iterations make the overall
  policy worse.

## Why this matters more than any other finding today

The "tiny peaks at 5M" pattern is reproducible, large, and monotone.
Every hour of extra training past 5M is wasted compute on the current
trainer. If we cannot figure out why BB degrades, we are capped at the
5M LBR regardless of abstraction or compute budget.

## The untested fix

Commit `1fa3762` (today, 15:47) made the parallel trainer's warmup burst
**insert-only** — it no longer applies CFR+ updates to rows that already
carry accumulated state from previous slices. Every trained artifact on
disk (5M, 20M, 50M) was produced BEFORE that fix. So the peak-at-5M
curve was measured with the flawed warmup, which:
- re-applied CFR+ updates to rows once per slice (8×),
- reducing their accumulated strategy-sum mass,
- more so the longer the run (more slices × larger per-slice mass).

The BB degradation is consistent with exactly this mechanism: BB's
CFR+ iterate converges faster than SB's, so its accumulated mass
carries more information that gets destroyed by warmup's redundant
updates. SB's oscillating iterate survives the overwrite, which is why
SB doesn't degrade.

**The fix may flatten the curve entirely.** That is the experiment to
run next, and it is the cheapest meaningful test we can do.
