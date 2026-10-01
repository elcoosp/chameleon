# Tiny peaks at 5M — seat-1 regression is real (2026-09-29)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


## The 1000-deal measurement

Same artifacts, 5× the LBR sample. This settles the noise question.

| config | deals | seat 0 | seat 1 | mean |
|---|---:|---:|---:|---:|
| 5M | 200  | 15 040 | 12 050 | 13 545 |
| 5M | 1000 | 13 977 | 12 858 | **13 417** |
| 20M | 200  | 13 809 | 14 956 | 14 383 |
| 20M | 1000 | **13 319** | 14 706 | 14 012 |

## What the 1000-deal numbers say

**5M is the tiny peak.**

- SB keeps improving with iterations: 23 280 (500k) → 13 977 (5M) →
  13 319 (20M). Gains 30 % then 5 %.
- BB peaks at 5M: 13 957 (500k) → 12 858 (5M) → 14 706 (20M). Improves
  8 % then regresses 14 %.
- Mean: peaks at 5M (13 417). 20M is 0.6 bb worse.

The seat-1 regression is +1.85 bb between 5M and 20M — well above the
~0.5 bb/hand that the 1000-deal sample can resolve. It is real.

## What causes it (hypotheses, not yet diagnosed)

The distinguishing feature of tiny 20M vs tiny 5M is the size of the
strategy sum per infoset. At 20M iterations with an 8-slice parallel
trainer, each slice's warmup overwrites some of the previously
accumulated strategy mass (the warmup draws the same range as the
parallel phase; when a warmup walk sees an existing row it **reduces**
it in place via CFR+ CAS). For SB, the CFR+ iterate oscillates and the
old and new mass roughly cancel; for BB, the iterate converges faster
and old mass is more informative — overwriting it degrades BB.

Alternative: BB's postflop positioning disadvantage means the current
iterate's strategy is systematically further from equilibrium than
SB's, and the last-quarter weight (T/4 delay) is not enough to average
it out at 20M iters.

Both explanations are compatible with the data. Both point at the
averaging design as the culprit — not the CFR+ update itself.

## What this means for the roadmap

The frontier is now:

- **5M tiny robust** is the best self-contained policy: mean LBR 13 417,
  47 min wall, ships as the reference.
- **20M tiny** is *worse* on the same metric. Do not run longer on tiny.
- Medium (in flight) will tell us whether more buckets change the shape.

If medium also peaks early, the story is "tiny-equivalent abstractions
peak around 5M iterations with the current averaging, and more compute
without averaging redesign is wasted". That would make the averaging
schedule — not the abstraction, not the iteration count — the frontier.

## Recommended next experiment

Fix the parallel trainer's warmup so it does NOT touch existing rows
(warmup should only insert missing keys, not re-run CFR+ on existing
ones). This would remove the "warmup overwrite" hypothesis in one shot.
The change is small: in warmup, skip any (path, player) whose key is
already in the table.

The second experiment, if #1 doesn't help: replace T/4 delay with T/2
for the strategy sum, giving the last half of iterations equal weight
with the first half. Delayed averaging is not required by CFR+; it is a
convention that works for small problems and fails for long ones.
