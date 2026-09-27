# Averaging-weight γ analysis (2026-09-27)

## The formula

`crates/cham-blueprint/src/trainer.rs::averaging_weight_gamma`:

    let d = total / 4;
    let base = if t > d { (t - d) as f64 } else { 0.0 };
    if robust {
        base * (gamma as f64).powi((total.saturating_sub(t)) as i32)
    } else {
        base
    }

Two factors multiply the CFR+ strategy-sum update:
1. `base` = delayed linear averaging: zero before `T/4`, then `t - T/4`.
2. `γ^(T-t)` = exponential future-decay, where `T-t` is the number of
   iterations remaining.

## Effective window by γ (measured numerically for T=3M)

| γ | weight in last 10 iters | weight in last 1000 iters |
|---|---|---|
| **0.9** (default) | **65 %** | 100 % |
| 0.99 | 9.6 % | 99.99 % |
| 0.999 | 1.0 % | 63 % |
| 1.0 | 0.0009 % | 0.09 % |

**γ=0.9 is not an average; it is a snapshot of the last ~10 iterations.**
The "average strategy" that a CFR+ run reports at γ=0.9 is
indistinguishable from the last iterate, and the last iterate of a
sampling MCCFR trajectory is a noisy sample, not a mixed Nash strategy.

## Why this matters for the LBR numbers

CFR+'s convergence theorem is about the **average** strategy, not the last
iterate. With γ=0.9, the reported policy is a last-iterate snapshot — so:

- The tiny-3M LBR (26,641) is the exploitability of a last-iterate, not
  the LBR that CFR+'s convergence guarantee predicts.
- The 10M LBR regression (39,065) is consistent with this: more iters
  picks a different tail sample, and there is no a-priori reason for the
  10M tail to be less exploitable than the 3M tail. Neither is "the
  average".
- The γ=1.0 setting is the ONLY one of the four that actually averages
  over a wide window (uniform over the last 75 % of iterations).

## The algorithm-level issue (subtler, larger)

Brown & Sandholm's DCFR applies γ as a per-step discount on the
**accumulated strategy sum**:

    S_t(I,a) ← γ_t · S_{t-1}(I,a) + σ_t(I,a)

with γ_t *increasing toward 1* over time (typically
γ_t = t^1.5 / (t^1.5 + 1)). Each iteration's strategy is discounted by the
cumulative product of γ_t over the *preceding* iterations.

The code applies γ^(T-t) as a *fixed* forward-looking decay — each
iteration's contribution is scaled by an exponentially-decaying factor
proportional to how far it is from the END of training. That is a
different algorithm, and it does not inherit DCFR's convergence
guarantee. Even at γ=1.0 there is no per-step decay, so it is closer to
CFR+'s linear averaging than to DCFR.

## What the previous γ sweep actually measured

The pre-session α/γ sweep (`exp-011-dcfr-alpha-gamma-sweep`, marked STALE
in `docs/reports/20260927-rbp-gate-stale-results.md`) found that γ=0.5
improved LBR by 8.6 % over γ=0.9. But:

- Every cell was measured on the pre-`fe84467` collapsed policy.
- γ=0.5 is an even *tighter* snapshot than γ=0.9 (last 1–2 iters). It
  being "better" was a measurement of which tail happened to be less
  exploitable under the buggy gate, not of any real averaging property.

The γ=1.0 diagnostic in the same session (also pre-`fe84467`) reported
γ=1.0 was WORSE (54,478 vs 43,622). This is not a valid comparison
either — both are collapsed-policy numbers.

## Suggested action

1. **Do not re-run the α/γ grid as-is.** The grid's premise (γ tunes
   averaging) is wrong under the current formula; the only meaningful
   value in {0.9, 0.99, 0.999, 1.0} is γ=1.0, which is the closest to
   linear averaging.
2. **Test γ=1.0 honestly** — post-`fe84467`, on the tiny abstraction at
   the 3M-iter scale (the same scale as `honest-lbr-tiny-3M-s7`). If LBR
   drops materially below 26,641, the averaging window was the problem;
   if not, the plateau is abstraction-driven.
3. **Separately: consider fixing the formula** to match DCFR's semantics
   — per-step discount `S_t ← γ_t · S_{t-1} + σ_t` with γ_t increasing
   toward 1. That's a real algorithmic change (and a `TrainerConfig`
   field reshuffle); do not do it in the same session as the honest
   re-baseline, or the two results will be indistinguishable.
4. **Update the v7 runbook's Item 3** to record that the γ grid needs
   redesigning, not just re-running.
