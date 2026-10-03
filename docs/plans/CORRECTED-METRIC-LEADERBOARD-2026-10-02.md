# Corrected-metric leaderboard (2026-10-02)

Every corrected (`tabular_br`) measurement from the 2026-10-01/02 sessions,
in one place. **Read the budget column** — the tabular BR is
budget-sensitive (`TABULAR-BR-CONVERGENCE-2026-10-01.md`); numbers at
different budgets are not directly comparable.

## Both-seat measurements (budget 5000/500/30)

`BR(0)+BR(1) < 0` = within the learner's residual gap of unexploitable;
`> 0` = a real exploiter exists. The sum is the honest exploitability.

| policy | seat0 tab | seat1 tab | **sum** | clairv s0/s1 |
|---|---:|---:|---:|---|
| **shipped `agent-honest-19dim/robust`** | +11.20 | +4.23 | **+15.43** | 35.4 / 17.4 |
| `par-f5-tiny-5000000` (CFR+ 5M) | -2.03 | +0.58 | **-1.45** | 14.8 / 14.2 |
| `par-f5-tiny-dcfr15-g2` (DCFR γ=2, 5M) | -1.65 | +0.96 | **-0.70** | 15.3 / 14.5 |
| **retrained `robust` (DCFR 1.5,0,γ2)** | -2.28 | +0.99 | **-1.29** | 15.3 / 14.5 |
| `par-f5-tiny-dcfr15` (DCFR, 20M) | -1.74 | +0.25 | **-1.49** | 14.0 / 15.9 |
| **`f6c-lite` robust** (2 sizes/street, slot bucket) | -1.72 | +1.33 | **-0.40** | 16.5 / 15.9 |
| `medium-20M` (64/32/32, **OLD trainer**) | +10.70 | +11.63 | **+22.34** | 33.0 / 26.1 |


> **2026-10-03 additions.** `f6c-lite` (rich-lite ladder + slot bucket) is
> the new best corrected-sum policy at **-0.40**. The `medium-20M` row is
> a **stale artifact** (trained 2026-09-29, pre-F3/F4/F6a) — its +22.34
> reflects the trainer gap, not the bucket count; do not use it to judge
> buckets. See `LEVER-COMPARISON-2026-10-03.md`.

## Single-seat measurements (budget 300/200/12 — lower budget, do not compare)

These are the overnight-curve seat-1-only numbers, at the old budget.
Recorded for provenance; the both-seat table above supersedes them.

| policy | iters | seat1 clairv | seat1 tab |
|---|---:|---:|---:|
| tiny CFR+ | 500k | 20.48 | -1.59 |
| tiny CFR+ | 5M | 14.21 | -3.30 |
| tiny CFR+ | 20M | 15.96 | -1.94 |
| tiny DCFR(1.5,0) | 20M | 15.86 | -1.55 |
| medium CFR+ | 20M | 15.72 | -3.02 |

## Headlines

0. **The retrained robust (DCFR 1.5/0/γ2) measures -1.29 bb**, down from
   the shipped robust's +15.43 — a 16.7 bb swing, landing in the same
   range as the fresh policies. First confirmation that the retrain
   fixes the shipped bundle. (Measured on the robust arm before the
   experts finished; `retrain-2026-10-02/early-robust-metric.txt`.)

1. **The shipped bundle is ~15 bb exploitable; the fresh tiny policies
   are ~0.** The gap is the F3/F4/F6a trainer fixes, measured. Retrain
   the bundle. (`SHIPPED-BUNDLE-EXPLOITABILITY-2026-10-02.md`)
2. **DCFR(1.5, 0, γ=2) at 5M is the best corrected-metric policy**
   (sum -0.70). (`DCFR-SWEEP-CORRECTED-2026-10-02.md`)
3. **The clairvoyant metric ranks differently** — it prefers CFR+ 5M over
   DCFR γ=2, while the corrected metric prefers DCFR. Do not use the
   clairvoyant metric to rank policies.
4. **Tiny peaks at 5M, not 20M** (clairvoyant 14.2 → 16.0). More
   iterations hurt on tiny. (`OVERNIGHT-CORRECTED-STACK-RESULTS-2026-10-01.md`)

## Budget caveat (repeat, because it matters)

At budget 300/200/12 the sum `BR(0)+BR(1)` is **-9.3** on
`par-f5-tiny-5000000`; at 5000/500/30 it is **-1.45**. The learner has
not converged at the low budget. Every number in the both-seat table is
at 5000/500/30 specifically so it can be compared; the single-seat table
is not comparable to it.

## Source logs

- `artifacts/post-overnight-2026-10-02/agent-honest-19dim.log`
- `artifacts/post-overnight-2026-10-02/dcfr-g2.log`
- `artifacts/post-overnight-2026-10-02/par5m-big-budget.log`
- `artifacts/par-f5-tiny-{500000,5000000,20000000}-metric.log`
- `artifacts/par-f5-tiny-dcfr15-metric.log`
- `artifacts/par-f5-medium-20M-metric.log`
