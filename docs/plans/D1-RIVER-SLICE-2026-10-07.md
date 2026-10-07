# D1 river slice: 5.85 +/- 0.15 bb/hand (2026-10-07)

## The measurement

The VBR (vector best-response) against the SHIPPED blueprint
(`agent-honest-19dim/robust`), river-only, 20 boards, spread ranges,
villain policy queried from the VILLAIN seat with a per-path table:

    mean VBR: 5.850 +/- 0.150 bb/hand   (20 boards)

SE tiny (0.150) => the number is solid.

## What it is

The exploitability a PERFECT river player extracts from the blueprint's
river play, reached via a fixed call/check line, ONE seat. It is a
**lower bound** on the full-game exploitability (the full VBR also
optimises preflop/flop/turn, so full >= river-slice).

## What it means for D1's three-way question

The tabular BR (same-abstraction, full game, both seats) gave 8.19 bb.
The river slice ALONE is 5.85 bb -- a large fraction of that.

Reading:
- The blueprint's river play is **genuinely, significantly exploitable**
  (~5.85 bb on one street). The old "≈0 exploitable" claims were wrong;
  they were the under-converged learner (`FULLCOV-AND-THE-BOUND`).
- The tabular 8.19 is in the right BALLPARK (not a wild artifact), and is
  likely an **UNDERESTIMATE**: the full-game VBR (all streets) must be
  >= the river slice + the earlier-street exploitability.

So D1 leans toward the plan's outcome #1/#2: **the blueprint really is
that exploitable, and the same-abstraction BR may understate it.**

## Caveats

- River only. The full-game VBR is the complete D1.
- One seat (hero = BB).
- The villain policy is queried at river nodes reached via one line; deeper
  within-node re-solves are not modelled (the opponent is a fixed table).

## Next (complete D1)

- Full-game VBR: preflop + flop + turn + river, both seats, summed.
- That needs the engine tree wired into the VBR recursion (the public
  betting tree, not just the river), which is the substantial piece.

The river slice already tells us the blueprint is genuinely exploitable,
which is enough to say: **Phase C (a real trainer) is mandatory** -- no
amount of knob-tuning fixes a blueprint that loses 5.85 bb on the river
alone.
