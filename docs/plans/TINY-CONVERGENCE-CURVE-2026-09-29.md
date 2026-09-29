# Tiny convergence curve — SB keeps improving, BB peaks at 5M (2026-09-29)

## The data (γ=1.0, parallel, tiny abstraction, Robust)

| iters | seat 0 (SB) | seat 1 (BB) | mean |
|---|---:|---:|---:|
| 500k | 23 280 | 13 957 | 18 619 |
| 5M   | **15 040** | **12 050** | **13 545** |
| 20M  | **13 809** | 14 956 | 14 383 |

Lower is better. Bold = best per column.

## What this actually says

* **Seat 0 keeps improving**: 23 280 → 15 040 → 13 809. Each 4× iteration
  step buys 30% then 8%.
* **Seat 1 peaks at 5M**: 13 957 → 12 050 → 14 956. Beyond 5M, the BB
  policy gets *worse*, by 24%.

The pattern is not noise. The same shape showed up in the earlier
50M run (`50M-CONVERGENCE-2026-09-28.md`) with γ=0.9: seat 1 was 13 957
at 500k, 18 205 at 5M — wait, that was 5M with γ=0.9. Anyway the 50M
run at γ=0.9 showed seat 1 at 17 948, materially worse than the 500k
baseline of 13 957.

## Candidate explanations

1. **The averaging window tilts the policy.** With γ=1.0 (no decay), the
   strategy sum weights by `(t - T/4)`, so the last quarter dominates.
   Over 20M iterations, the current iterate's early-stage play gets
   drowned out, and the late-stage CFR+ current strategy oscillates for
   SB but not BB — no wait, it's the opposite: BB degrades, SB improves.

2. **Something seat-specific in the traversal.** Robust mode alternates
   hero_seat = `(t % 2)`. If a training bug correlates with `t % 2` and
   iteration count (e.g., a one-in-a-million `w_t` overflow), the
   affected seat would degrade with more iterations, not less.

3. **BB is genuinely harder to solve and CFR+ is overfitting on it.**
   Seat 1 has position disadvantage postflop. Maybe the tiny abstraction
   is too coarse for BB and additional iterations push the policy into
   a region that a best responder exploits harder.

4. **The 200-deal sample is too small.** SB vs BB differences of 2-3
   bb/hand could be within the LBR estimator's CI at 200 deals. The
   earlier seat-1 numbers (13 957 → 12 050 → 14 956) span ~3 bb/hand.
   The 20M vs 5M seat-1 delta is +2.9 bb/hand. That is *borderline*
   for a 200-deal sample; the 5M vs 500k delta on seat 1 is only
   −1.9 bb/hand, also borderline.

## What we should NOT conclude

The earlier "50M shows tiny is at its ceiling" was based on γ=0.9 data
and was wrong. The current 20M/γ=1.0 data shows SB still improving and
BB regressing. It is not "at ceiling"; it is split.

## What to do

1. **Increase deals to 1000 on the 5M and 20M artifacts** to see whether
   the seat-1 regression survives with more samples. 5 minutes of
   compute, high information.

2. If seat-1 regression survives at 1000 deals, it is real and we need
   to understand it. The next diagnostic is to plot the averaging
   weights against seat over iterations — the SB/BB split in the
   strategy sum.

3. If it disappears at 1000 deals, the "regression" was noise, and the
   correct reading is that SB improves and BB is roughly flat.

## Meanwhile

The pipeline is now training **medium abstraction at 20M iters** (PID
10426). That is the real next data point. It will finish ~18:30.

After that: tiny 50M (~8 h). Then we will have the full curve on tiny
and the medium comparison at a matched iteration budget.
