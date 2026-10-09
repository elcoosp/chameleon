**Correction (2026-10-09, later):** the concern below is a metric
issue, not a solver issue. See `W3-METRIC-CORRECTION-2026-10-09.md`.
The gadget's guarantee is one-sided (agent's exploitability), and
that property holds on all 7 swept boards.

# W3 gadget scope — a self-play solver does not bound the two-seat sum

## The finding

The W3 sweep (`W3-GATE-SWEEP-2026-10-09.md`) reported a mean improvement
of -4.69 ± 2.15 bb from the gadget, but two boards went the wrong way.
Investigating board 13 and 15 with `exploitability_split`:

    board 13: VBR(bp)  490.6  (hero BR 197.1, villain BR 293.5)
              VBR(res) 560.4  (hero BR 504.9, villain BR  55.6)
              hero-side delta +307.8, villain-side delta -237.9

    board 15: VBR(bp) 1309.1  (hero BR  96.1, villain BR 1213.0)
              VBR(res) 1106.9 (hero BR  33.3, villain BR 1073.6)
              hero-side delta  -62.8, villain-side delta -139.4

The **gadget did its job**: on both boards, the villain's exploitability
drops. That is exactly what a one-sided safe-resolving gadget promises.
But on board 13 the **hero-side rose by 308 chips**, and the two-seat
sum rose with it.

## Why

`RiverCfr::solve` is a **self-play** solver. It runs CFR+ on the tree
with both seats' strategies as variables. The gadget adds a virtual root
decision for the villain ("terminate for v_bp, or play"). CFR+ then
finds *some* equilibrium of the modified game — but the hero's strategy
in that equilibrium is not constrained to be the "safe" one the gadget
was designed to produce.

The Burch/Brown-Sandholm (2014) guarantee — "resolved ≤ blueprint" — is
stated for **subgame re-solving**, not self-play: there is a searcher
whose strategy is being chosen, and an opponent whose range is fixed.
The gadget constrains the opponent's worst case; the searcher's strategy
is then safe *against a best-responding opponent* because the opponent's
best response is capped.

Our self-play formulation has no "searcher vs fixed range." Both seats
are free. So the theoretical guarantee does not apply to the two-seat
sum, and on some boards the sum rises.

## What this means

The W3 gate as written in `W3-GATE-SWEEP-2026-10-09.md`
("VBR(resolved) ≤ VBR(blueprint)") is **not** what the code enforces.
What the code enforces is weaker: "the villain's exploitability is
capped by their blueprint CFV." That is a real property and it is
verified by the two-board dump — but it is not the property the plan
asks for, and it is not sufficient to make the live agent safer in the
aggregate.

## What to do

To actually satisfy the plan's W3, `RiverCfr::solve` needs to become a
**hero-strategy choice against a fixed villain strategy** — not a
self-play solve. Concretely:

1. Fix the villain's strategy to the blueprint table (`bp_villain`).
2. Solve for the hero's strategy, either as:
   - **Best response** (hero maximizes, no mixing needed) — trivial,
     but then it isn't safe; the hero can be exploited in reverse by a
     non-blueprint villain.
   - **Safe response with the gadget** — the hero solves for a strategy
     that bounds its own exploitability against a best-responding
     villain, using the gadget as the constraint. This is the actual
     W3 formulation.
3. Either way, the hero's strategy is the output; the villain's stays
   fixed.

That's a solver rewrite: `solve_hero_safe(tree, hero_range, villain_range,
fixed_villain_strategy, v_bp_hero)` rather than a self-play `solve`.

## What is still true

- The gadget bounds the villain's exploitability. Verified on 2 boards.
- The self-play solver converges to a Nash equilibrium on a plain tree
  (no gadget): exploitability 0 to 1e-5 bb on toys and real rivers.
- The combo-level solver framework, tree builder, class expander, and
  bridge are all correct.
- The pipeline dispatch works and defaults to `Class`.

## What is not true

- "W3 gate passes on the shipped blueprint." The sweep shows the *mean*
  improves, but the property stated in the plan is not what the code
  produces, and two boards show the sum rising.
- The combo path is not yet a safe-resolving solver in the plan's sense.

## The honest next step

Before the pipeline flip is measured end-to-end, the solver must be
restructured as hero-vs-fixed-villain. That's a rewrite of `solve`,
estimated 4-6 hours. It is not a bug fix; it's the correction of a
conceptual mismatch that this session's tests exposed.

The single-board and sweep numbers are still meaningful as evidence
that the *machinery* works, and the mean improvement is real for the
self-play formulation. But the safety claim requires the rewrite.
