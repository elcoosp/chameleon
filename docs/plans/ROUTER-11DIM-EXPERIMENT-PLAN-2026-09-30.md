# 11-dim honest router training — experiment plan (2026-09-30)

## The proposal

`ROUTER-TILT-FEATURE-DESIGN-2026-09-30.md` identified a concrete
extension to the honest 10-dim raw-opponent feature vector: add a
**preflop/postflop aggression tilt** scalar that separates TAG from
LAG. The 10-dim vector gets top-1 B-test 0.797 with TAG recall 0.515
and LAG recall 0.584 — both below the 0.70 gate.

The 11-dim vector is now plumbed end to end:

- `Tracker::preflop_postflop_tilt` (commit `66f8eac`)
- `ChameleonAgent::opponent_only_features_11` (commit `a049b86`)
- `collect --real --raw-opponent-11` (commit `b37b29b`)

## The experiment

1. **Collect the data.** `collect --real --raw-opponent-11` running now
   (PID 62841, log `artifacts/collect-raw-11.log`, output
   `artifacts/router_raw_11.rbin`). Same parameters as the 10-dim run:
   4 opponents × 60 sessions × 500 hands = 120 000 rows.

2. **Train the router.** Same hyperparameters as the 10-dim run:
   100 epochs, default train/dev/test split.

       target/release/chameleon train-router \
         --rows artifacts/router_raw_11.rbin \
         --out artifacts/routers/v11

3. **Compare against the 10-dim baseline.** The gates are:
   - top-1 B-test ≥ 0.80
   - per-class recall ≥ 0.70
   - ECE B-test ≤ 0.15

   The 10-dim run failed all three: 0.797 / 0.515 (TAG) / 0.363.
   The single hypothesis is that the 11th feature raises TAG recall
   past 0.70 (design estimate: 0.65-0.75).

## Expected outcome

- **Upside:** TAG recall crosses 0.70. If so, the mixture can finally
  distinguish TAG from LAG and the router becomes shippable.
- **Downside:** TAG recall remains below 0.70. The raw-frequency
  approach is fundamentally too coarse and the router needs features
  that describe *which* hands the opponent raises with (showdown
  strength distribution, bet-size distribution), which are much
  larger changes.
- **Coin-flip:** TAG recall improves to 0.70-0.72 but ECE gets worse
  (the 10-dim run's ECE 0.363 is already failing). The gate requires
  calibration, not just top-1.

## What not to do

- **Do not retrain the synthetic router.** `collect` without `--real`
  produces feature vectors with the class ID encoded in a dimension.
  Its gate is vacuous. See gotcha 5.12 in
  `HANDOFF-2026-09-29-FULL.md`.

- **Do not swap `agent-honest/router.bin` with the 11-dim router yet.**
  The shipped `agent-honest` bundle uses the current synthetic-trained
  20-dim router. The 11-dim model requires the model's `n_features`
  to match. Loading a 20-dim router into an 11-dim-trained model
  would fail. The swap is a separate step after the router passes.

## The re-training pipeline after the gate

If the 11-dim router passes:

1. Build a new `artifacts/agent-honest-11dim` bundle: same experts
   and robust, but with the 11-dim router installed.
2. Re-run the ladder with the new bundle to compare to the +7 136
   argmax+synthetic baseline.
3. Only if the ladder improves, ship the new bundle.

## Cost

- Collection: ~10 min (4 opponents, similar to the 10-dim run's
  `collect-raw-v2.log` timing at 11:12).
- Training: ~5 min (the 10-dim run's `train-router-raw-v2.log`
  completed quickly).
- Ladder comparison: ~30 min × 3 routing modes = ~90 min.

## Related

- `ROUTER-TILT-FEATURE-DESIGN-2026-09-30.md`
- `ROUTER-RAW-FEATURES-RESULT-2026-09-29.md`
- `ROUTER-FAILS-ON-REAL-DATA-2026-09-29.md`
