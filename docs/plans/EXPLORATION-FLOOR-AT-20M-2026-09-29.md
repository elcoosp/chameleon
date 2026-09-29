# Exploration floor: helps at 20M, hurts at 5M, doesn't lift the ceiling (2026-09-29)

## The full picture

1000-deal LBR at depth 100. Same bundle seeds; only eps varies.

| iters | eps  | seat 0 | seat 1 | mean |
|---|---:|---:|---:|---:|
| 5M  | 0.00 | **15 040** | **12 050** | **13 545** |
| 5M  | 0.02 |  14 910 |  12 551 |  13 731 |
| 20M | 0.00 |  13 319 |  14 706 |  14 012 |
| 20M | 0.02 |  13 682 |  13 652 |  13 667 |

Two facts:

1. **The floor helps at 20M.** Mean improves 14 012 → 13 667 (2.5%).
   Seat 1 improves 14 706 → 13 652 (7.2%). This is real and directionally
   consistent with the freeze hypothesis.

2. **The floor hurts at 5M.** Mean worsens 13 545 → 13 731 (1.4%).
   Seat 1 worsens 12 050 → 12 551 (4.2%). At the current peak the floor
   is net negative.

The peak is still 5M-no-eps. 20M-eps is 12% worse on mean than 5M-no-eps.

## What this means

The freeze is real but **not fixable by a small uniform floor**. It
matters at 20M (where the freeze is deep) but the cost of the
exploration (forced 2% uniform) outweighs its benefit at 5M (where the
policy has not yet frozen enough to need it).

The floor is a **degradation slower** — it makes the collapse rate
shallower at long runs. It does not reverse the collapse.

## Conclusion

Ship neither eps=0.02 (hurts the current best) nor a rule that enables
it above some iteration threshold (untested and risky).

The correct next lever, per the earlier finding, is the **averaging
schedule**. That experiment is running now — 20M with
`CHAM_AVG_DELAY=0` (weights from iteration 0, includes the mixed
pre-freeze phase). If the delay is what discards the useful early
iterations, removing it should show up as a lower seat-1 LBR at 20M
WITHOUT an exploration floor.

If that also fails, the remaining candidates are:
- A much larger floor (eps = 0.05 or 0.10) — but likely to degrade the
  sharpened SB value the current policy depends on.
- DCFR alpha < 1 (`--regret-discount`) to slow the accumulation of the
  dominant regret.
- A softmax-over-regrets in place of RM+'s division.
