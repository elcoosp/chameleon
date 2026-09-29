# Exploration floor: does not fix the freeze (2026-09-29)

## The measurement

5M tiny robust at 1000-deal LBR, eps=0.02 vs the no-eps baseline:

| 5M variant | seat 0 | seat 1 | mean |
|---|---:|---:|---:|
| no-eps | 15 040 | **12 050** | **13 545** |
| eps=0.02 | 14 910 | 12 551 | 13 731 |

Seat 0 is unchanged. Seat 1 is 4% WORSE. Mean is 1.4% worse.

The `rm_freeze` diagnostic on the eps02 tables shows the freeze is still
happening:

| iters | mean avg max_p (no eps) | mean avg max_p (eps=0.02) |
|---|---:|---:|
| 5M  | ~0.453 (500k proxy) | 0.801 |
| 20M | 0.859 | **0.846** |

The floor moves the average-strategy concentration from 0.859 to 0.846
at 20M — a 1.5% shift, not the collapse-reversal that was hoped for.
And at 5M it actively hurts.

## What this rules out

The mechanism I hypothesized (RM+ regrets bottoming out so individual
actions have no signal) is **not** what is driving the collapse. A
2% uniform floor keeps every action's regret channel alive, but the
positive part still concentrates because the non-favored actions
accumulate only the tiny regrets the floor produces — and 2% is too
small to counterbalance that.

Put differently: the floor forces 2% of the mixture to be uniform, but
98% still follows the RM+ distribution. The RM+ distribution itself is
still freezing, and the resulting average is 0.846-concentrated instead
of 0.859. That is a small cosmetic change, not a fix.

## What might actually work

1. **Much larger eps.** 0.05 or 0.10 would materially change the mixture.
   But it likely also destroys the peak LBR (the sharpened policy is
   what beats uniform). The tradeoff is unknown.

2. **A different exploration mechanism.** Regret-matching with a
   temperature (softmax over regrets, not division) would smooth the
   distribution continuously rather than adding a floor. Or the DCFR
   `alpha < 1` regret discount (already a knob: `regret_discount`) slows
   accumulation of the dominant regret.

3. **Fix the averaging schedule, not the iterate.** The finding doc
   `AVG-DELAY-VS-FREEZE-2026-09-29.md` argues the delay D=T/4 excludes
   the mixed pre-freeze phase. That experiment is queued (running now,
   PIDs 70041 / 70016) and tests whether including the early phase
   alone fixes BB.

## The 20M eps02 result is pending

The 20M eps=0.02 LBR is still running. Two possibilities:
- It lands worse than the no-eps 20M (13 319 / 14 706): the floor is
  unambiguously bad and we drop it.
- It lands between the no-eps 20M and the 5M peak: the floor is
  neutral-to-mildly-positive at 20M but hurts at 5M. Not shippable
  as a default.

Either way, the exploration floor is not the answer to the freeze.

## Housekeeping

The `artifacts/par-5M/` directory (the current best policy) is missing
its `table.snap` on disk — the whole dir was recreated at some point
and only the policy survived. I still have the LBR (15 040 / 12 050)
but not the snapshot for diagnostic re-runs. If a future experiment
needs the 5M no-eps snapshot, it must be retrained (~47 min).
