# Results matrix — every measured config, one table

Every number below is LBR at depth 100, 200 deals (unless noted), lower is
better. All at γ = 1.0 except where marked.

## Tiny abstraction (21k infosets)

| mode | iters | wall | seat 0 | seat 1 |
|---|---:|---:|---:|---:|
| serial 500k | 500k | 10 min | 23 280 | 13 957 |
| parallel 5M | 5M | 47 min | **15 040** | **12 050** |
| parallel 20M | 20M | ~2 h | 13 809 | 14 956 |
| parallel 50M | 50M | in flight | — | — |

## Full abstraction (380k infosets)

| mode | iters | wall | seat 0 | seat 1 |
|---|---:|---:|---:|---:|
| parallel 9M | 9M | 3.6 h | 18 079 | 14 298 |

At matched visits/infoset (tiny-500k vs full-9M, both ~24):
- SB: 18 079 vs 23 280 → **full wins by 22 %**
- BB: 14 298 vs 13 957 → tiny wins by 2 %

## Medium abstraction (in flight, 84k infosets expected)

| mode | iters | wall | seat 0 | seat 1 |
|---|---:|---:|---:|---:|
| parallel 20M | 20M | ~5 h | — | — |

## Baselines vs the archetype pool (ladder, not LBR)

| agent | mean mb/seating | wins |
|---|---:|---:|
| uniform | ≈0 | — |
| full-mixture (synthetic router) | +4 388 | 6/9 |
| full (argmax) | **+7 146** | **9/9** |
| full-hedged | −1 994 | 0/9 |

## What the frontier looks like right now

**Best LBR seat 0:** tiny 20M parallel at 13 809.
**Best LBR seat 1:** tiny 5M parallel at 12 050.
**Best mean:** tiny 5M parallel at 13 545.
**Best against the pool:** tiny 500k argmax at +7 146 ladder mean.

The tiny 5M artifact is the current shipping candidate — it minimizes
mean exploitability and is cheap to produce (47 min).

## The remaining unknown

Whether medium-20M produces a better frontier point than tiny-5M or
tiny-20M. If it does, the answer is "use medium with more iters". If not,
the answer is "the abstraction does not matter much below 380k, use tiny
with as many iters as budget allows".

The medium run finishes ~18:30; tiny 50M finishes ~04:00.
