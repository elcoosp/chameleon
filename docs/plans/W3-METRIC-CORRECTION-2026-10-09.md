# W3 metric correction — the gadget's guarantee is one-sided

## What earlier findings got wrong

`W3-GADGET-SCOPE-FINDING-2026-10-09.md` said the solver needed a
hero-vs-fixed-villain rewrite. That was too strong. The **mechanism** is
right — the two-seat sum is not bounded by the gadget — but the fix is
a **metric correction**, not an algorithm rewrite.

## The correct reading of the theorem

Burch/Brown-Sandholm (2014) safe resolving guarantees:

> The opponent's best-response value against the resolved strategy is no
> higher than the opponent's best-response value against the blueprint.

When the AGENT is the hero, that is:

    br_villain(resolved) <= br_villain(blueprint)

one-sided. It bounds how much a best-responding opponent can extract
from the agent. It does **not** bound:

    br_hero(resolved)

which is "how much the hero can extract from the villain's strategy."
That can rise — a hero optimized against a specific opponent may be
worse at punishing *other* opponents.

The two-seat sum `br_hero + br_villain` mixes both directions. It's a
symmetry check at equilibrium (both equal, opposite). It is **not** the
right W3 gate, because the plan's problem is "search makes the agent
more exploitable" — one-sided.

## Confirmed on every board (7/7)

Sweep after the metric fix:

    board  2: agent-expl(bp)= 498.55  agent-expl(res)= 100.00  delta  -398.54
    board  3: agent-expl(bp)= 796.54  agent-expl(res)= 150.74  delta  -645.79
    board  9: agent-expl(bp)=  43.13  agent-expl(res)= -66.59  delta  -109.72
    board 13: agent-expl(bp)= 293.50  agent-expl(res)=  55.56  delta  -237.94
    board 15: agent-expl(bp)=1212.95  agent-expl(res)=1073.57  delta  -139.38
    board 17: agent-expl(bp)=1273.55  agent-expl(res)= 155.67  delta -1117.88
    board 19: agent-expl(bp)= 727.42  agent-expl(res)= 100.00  delta  -627.42

Mean delta = -468 chips = **-4.68 bb**. Every board improves.

Test asserts per board: `res_agent_expl <= bp_agent_expl + 1.0`. Any
future change that breaks the one-sided bound fails.

## Why boards 13 and 15 were not counterexamples

Under the correct metric, board 13 dropped 238 chips and board 15 dropped
139. The two-seat sum for board 13 *rose* +70 chips, but that rise was on
the hero side — the hero (our agent) extracting more from a weak
opponent, not the opponent extracting from us.

## The plan's W3 gate, corrected

The plan reads:

> VBR(blueprint + resolve) <= VBR(blueprint)

If `VBR` is the two-seat sum, this is not achievable by safe resolving.
If `VBR` is the agent's exploitability (opponent BR value), it is, and
it holds on all 7 boards. The latter is the correct interpretation.

**Reported gate:** `agent_exploitability(resolved) <= agent_exploitability(blueprint)`,
available via `RiverCfr::exploitability_split(s).1` when the hero is the
agent.

## 40-board confirmation (2026-10-09)

`combo_gadget_w3_sweep40.rs` — per-board disjoint ranges so all 40
survive:

    agent-expl(bp)   = 5.819 +/- 0.884 bb
    agent-expl(res)  = 2.516 +/- 0.701 bb
    delta (res - bp) = -3.303 +/- 0.441 bb
    z = delta/SE     = -7.49

**40/40 boards pass the one-sided gate.** One board (32) has delta 0.0
(both values -100 chips; the blueprint is already optimal there).
Every other board improves.

This settles the one-sided W3 finding: the combo+gadget solve reduces
the agent's own exploitability by 3.3 bb on a 40-board sweep, z=-7.5.

## 40-board at 15/side (the pipeline's cap width)

`combo_gadget_w3_sweep40.rs` with `CHAM_SWEEP_COMBOS=15` — the width
`expand_classes_to_combos_capped(.., 15)` produces in the live bridge:

    agent-expl(bp)   = 2.833 +/- 0.346 bb
    agent-expl(res)  = 0.616 +/- 0.167 bb
    delta (res - bp) = -2.217 +/- 0.221 bb
    z = delta/SE     = -10.03

**40/40 boards pass**, mean improvement -2.2 bb. The magnitude falls
from the 3/side case (-3.3 bb) as the blueprint gets better with a
wider range, but the effect is *more* significant (z=-10.0 vs -7.5).

This is the width the live pipeline uses (via the cap), so it confirms
the cap is not a workaround: the one-sided safety property holds at the
width the bridge actually produces.

## Width table — CORRECTED (2026-10-09, post 9627158)

**The width table below was measured with a buggy `draw_range`**: it
iterated `(i, i+1), (i, i+2), ...` so the first `n` combos all shared
`avail[0]` — the same concentration as the D1 harness before `af1b635`.
The fix (`9627158`) enumerates all pairs, seeded-shuffles, takes `n`.

**Corrected numbers (40 boards each):**

| width | agent-expl(bp) | agent-expl(res) | delta | z | status |
|---|---|---|---|---|---|
| 3 | pending | pending | pending | pending | not yet re-measured |
| 8 | pending | pending | pending | pending | not yet re-measured |
| **15** | **2.393 +/- 0.326** | **0.701 +/- 0.138** | **-1.692 +/- 0.222** | **-7.61** | re-measured |
| 30 | pending | pending | pending | pending | not yet re-measured |

The corrected 15/side number (**-1.69 bb, z=-7.61**) is smaller in
magnitude than the buggy-range one (-2.22, z=-10.0) — the concentrated
range was easier for the resolver to exploit. But the **direction and
significance hold**: the one-sided gate passes at 15/side with uniform
ranges.

**The conclusion is unchanged:** the gate holds, the pipeline cap of 15
is defensible. The exact per-width numbers below are from the buggy
construction and should not be quoted.

---

## Width table — ORIGINAL (buggy draw_range; do not quote)

## Width table — the pipeline cap is optimal

`combo_gadget_w3_sweep40.rs` at four range widths (40 boards each):

| width | agent-expl(bp) | agent-expl(res) | delta | z | wall |
|---|---|---|---|---|---|
| 3 | 3.84 +/- 0.72 | 1.01 +/- 0.56 | -2.83 +/- 0.40 | -7.04 | 14s |
| 8 | 3.01 +/- 0.38 | 0.66 +/- 0.17 | -2.35 +/- 0.27 | -8.76 | 81s |
| 15 | 2.83 +/- 0.35 | 0.62 +/- 0.17 | -2.22 +/- 0.22 | **-10.03** | 56s |
| 30 | 2.73 +/- 0.34 | 0.59 +/- 0.14 | -2.14 +/- 0.22 | -9.64 | 266s |

Two readings:

1. **The one-sided gate holds at every width.** No board anywhere in
   the table regressed. The gadget's guarantee is robust to the
   pipeline's `CHAM_COMBO_CAP`.
2. **Width 15 is the sweet spot.** Highest z (-10.03), moderate cost.
   Width 30 doubles the work for slightly worse significance, and its
   wall time (266s) is contaminated by a loaded machine but the trend
   is clear.

The pipeline default (`CHAM_COMBO_CAP=15`) is what the measurement
endorses; no change needed.

## What still needs work

- 40+ board sweep (only 7 boards survived the range/board filter here).
- Wider (30-combo) tracker ranges — synthetic 3-combo here.
- The **live** pipeline measurement: `search_exploitability` with
  `CHAM_SEARCH_IMPL=combo-gadget`. Still blocked by
  `COMBO-PATH-MEASUREMENT-BLOCKER-2026-10-09.md` — the harness cannot
  thread `State`.
