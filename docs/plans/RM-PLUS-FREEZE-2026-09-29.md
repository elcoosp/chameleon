# RM+ freeze is why tiny LBR peaks at 5M (2026-09-29)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


## The diagnostic

A new inspection tool (`rm_freeze`) reads a `table.snap` and reports
per-row how concentrated the current iterate (`sigma_rms`) and the
average strategy (`avg_strategy`) are. Frozen = max prob ≥ 0.9.

    rows (w≥2)   soft (<0.5)  avg_near_frozen  mean cur max_p  mean avg max_p
    ─────────────────────────────────────────────────────────────────────────
    500k         13.5 %        4.1 %           0.762           0.453
    20M           3.3 %       60.0 %           0.871           0.859
    50M           2.3 %       66.0 %           0.888           0.879

## What it means

**The current iterate collapses.** RM+ floors regrets at zero (standard
CFR+). Once `Σ max(R,0)` concentrates on a single action, RM+ plays that
action with probability 1, the regret for the other actions never rises
above 0 again, and the policy never re-explores. Soft rows fall from
13.5 % (500k) to 2.3 % (50M) — the policy is progressively freezing.

**The average strategy follows.** At 500k the average is genuinely mixed
(mean max prob 0.45). At 20M+ the average has converged to the current
iterate (0.86 / 0.88) because Linear CFR+ averaging weights the last T/4
of iterations, and the last T/4 are all frozen at the same policy.
Averaging cannot un-freeze an iterate that has stopped exploring.

**This explains the LBR curve exactly.** A one-hot policy is trivially
exploitable. SB (seat 0) benefits from the sharpening — its equilibrium
is closer to pure, so the frozen policy is a *better* approximation of
the equilibrium. BB (seat 1) suffers — its equilibrium needs mixing, and
the frozen policy abandons it. Net: 5M is the sweet spot where the
sharpening has improved SB but not yet cost BB as much.

## The fix

Inject an **exploration floor** into `sigma_rms`: every action keeps at
least `eps/n` probability, so no action's regret can permanently bottom
out. Standard CFR+ on large games does this implicitly through sampling
noise; here the training is deterministic per iteration and the floor is
needed explicitly.

Signature change:

    pub fn sigma_rms(&self, off: u32, w: usize) -> Vec<f64>

becomes

    pub fn sigma_rms_eps(&self, off: u32, w: usize, eps: f64) -> Vec<f64>

with `sigma_rms` delegating with `eps = 0.0` (bit-identical to today for
callers that don't opt in).

The training loop reads `CHAM_EXPLORE_EPS` (already exists for opponent
exploration; extend it to hero, or add `CHAM_TRAIN_EPS`).

## What to test

Retrain tiny robust at 5M and 20M with `CHAM_TRAIN_EPS=0.02`. Measure:
- `mean avg max_p` — should stay near 0.5-0.6 instead of climbing past 0.85
- `soft (<0.5)` — should stay above 10 %
- LBR seat 1 at 20M — should stay near 12 000 instead of 14 700

If it does, the tiny ceiling is lifted and 20M/50M become useful again.

## Related

This is a *training dynamics* finding, not a code bug. The trainer, the
averaging, and the table all do exactly what they are documented to do.
The freezing is an inherent property of CFR+ on this abstraction at this
iteration scale.
