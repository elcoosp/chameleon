# Adaptive search A/B: inconclusive (non-reproducible baseline) (2026-10-06)

## Result

`self-exploit` adaptive manipulator (nit-then-deviate) vs live `full`,
3000 deals:

| arm | manipulator earns |
|---|---:|
| OFF | -3803.9 ± 560.4 |
| ON (--search, gadget) | -4850.8 ± 651.4 |
| delta (ON-OFF) | -1046.9 ± 859.3 (**1.2 sigma**) |

Direction (ON more negative = victim less exploitable) FAVORS search, but
1.2 sigma is **not significant**.

## The bigger problem: non-reproducible baseline

The SAME config (tiny-full snapshot, 3000 deals, OFF) gave:

| run | OFF manipulator earns |
|---|---:|
| insight-2026-10-06 | **-122.6 ± 198.1** |
| insight2-2026-10-06 | **-3803.9 ± 560.4** |

A 3700 mb/seating swing — 18x the stated SE. The adaptive test is NOT
reproducible at this budget. Any ON/OFF delta from it is therefore noise.

## What is established

- Both OFF and ON show the manipulator LOSING (-3804 / -4851): the agent is
  NOT beaten by this nit-then-deviate adaptive strategy, either way.
- Search's effect vs an adaptive opponent is **unresolved**: the instrument
  (adaptive self-exploit at 3000 deals) is too unstable to say.

## To make it decidable

1. **Fixed, printed seed** for the manipulator run (so OFF reproduces).
2. **>= 20k deals** (3000 is too few; variance dominates).
3. Report the ON/OFF delta with a PAIRED comparison (same deals/seeds), which
   cancels the common variance.

Until then, search's adaptive value is an open question, and its scripted-
ladder cost (-2104, INSIGHT-RUN-2026-10-06) stands.
