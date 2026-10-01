# Router tilt feature: design proposal (2026-09-30)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


## The problem

`ROUTER-RAW-FEATURES-RESULT-2026-09-29.md` reported that the honest
10-dim raw-opponent frequency vector separates TAG (top-1 recall 0.52)
and LAG (0.58) poorly. The gate requires per-class recall ≥ 0.70.

The test committed today (`tag_lag_aggression_tilt_is_visible_in_raw_features`,
`crates/cham-agent/tests/tracker_raw_freq.rs::bfb058d`) proves the
signal is present but **not in any single raw frequency**.

## What the archetype parameters actually say

From `crates/cham-opponents/src/params.rs::point()`:

| parameter     | TAG  | LAG  | direction |
|---|---:|---:|---|
| open_raise    | 0.25 | 0.40 | LAG opens more |
| three_bet     | 0.09 | 0.14 | LAG 3bets more |
| call_open     | 0.58 | 0.70 | LAG calls wider |
| cbet_flop     | **0.52** | **0.42** | **TAG cbets more** |
| barrel_turn   | **0.62** | **0.55** | **TAG barrels more** |
| barrel_river  | **0.72** | **0.66** | **TAG barrels more** |
| donk          | **0.50** | **0.42** | **TAG donks more** |
| size_idx      | 1 (66%) | 2 (100%) | LAG bets bigger |

TAG and LAG move in **opposite directions** on preflop-aggression vs
postflop-continuation. That is the discriminator. No single raw
frequency captures it because the naive "preflop raise rate" points
one way and "flop bet rate" points the other.

## Proposed derived features

Add three derived scalars to the honest feature vector. They are
functions of the existing 10 raw frequencies — no new tracker state
is needed, and the 10 raw frequencies themselves stay in the vector.

### 1. `preflop_postflop_tilt`

    tilt = (flop_bet_freq + turn_bet_freq + river_bet_freq) / 3
           - preflop_raise_freq

Interpretation: positive → more postflop aggression relative to
preflop (TAG pattern); negative → more preflop aggression (LAG
pattern). Range roughly [-1, +1].

Expected values from the point parameters:

- TAG: (0.52 + 0.62 + 0.72)/3 − 0.25 = 0.62 − 0.25 = **+0.37**
- LAG: (0.42 + 0.55 + 0.66)/3 − 0.40 = 0.543 − 0.40 = **+0.14**

Δ = 0.23 on the tilt axis. That is 8x larger than the LBR noise band
and 3x larger than the naive flop-bet gap.

### 2. `aggression_size`

    size = (opp_river_bets as f64 / opp_river_bets_that_were_raises_size2)

The tracker doesn't currently record bet sizes. Extending it is
invasive (I9 leak rules, see the earlier audit). **Defer.**

### 3. `preflop_tightness`

    tightness = 1 - preflop_raise_freq

A direct read of the LAG-opens-more signal, redundant with feature 0
of the existing vector. **Do not add** — it's already there.

## Recommendation

Add ONE feature: **`preflop_postflop_tilt`**, defined as above.

That brings the honest vector to 11 dims. `SoftmaxModel::new(11, 4)`
gives the router an axis aligned with the TAG/LAG difference that
currently costs 8 200 mb/seating on the mixture-vs-argmax comparison.

## Integration cost

- `tracker.rs`: no change (derives from existing counters)
- `pipeline.rs::opponent_only_features`: adds one element → [f64; 11]
- `features.rs`: new dimension constant → 21 for the 20-dim set, 11
  for the honest set
- `model.rs`: `SoftmaxModel::new(11, 4)` for training
- `cham-cli/src/cmd/collect.rs`: `--raw-opponent` path emits 11 columns
- Retrain `router.bin` on a fresh `collect --real --raw-opponent` run
- `agent-honest` bundle's `router.bin` becomes invalid; requires a
  retrain-and-replace cycle

That's a session-sized job, not a mid-morning fix. But it is the
single concrete next step toward a working router.

## What this does not solve

Even an 11-dim honest vector won't clear the 0.70-recall gate by
itself. The tilt feature helps TAG vs LAG; the remaining failure mode
is LAG vs NIT, which the current 10-dim set separates reasonably
(TAG recall 0.52 is bad, but NIT is much higher). A full pass needs
per-class recall measurement after the tilt feature is added.

## Prior art in the session

`docs/plans/ROUTER-RAW-FEATURES-RESULT-2026-09-29.md::paths forward`
suggested "Raise-rate by position/street" as the easy win. The tilt
scalar is the simplest member of that family, and the test proves
its magnitude.
