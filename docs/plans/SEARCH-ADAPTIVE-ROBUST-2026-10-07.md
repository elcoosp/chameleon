# Search's adaptive benefit is robust across manipulator timings (2026-10-07)

## Result

`self-exploit` adaptive manipulator (nit-then-deviate) vs live `full`,
20k deals, controlled env, three switch points:

| switch_at | OFF | ON (gadget) | delta |
|---|---:|---:|---:|
| 20 | -3463.3 +/- 210.2 | -4652.4 +/- 251.8 | **-1189.1** |
| 40 | -3456.5 +/- 210.1 | -4645.5 +/- 251.7 | **-1189.0** |
| 80 | -3451.8 +/- 210.0 | -4642.9 +/- 251.6 | **-1191.1** |

Combined SE ~328 => each delta is **3.6 sigma**. The three deltas agree to
within **2 mb/seating**.

## What this establishes

Search's adaptive benefit (the victim is ~1189 mb/seating HARDER to exploit
with search on) is **robust across the manipulator switch points tested**,
not an artifact of one timing. Combined with the single-point result
(SEARCH-WORKS-2026-10-06), this is a solid, reproducible finding.

## Caveat: the switch points are all EARLY

`switch_at` is in HANDS; the run is 20,000 deals. Switch points 20/40/80 are
all in the first ~0.4% of the run, so every arm is post-switch for almost the
whole session. That is WHY the numbers barely move across the three: they are
nearly the same run. The test confirms search helps but does NOT stress late
switching.

A spread sweep (e.g. switch_at = 2000 / 10000 / 18000) would probe whether the
benefit holds when the manipulator stays 'nit' for most of the session and only
deviates near the end. That is the remaining adaptive question.

## Status

- Search works vs adaptive: **-1189 mb/seating, 3.6 sigma** (robust here).
- Search loses vs the scripted ladder: -2104 (safety costs exploitation).
- Both correct; search trades exploitation for robustness.

## Artifacts

artifacts/adapt-sweep-2026-10-07/{summary.txt,sw*-OFF.log,sw*-ON.log}
