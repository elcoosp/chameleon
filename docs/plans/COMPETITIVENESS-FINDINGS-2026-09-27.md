# Competitiveness findings (2026-09-27/28)

## The one number we trust

At depth 100, tiny abstraction, 200 deals:

| Policy | seat 0 | seat 1 |
|---|---|---|
| Uniform (no trained weights) | +48 186 mb/hand | +28 013 mb/hand |
| Trained 50k iters, flat ladder | +44 004 | +20 271 |
| Trained 50k iters, rich ladder | +43 813 | +22 868 |

**The trained policy is only 3–27% better than uniform** on this abstraction.

## What this rules out

**Betting-tree coarseness is not the bottleneck.** A richer ladder (3 flop
sizes, 3 turn sizes, 2 preflop opens, cap 2) was trained at the same 50k
budget and came out:
- marginally worse on seat 0 (43.8 vs 44.0)
- marginally worse on seat 1 (22.9 vs 20.3)

The rich ladder cost 1.8 h for 50k iters (16k → 305k infosets, ~100x
compute) and produced no improvement. Config-only ladder tuning is not
where the EV is.

## What remains to check

1. **Iteration budget** (sweeps launched): 500k and 5M on the flat tiny.
   If 500k is much better than 50k, the policy is simply undertrained.
   If flat, the abstraction is the ceiling.
2. **The training pipeline itself** — is CFR+ actually updating regrets on
   the reach-weighted trajectory it is supposed to? The "Robust" mode uses
   the traversal of the OTHER seat's current strategy, which is the whole
   point of the abstraction. A silent bug there would produce a policy
   that barely beats uniform despite 50k iterations, exactly what we see.
3. **Router + mixture.** The trained policy in the bench is the ROBUST
   policy alone. If the shipped agent is a mixture, an undermixed or
   mis-weighted router could be suppressing the strong specialist.

## What this says about priorities

Before touching more config knobs, verify that the trainer's output is
actually converging to a Nash-like policy on this abstraction. Uniform is
48/28; a real CFR+ policy at 50k iters should be measurably better than
that, not 3–27% better. A factor of 5+ improvement is the realistic
target; a factor of 1.1x means the trainer is broken or the abstraction
is degenerate.

## Recommended next step

Wait for the 500k / 5M sweeps to finish, then look at the curve:
- 50k → 500k → 5M
- If the curve is flat after 50k: trainer bug, investigate traversal.
- If it drops sharply at 500k: undertrained, let it run longer.
