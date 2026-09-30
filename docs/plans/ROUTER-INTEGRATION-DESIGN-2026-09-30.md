# Router integration design: shipping the 19-dim calibrated router (2026-09-30)

## The blocker

The 19-dim calibrated router passes the gate
(`ROUTER-19DIM-PASSES-2026-09-30.md`). It cannot be shipped without
three integration changes:

### Blocker 1: `RouterRuntime::weights_for_hand` is hardcoded to 20 features

    pub fn weights_for_hand(&mut self, features: &[f32; 20], trend_z: f64)

The array length is `20` in the signature. A 19-dim model's features
would have to be padded to 20 (or the signature widened to a slice).

### Blocker 2: the pipeline emits the 20-dim opportunity-gated vector

`ChameleonAgent::start_hand_if_needed` calls `self.tracker.tracker_features()`,
which produces the 20-dim *opportunity-gated* vector. For the 19-dim
model to see the right inputs, the pipeline must emit
`opponent_only_features_19()` instead.

### Blocker 3: the model artifact does not declare its feature set

`SoftmaxModel` serializes `weights`, `bias`, `n_features`, `n_classes`.
It does not record *which* feature set the weights were trained
against. Loading a 19-dim honest router is indistinguishable from
loading a 20-dim opportunity-gated one — the `n_features` field
matches the array length but not the *semantic* meaning of each
dimension.

## The three integration changes

### Change 1: widen `weights_for_hand` to `&[f32]`

    pub fn weights_for_hand(&mut self, features: &[f32], trend_z: f64) -> [f64; N_EXPERTS]

`SoftmaxModel::forward` already does `.take(self.n_features)` on the
input, so a slice of any length ≥ `n_features` works. The only callers
are `pipeline.rs` and the tests. This is a mechanical signature change.

### Change 2: add `feature_set` to `SoftmaxModel`

    #[derive(Serialize, Deserialize)]
    pub struct SoftmaxModel {
        pub weights: Vec<Vec<f64>>,
        pub bias: Vec<f64>,
        pub n_features: usize,
        pub n_classes: usize,
        #[serde(default = "default_feature_set")]
        pub feature_set: String,   // "opportunity-gated-20", "raw-opponent-19", ...
    }

Default is `"opportunity-gated-20"` so all pre-change artifacts load
identically. `train-router` writes the actual feature set name, which
it can derive from `n_features` + a CLI flag
(`--feature-set opportunity-gated-20|raw-opponent-10|raw-opponent-11|raw-opponent-19`).

### Change 3: pipeline dispatch on `feature_set`

    let raw_features: Vec<f32> = match self.router.model().feature_set.as_str() {
        "raw-opponent-19" => self.opponent_only_features_19().iter().map(|&x| x as f32).collect(),
        "raw-opponent-11" => self.opponent_only_features_11().iter().map(|&x| x as f32).collect(),
        "raw-opponent-10" => self.opponent_only_features().iter().map(|&x| x as f32).collect(),
        _ => self.tracker_features().to_vec(),   // opportunity-gated-20 (default)
    };

The pipeline needs read access to the model's `feature_set` string.
`RouterRuntime` exposes `pub model: SoftmaxModel`, so this is a
one-line `self.router.model.feature_set.as_str()`.

## The temperature question

The `T_calibration` fitted by `train_model` is a *softmax* temperature:
it re-tempers the output distribution for calibration purposes. The
`RouterRuntime.temp` field is a *sharpening* temperature applied to
`p^(1/T)` inside `weights_for_hand`.

If we just load the calibrated model, the runtime's `temp=0.7`
sharpening is applied to a softmax that is *already* the calibrated
distribution. The composition is:

    calibrated_p = softmax(logits / T_cal)
    sharpened    = calibrated_p ^ (1 / temp_runtime) / Z

That is NOT the same as `softmax(logits / (T_cal * temp_runtime))` in
general (temperature composition of softmax doesn't commute with power
sharpening). The cleanest integration is:

**Fold `T_cal` into the serialized model.** Store `weights / T_cal`
in the model JSON's `weights` field. Then the runtime's `forward`
computes `softmax((weights / T_cal) · x + bias / T_cal)` — which is
exactly `softmax(logits / T_cal)`. Then the runtime's `temp=0.7`
sharpening composes normally.

That keeps `RouterRuntime` oblivious to the calibration and preserves
the gate numbers.

## The minimal-risk sequence

1. **Widen the signature to `&[f32]`** (change 1). Zero behaviour
   difference; all existing tests still pass.
2. **Add `feature_set` to `SoftmaxModel`** with a default that
   preserves all historical loads (change 2). `train-router` gains a
   `--feature-set` flag. Integration test: a `--feature-set
   opportunity-gated-20` retrain produces a byte-identical model JSON.
3. **Fold `T_cal` into the model weights** at save time. The
   `train-router` code writes `weights / T_cal` and drops `bias / T_cal`
   in as well. `TrainReport` still reports the fitted T for audit.
4. **Dispatch on `feature_set` in `start_hand_if_needed`** (change 3).
5. **Build the `agent-honest-19dim` bundle** and ladder-compare.

## Estimated cost

- Changes 1-4: ~2 hours of careful work.
- Bundle assembly + ladder: ~45 min.
- Total: ~3 hours. Non-trivial but tractable.

## The alternative

**Skip the mixture path entirely.** Given that argmax+synthetic is
degenerate and its +7 136 ladder is really the LAG expert alone, the
shipped configuration might be better served by:

- Simplifying to a single-expert bundle (LAG) with no router.
- Retraining the LAG expert on the archetype ladder objective.
- Dropping the mixture architecture from the shipping path.

That saves the entire 3-hour integration and arguably produces a
better outcome (a policy that's been trained against the actual
opponents vs a mixture of 4 experts with one good one).

**Recommendation:** run the 5M expert retrain to completion first
(it's in flight). If argmax on the 5M-expert bundle beats +7 136,
the expert path is the frontier; if not, do the mixture integration.

## Related

- `ROUTER-19DIM-PASSES-2026-09-30.md` — the gate pass
- `ROUTER-BET-SIZE-FEATURE-DESIGN-2026-09-30.md` — the feature
- `SYNTHETIC-ROUTER-IS-DEGENERATE-2026-09-30.md` — why the current
  shipped router is degenerate
