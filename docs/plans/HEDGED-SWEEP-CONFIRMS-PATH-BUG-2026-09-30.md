# Hedge threshold sweep confirms the path bug (2026-09-30)

**CORRECTION 2026-09-30 (13:21):** The full-hedged numbers in this doc
were produced by a `ladder` invocation that silently fell through to
CallBot (the CLI's `TRAINED_AGENTS` guard did not include
`full-hedged`). The numbers reflect CallBot-vs-pool, not hedged
routing. See `HEDGED-BUG-TRAINED-AGENTS-2026-09-30.md`. A re-measurement
with the fix is at `artifacts/ladder-full-hedged-FIXED.log`.

## The sweep

`scripts/ladder-hedge-sweep-2026-09-30.sh` ran `full-hedged` at 5
thresholds: 0.00, 0.20, 0.50, 0.80, 1.00. The hypothesis being tested:
if the hedged routing is a working argmax-mixture hybrid, the
threshold should tune the tradeoff, and the extreme values should
match the corresponding pure mode:

- threshold=0.00 should match `--agent full` (argmax): **+6 587**
- threshold=1.00 should match `--agent full-mixture`: **+4 388**

## The result

| threshold | mean |
|---|---:|
| 0.00 | −1 826 |
| 0.20 | −1 825 |
| 0.50 | −1 825 |
| 0.80 | −1 806 |
| 1.00 | −1 887 |

**Every threshold produces approximately the same mean.** The
threshold is a non-lever. The maximum spread across all five runs is
81 mb/seating, well inside the ±200 run-to-run noise band.

The **delta to `full` is +8 400 at every threshold**, not just at
threshold=0. This cannot be explained by any threshold-setting mistake:
a working hedged implementation would give `full`-equivalent numbers
at threshold=0.00.

## What this proves

The hedged routing decision is not "argmax above threshold, mixture
below". The `top_weight >= threshold` condition at threshold=0.00 is
trivially true (`top_weight` is a non-negative probability from a
5-simplex), so if the condition were consulted, `full-hedged` and
`full` would be bit-identical at 0.00. They are not.

Two possible mechanisms, both structural:

1. **The `top_weight` value is NaN.** `NaN >= 0.0` is `false`, so the
   hedged branch would take the mixture path at every threshold,
   producing identical results across all thresholds. This is the
   cleanest explanation for the observed sweep.

2. **The hedged branch's argmax path differs from `argmax` mode in a
   second way** (e.g., a different `tier_missed` computation, a
   different R3 re-decision rule). Even when threshold forces argmax,
   the *actions taken* differ from `full`'s.

The earlier doc `HEDGED-PATH-BUG-2026-09-30.md` identified mechanism
(2). The sweep is consistent with either (1) or (2). The next step
for a future session is to instrument `top_weight` and log it — a
5-line `eprintln!` in the hedged branch will settle which mechanism.

## Action items for the next session

1. **Instrument** the hedged branch: log `top_weight` once per hand
   under an env var (`CHAM_HEDGE_DEBUG=1`). Run a probe and check
   the printed values. If any are NaN, mechanism (1) is confirmed.

2. **If (1)**: find where `weights_for_hand` can produce NaN.
   Candidate: `prior[k]` after `powf(1.0/temp)` on a `p[k] = 0.0`
   gives `0^∞ = 0` (fine) but `1.0/temp` on `temp = 0` gives `∞`
   (temp is 0.7 by default, so not the issue). Or the `1/total`
   normalization if `total` is 0.

3. **If (2)**: unify the argmax and hedged-confident branches into a
   single function.

4. **Do not re-run the sweep** — it has answered its question.

## Artifacts

- `artifacts/ladder-hedge-thr0.00.log`
- `artifacts/ladder-hedge-thr0.20.log`
- `artifacts/ladder-hedge-thr0.50.log`
- `artifacts/ladder-hedge-thr0.80.log`
- `artifacts/ladder-hedge-thr1.00.log`
