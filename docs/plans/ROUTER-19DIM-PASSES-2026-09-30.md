# Honest router passes the gate for the first time (2026-09-30)

## The milestone

| feature set | top-1 B-test | TAG recall | LAG recall | ECE B-test | gate |
|---|---:|---:|---:|---:|---|
| 20-dim opportunity-gated (leaky) | 0.697 | 0.375 | 0.514 | 0.201 | FAIL |
| 10-dim raw opponent | 0.797 | 0.515 | 0.584 | 0.363 | FAIL |
| 11-dim + preflop/postflop tilt | 0.766 | 0.517 | 0.503 | 0.305 | FAIL |
| 19-dim + bet-size histogram (raw) | 0.906 | 0.825 | 0.995 | 0.173 | near-pass |
| **19-dim + temperature calibration** | **0.906** | **0.825** | **0.995** | **0.110** | **PASS** |
| gate | ≥0.80 ✓ | ≥0.70 ✓ | ≥0.70 ✓ | ≤0.15 ✓ | |

**The gate passes for the first time.** Every threshold is met
comfortably: top-1 at 0.906 (13% margin), TAG recall at 0.825 (18%
margin), LAG at 0.995 (42% margin), ECE at 0.110 (27% margin).

## What made the difference

Two independent improvements stacked:

1. **Bet-size histogram** (19-dim feature). Raw top-1 jumped from
   0.797 (10-dim) to 0.906, and TAG recall from 0.515 to 0.825. The
   raw aggregate frequencies cannot separate TAG from LAG; their bet
   sizes (66% vs 100%) can.

2. **Temperature scaling** (post-hoc calibration). ECE dropped from
   0.173 to 0.110. The raw softmax was overconfident; softening it
   matches the predicted confidence to the observed accuracy. Since
   temperature scaling is monotone in the logits, top-1 and recall
   are preserved exactly.

## The implementation

- `Tracker::opp_postflop_bet_size_hist: [u64; 8]` — 8 buckets of
  0.25-pot-fraction. Populated in `observe_hand` from the public
  action stream (bet/raise `to` amounts, pot reconstructed by walk).
  I9-safe: bet sizes are public.
- `ChameleonAgent::opponent_only_features_19` — 10 raw + 8 hist + 1 tilt.
- `collect --real --raw-opponent-19` — emits 19-dim feature vectors.
- `SoftmaxModel::logits` — raw scores, needed for temperature scaling.
- `train_model` — grid-searches T in [0.5, 5.0] on B-dev, applies T to
  the ECE computation, reports both raw and calibrated ECE.
- `TrainReport.temperature` — the fitted T.

## What this unlocks

The 19-dim calibrated router is shippable. Before this session, the
mixture path was blocked: the router couldn't separate TAG from LAG
(the two aggressive archetypes), so `--agent full-mixture` averaged
across near-random picks and lost to argmax. With the gate passing,
the mixture has a working opponent model for the first time.

## The next step

1. **Install the calibrated router** in a new bundle
   `artifacts/agent-honest-19dim`:
   - Copy `agent-honest`'s experts, robust, buckets, abstraction.
   - Replace `router.bin` with `artifacts/routers/v19-calibrated/model.bin`
     (or wherever `train-router` writes it).
2. **Measure `ladder --fast --agent full-mixture`** on the new bundle.
3. **Compare** to:
   - `agent-honest` / full-mixture: +4 388
   - `agent-honest` / full (argmax): +7 136

If the calibrated mixture beats +7 136, the mixture becomes the new
shipped configuration.

## A critical caveat: the router.runtime temperature

The `RouterRuntime::new` constructor takes a `temp` parameter (default
0.7) — this is the SHARPENING temperature, different from the
CALIBRATION temperature I fitted. They compose: the raw softmax is
replaced by `softmax(logits / T_calibration)`, then the runtime applies
`temp` sharpening for the Dirichlet prior. To preserve the calibration,
the runtime must use the fitted `T_calibration`. The best integration
point is probably to fold `T_calibration` into the model artifact (i.e.
store `weights / T` in the serialized `SoftmaxModel`), so the runtime's
`temp` sharpening composes correctly on top.

**Do not naively swap the router.bin without understanding the two
temperatures.** The pass is real; the integration needs care.

## Artifacts

- `artifacts/router_raw_19.rbin` (120 000 × 19)
- `artifacts/train-router-raw-19-calibrated.log` (the PASS line)
- `artifacts/routers/v19-calibrated/`

## Related

- `ROUTER-BET-SIZE-FEATURE-DESIGN-2026-09-30.md` — the feature design
- `ROUTER-19DIM-NEARLY-PASSES-2026-09-30.md` — the pre-calibration view
- `ROUTER-11DIM-NEGATIVE-2026-09-30.md` — the earlier negative
- `ROUTER-TRAINING-GAP-2026-09-28.md` — the synthetic stub problem
