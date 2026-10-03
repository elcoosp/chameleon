# Rich ladder: 40x infoset explosion — 5M iters is 1.5 visits/infoset (2026-10-03)

**Finding:** `abstraction-tiny-rich.toml` produces **3,196,020 infosets**
(vs tiny's 80,617) — a **~40x** blow-up, not the ~3x the "11 vs 6
distinct sizes" count suggested. At the retrain's 5M iterations that is
**1.56 visits per infoset**.

## The numbers

| abstraction | infosets | iters | visits/infoset |
|---|---:|---:|---:|
| tiny | 80,617 | 5,000,000 | **62.0** |
| rich | 3,196,020 | 5,000,000 | **1.56** |

To match tiny's 62 visits/infoset, rich would need **~198M iterations**.

## Why 40x and not 3x

Distinct *sizes per street* is 11 vs 6 (~2x). But the infoset space is
over action **histories**, which grow combinatorially with
(sizes × raises-cap)^street. Rich has `raises_per_street_cap = 2` (tiny
has 1) and 2-3 bet fracs per street, so the sequence space multiplies
per street, not adds. ~40x is the honest consequence.

## What this means for the retrain

The retrain (running now) is **not** the report's gate. The report's
Phase-2 gate is:

> Corrected BR of rich-tiny < tiny at **matched visits/infoset**.

At 1.56 vs 62 visits/infoset, this run is **not matched**. It cannot
confirm or refute the gate. What it CAN answer is a weaker, confounded
question:

> Does rich@5M (≈60 min wall) beat tiny@5M (≈67 min wall) at **similar
> wall time**, despite being 40x undertrained?

If yes, rich is promising (it wins while starving). If no, the result
is uninformative about rich's ceiling — we only learn 5M is too few.

## Consequences for the F6c roadmap

1. **A full rich retrain is infeasible as-is.** 198M iters × the rich
   per-iter cost is weeks, not hours.
2. **The rich config needs shrinking.** A ladder with 2 bet sizes per
   street and `cap = 1` would cut the history space ~an order of
   magnitude. Target: infosets within ~3-5x tiny so 5-20M iters gives
   10-60 visits/infoset.
3. **The slot-bucket payoff is bounded by infoset count.** More sizes
   only help if the policy can be trained to use them; at 1.5
   visits/infoset it cannot.

## Recommendation

- Let the current run finish (it gives the wall-time data point) but
  **label its result wall-time-confounded, not visits-matched**.
- Before any further rich retrain, **design a "rich-lite" ladder**:
  2 sizes/street, cap 1, and re-measure infoset count. The whole point
  is size resolution *that the iteration budget can train*.

Source: `artifacts/blueprints-f6c-rich/robust/robust-7/provenance.json`
(infosets: 3196020), vs `artifacts/blueprints-retrain-19dim/...`.

## Measured: rich-lite fixes it (100k-iter comparison)

| config | infosets @100k | wall | ms/iter | projected 5M retrain |
|---|---:|---:|---:|---:|
| rich | 275,710 | 705s | 7.0 | **~49h** |
| rich-lite | 52,193 | 45.7s | 0.46 | **~3.2h** |
| tiny | 32,054 | 32.8s | 0.33 | ~2.7h |

`config/abstraction-tiny-rich-lite.toml`: 2 bet sizes per street, raise
cap 1 (vs rich's 3 sizes, cap 2). It is **1.6x tiny's infosets** at
100k iters (vs rich's 8.6x) and **15x faster per iteration**.

**Conclusion:** rich-lite is the config to use. The rich retrain
launched 2026-10-03 is infeasible (~49h) and should be replaced.
