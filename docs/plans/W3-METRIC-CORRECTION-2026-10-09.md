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

## What still needs work

- 40+ board sweep (only 7 boards survived the range/board filter here).
- Wider (30-combo) tracker ranges — synthetic 3-combo here.
- The **live** pipeline measurement: `search_exploitability` with
  `CHAM_SEARCH_IMPL=combo-gadget`. Still blocked by
  `COMBO-PATH-MEASUREMENT-BLOCKER-2026-10-09.md` — the harness cannot
  thread `State`.
