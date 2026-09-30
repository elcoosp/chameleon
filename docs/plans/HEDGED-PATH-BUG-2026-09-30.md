# Hedged routing path diverges from argmax even at threshold=0.00 (2026-09-30)

## The diagnostic

`CHAM_HEDGE_THRESHOLD=0.00` makes the hedged branch's condition

    if top_weight >= threshold {
        // take the argmax path
    } else {
        // fall back to the mixture
    }

trivially true (`top_weight >= 0.0` for any non-negative weight). So
hedged-at-0.00 SHOULD take the argmax path on every single decision,
and the resulting ladder SHOULD match `full` (+6 587).

It does not. Measured:

| opponent | full (argmax) | full-hedged @ thr=0.00 | full-hedged @ thr=0.50 |
|---|---:|---:|---:|
| arch:nit      | +1 384 | −1 298 | −1 266 |
| arch:tag      | +3 382 | −1 364 | −1 373 |
| arch:lag      | +3 932 | −4 624 | −4 528 |
| arch:station  | +14 259 | −462 | −472 |
| callbot       | +24 962 | **+0** | **+0** |
| jamfix        | +4 787 | −299 | −299 |
| pnash         | +4 168 | −362 | −362 |
| famB:tag      | +2 269 | −2 690 | −2 690 |
| noisy:0.1:lag | +5 084 | −3 832 | −3 832 |
| **mean**      | **+6 587** | **−1 826** | **−1 800** |

**Two observations:**

1. **thr=0.00 ≠ thr=0.50, but only barely.** The means differ by 26
   mb/seating. Three of nine opponents (callbot, jamfix, pnash, famB,
   noisy) produce *identical* numbers. The others differ by 20-100,
   which is at the run-to-run noise level.

2. **thr=0.00 ≠ argmax at all.** Mean differs by 8 400 mb/seating. That
   is not noise.

**Conclusion:** the hedged decision path is not the argmax decision
path, even when the branch condition forces argmax. There is a real
code-path divergence. `HEDGED-ROUTING-BUG-2026-09-30.md` correctly
identified that hedged doesn't work; this doc pins the mechanism more
precisely: the failure is not "the threshold is wrong" (the sweep
proves it isn't tuned), the failure is that hedged *is not a variant of
argmax at all*.

## What the code says

`pipeline.rs::act_impl`:
