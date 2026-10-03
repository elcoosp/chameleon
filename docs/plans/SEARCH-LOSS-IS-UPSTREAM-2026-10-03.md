# Search loss is upstream of the solver (2026-10-03)

## The measurement

All three solvers, same bundle, `--search`, 2 opponents, 2500 deals/pair:

| solver | callbot | arch:station |
|---|---:|---:|
| **OFF** | **+24796.7** | **+14091.7** |
| Rnr (default) | +12325.9 | +5738.1 |
| ReachGadget | +12324.3 | +5674.4 |
| Fmbr | +12325.9 | +5746.3 |

The three ON arms are **identical to within SE**. Solver choice is
irrelevant to the loss.

## What this means

The loss is NOT in the solver. All three solvers share two things the
OFF path does not:

1. **The villain range** — `search_bridge` builds `blended` villain
   strategies from the prior + tracker (`villain_range_from_tracker`),
   and all three solvers consume that same range.
2. **The abstract→real action mapping** — the solver returns an
   *abstract* action (e.g. `bet0.5` over strength classes); the bridge
   maps it back to a real engine action. If that mapping systematically
   over-checks (because the range makes betting look -EV), the hero
   stops value-betting — exactly the calling-station failure.

Since swapping solvers changes nothing, **the bug is in (1) or (2)**,
not in the solve.

## Implications for the fix

- A **safe gadget alone will not fix it.** A gadget bounds exploitability
  by the blueprint's, but if the *action the search selects* is wrong
  (mapped from a wrong range), the bounded strategy is still just the
  blueprint's action — i.e. the gadget would at best **revert search to
  OFF** (no loss, no gain). Worth having as a floor, but it is not the
  win.
- The win requires **the range and/or the mapping to be right.** The
  blueprint-reach range (F1 plan, approach (a)) is the principled fix;
  the heuristic (c) demonstrably fails.
- The action mapping deserves its own audit: dump, per decision, the
  solver's abstract action and the real action it maps to, and compare
  to the OFF policy's action on the same node. (Not done here.)

## Status

Search stays OFF. The real fix is a **multi-day** effort:
1. blueprint-reach villain range (1-2d),
2. abstract→real mapping audit + fix (1d),
3. safe gadget as the floor (1d),
4. re-measure.

Not a between-jobs task. Closing the search thread until then.

Source: this doc + `SEARCH-NEGATIVE-IS-STALE-2026-10-03.md` (corrected)
+ `SEARCH-REAL-FIX-PLAN-2026-10-03.md`.
