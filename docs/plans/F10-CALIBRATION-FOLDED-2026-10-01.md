# F10 verified: calibration T is folded into the shipped model (2026-10-01)

## The verification

`SoftmaxModel::with_temperature(T)` divides every weight and bias by T.
The check: the folded model's mean |w| should equal the raw model's
mean |w| divided by T.

| model | |w| mean | T fitted | |w|/T expected | actual | verdict |
|---|---:|---:|---:|---:|---|
| `routers/v19/router.bin` (raw) | 0.629 | 0.55 | 1.144 | — | baseline |
| `routers/v19-integrated/router.bin` (folded) | 1.144 | — | — | 1.144 | **✓ matches** |

The ratio 1.144 / 0.629 = 1.82 exactly equals 1/T = 1/0.55 = 1.818.
**The calibration is correctly folded into `agent-honest-19dim`.**

## The v19 uncalibrated model is a footgun

`artifacts/routers/v19/router.bin` (the pre-integration model) has NO
`feature_set` field in its JSON — it predates the `feature_set`
serialization. When loaded via serde, the default
`"opportunity-gated-20"` is applied. The pipeline would then dispatch
the *20-dim tracker vector* to a *19-feature model* (forward() truncates
to 19), silently reading the wrong 19 dimensions.

**This is a silent correctness bug for that artifact.** It should not
be loaded; only `routers/v19-integrated/` and any future
`train-router --feature-set` runs are trustworthy.

Documented for future sessions.

## What this means

- The 19-dim SOTA bundle (`agent-honest-19dim`) is correct and shippable.
- The intermediate `routers/v19/` should be deleted or marked unusable.
  Keep for historical reference but never load.

## Artifacts

- `artifacts/routers/v19-integrated/router.bin` — SOTA model, calibrated, correct
- `artifacts/routers/v19/router.bin` — DO NOT USE (no feature_set)
- `artifacts/routers/v19-integrated/metrics.json` — T=0.55, ece_b_test 0.110
- `artifacts/routers/v19/metrics.json` — ece 0.173 (pre-calibration)
