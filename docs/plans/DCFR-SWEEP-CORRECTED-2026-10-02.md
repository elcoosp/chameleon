# DCFR sweep under the corrected metric (2026-10-02)

First honest DCFR A/B: proper slice discount (F5) + corrected tabular BR,
both seats, budget 5000/500/30.

## Results

| policy | iters | seat0 clairv | seat0 tab | seat1 clairv | seat1 tab | sum |
|---|---:|---:|---:|---:|---:|---:|
| CFR+ (α=β=1) | 5M | 14.801 | -2.031 | 14.205 | +0.581 | **-1.450** |
| DCFR(1.5, 0, γ=2) | 5M | 15.327 | -1.653 | 14.546 | +0.955 | **-0.698** |
| DCFR(1.5, 0, γ=default) | 20M | 13.991 | -1.744 | 15.857 | +0.253 | **-1.491** |

(the CFR+ 5M row is the fresh `par-f5-tiny-5000000`; the DCFR rows are
`par-f5-tiny-dcfr15-g2` and `par-f5-tiny-dcfr15`.)

## What it says

1. **DCFR(1.5, 0, γ=2) at 5M beats CFR+ at 5M on the corrected metric**:
   sum -0.698 vs -1.450. Both seats improve on the tabular BR
   (seat0 -2.031 → -1.653, seat1 +0.581 → +0.955). This is the first
   positive DCFR result — and it required all three fixes (proper F5
   discount, the γ=2 half via `CHAM_AVG_DELAY=0`, and the corrected
   metric) to appear.

2. **But the clairvoyant metric is slightly worse for DCFR γ=2**
   (seat0 15.327 vs 14.801). The two metrics disagree on which policy
   is "better" — the corrected metric prefers DCFR, the clairvoyant
   prefers CFR+. This is exactly why the corrected metric matters:
   the clairvoyant number was driving every prior DCFR decision, and
   it was pointing the wrong way.

3. **DCFR without γ=2 (default γ) at 20M does not beat CFR+ 5M** on the
   corrected sum (-1.491 vs -1.450 — a tie within noise). The γ=2 half
   is load-bearing.

4. **The residual is small.** Every corrected sum here is within ~1.5 bb
   of zero. At this budget and abstraction the policies are all close
   to unexploitable; the differences are second-order. That is itself
   the headline: the tiny abstraction's policies are much closer to
   Nash than the clairvoyant numbers ever suggested.

## Caveats

- One seed (7). These are point estimates, not distributions.
- Budget 5000/500/30; the tabular BR is budget-sensitive (see
  `TABULAR-BR-CONVERGENCE-2026-10-01.md`). Direction is robust; exact
  values are not.
- The DCFR γ=2 arm ran on the post-20:20 binary; the F5 slice discount
  is exercised. Verified: `provenance.json` now records `dcfr_alpha`,
  `dcfr_beta`, `avg_gamma`, `avg_delay_override` (bug-hunt-adjacent
  change `5039315`).

## Recommendation

- **Retrain the shipped bundle with DCFR(1.5, 0, γ=2).** It is the best
  corrected-metric policy measured so far, and the shipped bundle is
  ~15 bb exploitable (see `SHIPPED-BUNDLE-EXPLOITABILITY-2026-10-02.md`).
- **Do not trust the clairvoyant metric to rank DCFR arms.** It ranked
  γ=2 worse while the corrected metric ranked it better.
- Multi-seed confirmation is the natural next step before committing to
  the DCFR schedule as the training default.
