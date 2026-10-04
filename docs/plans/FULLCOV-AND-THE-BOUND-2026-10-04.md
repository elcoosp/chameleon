# Full-coverage buckets + the zero-sum bound correction (2026-10-04)

## Results

| experiment | result |
|---|---|
| rich-lite bundle, 9-opponent ladder | mean **6800** vs tiny **8365** => **-1565** (worse) |
| full-coverage robust, corrected BR | **+9.47** (s0 +4.24, s1 +5.24) |
| fine-information BR vs same-abstraction | 5.41 vs 9.47 (fine is LOWER) |
| full-coverage retrain wall | 344s vs ~1135s sampled (**3x faster**) |
| infosets | 190057 vs 190158 (unchanged) |

## The correction: negative BR sums were under-convergence, not "unexploitable"

Two-player zero-sum implies, for ANY policy sigma:

    BR(0) + BR(1) >= u0(sigma,sigma) + u1(sigma,sigma) = 0

So a converged tabular BR must **sum to >= 0**. Every result this
session read as "~0 unexploitable" actually had a NEGATIVE sum:

| policy | sum |
|---|---:|
| tiny CFR+ 5M | -1.45 |
| retrained robust | -1.29 |
| rich-lite sampled | -0.40 |

All **violate the bound** => the BR learner was under-converged (it
failed to even match the trivial sigma-vs-sigma response). "Negative
sum" means "learner failed", NOT "policy is unexploitable".

Only two measurements are bound-consistent:
- shipped old-trainer bundle: +15.43 (genuinely exploitable)
- **full-coverage robust: +9.47** (first bound-consistent NEW number)

## What this means

1. **The "we're at ~0 exploitability" story was an artifact.** Every
   negative-sum reading (mine included, in
   `CORRECTED-METRIC-LEADERBOARD-2026-10-02.md`) misread a
   non-converged learner as a strong policy.
2. **+9.47 is the first believable number**: the full-coverage policy
   is exploitable for ~9.5 bb *within its own abstraction*. Either the
   correct buckets make the abstraction harder to solve (more meaningful
   infosets at fixed 5M iters), or the BR finally converged.
3. **Fine-BR < same-BR** contradicts the audit's §3.3 prediction. Most
   likely a **learner-budget artifact**: the fine key (300/200/64/8) has
   ~10x the infosets, so at the same 5000 train deals the fine BR is
   starved and its learned choice is worse. Not a real signal until the
   fine BR gets ~10x the train deals.
4. **Full coverage is 3x faster** (no 1326-combo fallback per miss) — a
   genuine perf win, and it makes the keys honest (no suit-blind fallback).

## Rich-lite: negative

The richer betting tree (2 sizes/street) **loses 1565 on the ladder**
vs tiny. Combined with its 26 visits/infoset (vs tiny's 62), it is
under-trained at 5M. **Do not promote rich-lite.**

## Next (correctness-first)

1. **Gate every BR on `sum >= 0`.** A negative sum is a failed
   measurement, not a result. Re-run the key comparisons with enough
   `train_deals` for the learner to converge.
2. **Give fine-BR ~10x train deals** before reading its ratio.
3. **Re-measure tiny vs rich-lite vs DCFR on full-coverage buckets**
   (correct keys) at the same budget, checking the bound each time.
