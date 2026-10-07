# D1 preliminary: VBR pipeline works, river-slice ~3.7 bb (2026-10-07)

## What ran

The plan's VBR (vector best-response) against the SHIPPED blueprint
(`agent-honest-19dim/robust`), river-slice, 3 boards x 60 combos:

    board 0: VBR 3.694 bb
    board 1: VBR 3.694 bb
    board 2: VBR 3.694 bb
    coverage: 180/180 combos (100%)

The blueprint IS queried (coverage 100%, VBR moved 4.22 -> 3.69 when the
encoder + seq were fixed), so the pipeline is real.

## What this number is NOT

Three known simplifications mean 3.694 is a FIRST CUT, not the honest D1:

1. **Path-independent policy.** The opponent plays its ROOT strategy at
   every node; the true VBR re-queries the blueprint per (state, seq).
2. **Opponent keyed by hero seat.** The policy closure is built from the
   blueprint queried from HERO's view; the opponent should be queried from
   VILLAIN's seat. Robust self-play makes this roughly symmetric, but it is
   not correct.
3. **Degenerate ranges.** Hero and villain both use the first 60 combos by
   index (low cards), and the dummy villain holes are the same per board, so
   the three boards give identical structure. The 3.694 is one spot, not a
   river average.

## What IS established

- The VBR module (kernel + full tree) is **validated against a recursive
  brute force** (vbr_validate).
- The blueprint can be queried at real river keys with correct
  encoder + seq (100% coverage).
- The pipeline (engine state -> combos -> ranks -> policy table -> VBR)
  works end to end.

## To finish D1 (the honest number)

1. Query the blueprint from VILLAIN's seat for the opponent policy.
2. Per-node blueprint queries (thread the engine State through the VBR
   recursion), or a per-path policy table.
3. Non-degenerate ranges: hero = a spread of combos, villain = a spread,
   dummy holes varied per board.
4. Aggregate over many boards (river spots), report with SE.

Then D1's three-way readout (real ~8-10 bb / abstraction leak / metric
artifact) is answerable.
