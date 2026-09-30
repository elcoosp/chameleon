# 19-dim honest router: gate PASSES with temperature scaling (2026-09-30)

**UPDATE 15:16:** the ECE gap is closed. Temperature scaling (grid-searched
on B-dev, `T > 1` to soften the softmax) dropped ECE from 0.173 → 0.110
without changing top-1 or recall (temperature scaling is monotone on the
logits, so argmax decisions are preserved). The gate now **PASSES**:

| feature set | top-1 B-test | TAG | LAG | ECE | gate |
|---|---:|---:|---:|---:|---|
| 20-dim opportunity-gated (leaky) | 0.697 | 0.375 | 0.514 | 0.201 | FAIL |
| 10-dim raw opponent | 0.797 | 0.515 | 0.584 | 0.363 | FAIL |
| 11-dim + tilt | 0.766 | 0.517 | 0.503 | 0.305 | FAIL |
| 19-dim + bet-size histogram (raw) | 0.906 | 0.825 | 0.995 | 0.173 | near-pass |
| **19-dim + calibration** | **0.906** | **0.825** | **0.995** | **0.110** | **PASS** |

This is the **first honest router to pass the gate** in this project.
See the original "near-pass" analysis below.

## The result

Trained on 120 000 rows collected by
`collect --real --raw-opponent-19` (10 raw frequencies + 8 postflop
bet-size histogram buckets + 1 preflop/postflop tilt). The bet-size
histogram is the key new signal.

| feature set | top-1 B-test | NIT | TAG | LAG | STATION | ECE |
|---|---:|---:|---:|---:|---:|---:|
| 20-dim opportunity-gated (leaky) | 0.697 | — | 0.375 | 0.514 | — | 0.201 |
| 10-dim raw opponent | 0.797 | 0.711 | 0.515 | 0.584 | 0.979 | 0.363 |
| 11-dim + tilt | 0.766 | 0.742 | 0.517 | 0.503 | 0.984 | 0.305 |
| **19-dim + bet-size histogram** | **0.906** | **0.811** | **0.825** | **0.995** | **0.997** | **0.173** |
| gate | ≥0.80 ✓ | ≥0.70 ✓ | ≥0.70 ✓ | ≥0.70 ✓ | ≥0.70 ✓ | ≤0.15 ✗ |

**The 19-dim router passes every gate except ECE.** ECE is 0.173,
which is 15% above the 0.15 threshold. Everything else is
comfortably above: top-1 at 0.906 (13% margin), TAG recall at 0.825
(18% margin), LAG at 0.995 (42% margin).

## What the bet-size histogram captures

The design doc predicted the signal. Concretely:

- Nit cbets 62% pot
- **TAG cbets 66% pot**
- **LAG cbets 100% pot**
- Station cbets 33% pot

The 8-bucket histogram resolves the 0.33 / 0.66 / 1.0 clusters
(0.25-wide bins). The classification jump from 0.515 (TAG, 10-dim)
to 0.825 (TAG, 19-dim) is directly attributable to the bet-size
dimension.

## Why ECE fails

ECE (Expected Calibration Error) measures how well the model's
confidence matches its accuracy. A model can be highly accurate but
poorly calibrated: if it predicts class X with 90% confidence but is
only right 70% of the time, ECE is high.

The jump in accuracy (0.797 → 0.906) came with a reduction in ECE
(0.363 → 0.173) but not enough. The remaining gap is a *calibration*
problem, not a *signal* problem.

**Calibration fixes (not attempted):**

1. **Temperature scaling** — divide the logits by a learned scalar
   T > 1 to soften the distribution until ECE is minimized. This is
   a one-parameter post-hoc fix that preserves argmax decisions.
2. **Platt scaling** — per-class logistic calibration on a held-out
   set.
3. **Label smoothing** during training — mixes the target with a
   uniform distribution, reducing overconfidence.
4. **More data** — ECE typically improves with sample size.

Any of these can plausibly drop ECE below 0.15 without affecting
top-1 accuracy.

## What this unlocks

The 19-dim router is the **first honest feature set that separates
TAG from LAG**. This was the session's second-highest-priority open
problem (after the freeze, which is now closed).

With ECE fixed, the router becomes shippable as the mixture's
classifier. The mixture's +4 388 ladder mean (vs argmax's +7 136)
was the mixture's cost; a working router should narrow or eliminate
that gap.

## Next steps

1. **Add temperature-scaling calibration** to `train-router`. This is
   a ~30-line change: after SGD, fit `T` on the B-dev set, report the
   calibrated ECE. If the calibration is built into `SoftmaxModel`,
   the argmax decisions don't change but ECE drops.
2. **Measure the calibrated model's gate.** If ECE drops below 0.15,
   the gate passes for the first time in the session.
3. **Install the calibrated 19-dim router into a new bundle** and run
   `ladder --fast --agent full-mixture`. Compare to the +4 388
   baseline. If the ladder improves, the mixture becomes the new
   frontier.

## The full router search

| feature set | top-1 | TAG | LAG | ECE | gate |
|---|---:|---:|---:|---:|---|
| 20-dim opportunity-gated (leaky) | 0.697 | 0.375 | 0.514 | 0.201 | FAIL |
| 10-dim raw opponent | 0.797 | 0.515 | 0.584 | 0.363 | FAIL |
| 11-dim + tilt | 0.766 | 0.517 | 0.503 | 0.305 | FAIL |
| **19-dim + bet-size histogram** | **0.906** | **0.825** | **0.995** | **0.173** | **near pass** |

The trajectory is unambiguous: aggregate frequencies don't separate
TAG/LAG (10-dim, 11-dim), but bet sizes do (19-dim).

## Artifacts

- `artifacts/router_raw_19.rbin` (120 000 × 19 features)
- `artifacts/collect-raw-19.log`
- `artifacts/train-router-raw-19.log`
- `artifacts/routers/v19/`

## Related

- `ROUTER-BET-SIZE-FEATURE-DESIGN-2026-09-30.md` — the design
- `ROUTER-11DIM-NEGATIVE-2026-09-30.md` — the negative that preceded
- `ROUTER-RAW-FEATURES-RESULT-2026-09-29.md` — the 10-dim baseline
- `ROUTER-TRAINING-GAP-2026-09-28.md` — the synthetic stub problem
