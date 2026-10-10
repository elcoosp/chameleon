# W2 — what to measure, and why (2026-10-09)

D2 failed in the way the plan predicted (`D2-RESULT-2026-10-09.md`): two
independent trainers converge to ~8.5-9 bb on the tiny abstraction. The
floor is the abstraction. The plan says W2 next.

## The W2 mechanisms (from the plan, §2 + F-6)

| mechanism | what it changes | bundle |
|---|---|---|
| Real preflop tree (3-bet/4-bet) | `preflop_levels_bb` adds 8bb, 20bb raises | `real-preflop` |
| Finer buckets (128/64/64/8) | 4-8x more postflop buckets | `real-full` |
| key v2 (v3 abstraction) | structural (C-3 split) | not built |
| More buckets | W2 proper | not built |

Two of those already have trained bundles (`real-preflop`,
`real-full`). **Both have river-slice numbers; neither has a full-game
D1.** This doc + the two runs launched alongside it give them.

## The measurements in flight

Both bundles, full-game D1, 180 boards, shuffled ranges:

    real-full:     artifacts/levers-bp/real-full/robust/robust-7/policy
    real-preflop:  artifacts/levers-bp/real-preflop/robust/robust-7/policy

Compare against:

    tiny (agent-honest-19dim):  8.5196 +/- 0.6208 bb  (180 boards)
    tiny-full:                 10.7319 +/- 0.6206 bb  (180 boards)

## What each outcome means

**If `real-full` < 8.5 by > 3 SE:** finer buckets help. W2's
"more buckets" mechanism is validated; build the v3 abstraction with
even finer buckets.

**If `real-full` ~ 8.5:** finer buckets do not move the full-game
number. Same as the river-slice result (all postflop abstractions at
~5.9 river slice). The floor is deeper than bucket count.

**If `real-preflop` > 8.5 (matching its 11.34 river-slice outlier):**
the preflop tree alone makes things worse — the prior is right that
preflop matters, wrong about direction.

**If `real-preflop` < 8.5:** real preflop is the fix. W2's
"real preflop tree" is validated.

## What the plan expects

Line 113 says: *"If PCS also plateaus ⇒ the floor is the abstraction,
skip to W2 immediately (more buckets/real tree) before more trainer
work."*

The plan does NOT say which W2 mechanism will help. That's what these
two runs answer. Only after seeing them should the next W2 build be
scoped.

## The `real-preflop: 11.34` river-slice outlier

From `artifacts/insight-10h-2026-10-07/summary.txt`, the river-only VBR
of each abstraction:

    tiny-full        5.890
    rlf-g2           5.926
    rlf-cfr          5.956
    rlf-12m          5.885
    real-full        5.870   (128/64/64/8)
    real-preflop    11.343   (real 3-bet/4-bet, tiny postflop)

The **only** abstraction whose river slice differs is `real-preflop`.
All the bucket-count variants are indistinguishable on the river. If
the pattern holds in full-game, bucket fineness is not the lever and
the preflop tree is.

The two runs settle it.
