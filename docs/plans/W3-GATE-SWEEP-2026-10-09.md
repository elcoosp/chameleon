# W3 gate sweep — 7 boards, shipped blueprint (2026-10-09)

## Result

`combo_gadget_w3_sweep.rs`, `CHAM_W3_BOARDS=20` (7 boards survived the
range/board collision filter), shipped blueprint `agent-honest-19dim`:

    VBR(shipped blueprint):  7.2366 +/- 1.4266 bb
    VBR(resolved, gadget) :  2.5470 +/- 1.6120 bb
    delta (res - bp)      : -4.6896 +/- 2.1526 bb
    z = delta/SE(delta)   : -2.18

**Per-board:**

    board 2:  VBR(bp)=  398.55  VBR(res)=    0.00  chips
    board 3:  VBR(bp)=  717.42  VBR(res)=   55.73  chips
    board 9:  VBR(bp)=  349.04  VBR(res)=    4.16  chips
    board 13: VBR(bp)=  490.56  VBR(res)=  560.42  chips    ← gadget lost here
    board 15: VBR(bp)= 1309.08  VBR(res)= 1106.90  chips    ← gadget lost here
    board 17: VBR(bp)= 1173.55  VBR(res)=   55.67  chips
    board 19: VBR(bp)=  627.42  VBR(res)=    0.00  chips

## What this settles

**Correction (2026-10-09, later):** the property the code actually
enforces is weaker than the sweep's headline claim. The gadget bounds
the VILLAIN's exploitability, not the two-seat sum. See
`W3-GADGET-SCOPE-FINDING-2026-10-09.md`. The mean improvement is real
for the self-play formulation; the plan's stated W3 gate requires a
hero-vs-fixed-villain rewrite.

The single-board result (`W3-GATE-RESULT-2026-10-09.md`: 2.45→1.56 bb)
generalizes. Across boards the gadget-bounded combo solve is **-4.69 bb
less exploitable** than the shipped blueprint, at z = -2.18 (about 1.5%
one-tailed). That is the direction the plan's Phase D predicted, and the
opposite of the class solver's +7.36 bb harm.

## What it does not settle

1. **Two boards went the wrong way** — 13 and 15, both with high base
   VBR (490, 1309 chips). On those boards the resolved strategy is *more*
   exploitable than the blueprint. The gadget bound is not absolute here;
   the resolved value exceeds the bound on ~30% of boards. That's either
   (a) the finite iters at those boards not converging, (b) the gadget's
   `v_bp` mis-sized on wide hero ranges, or (c) a bug when the blueprint
   CFV is far from 0. **Needs investigation.**
2. **7 boards is small.** z = -2.18 is real but not decisive; 40+ boards
   would move it to z ≈ -4 if the signal holds.
3. **Range/board collision filter dropped 13 of 20 boards.** The fixed
   range hits many random boards. A proper sweep picks ranges per board
   or uses a wider pool.
4. **Synthetic 3-combo ranges.** Real tracker ranges are 30+ combos.
5. **River-only.** The full-game number is 5.77 bb (D1). This measures
   the river subgame, not the live pipeline.

## What to do next

- **Investigate boards 13 and 15.** The gadget is supposed to *bound*
  the resolved value by the blueprint's; when it doesn't, the bound is
  either mis-computed or the solver isn't converging at those
  configurations. This is the one result that contradicts the gadget's
  theoretical guarantee and deserves priority.
- **40+ board sweep** with a range construction that survives more
  boards (draw from disjoint half-decks, like the D1 harness).
- **Full-game check.** Once the harness in
  `COMBO-PATH-MEASUREMENT-BLOCKER-2026-10-09.md` is fixed, run the
  same ON-vs-OFF comparison against the combo path, not the class path.

## Command

    env -i PATH="$PATH" HOME="$HOME" \
      CHAM_D1_BP="$PWD/artifacts/agent-honest-19dim/robust" \
      CHAM_D1_BUCKETS="$PWD/artifacts/agent-honest-19dim/buckets" \
      CHAM_W3_BOARDS=20 \
      target/debug/deps/combo_gadget_w3_sweep-<hash> --ignored --nocapture
