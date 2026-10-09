# W3 gate result — shipped blueprint (2026-10-09)

## Result

`combo_gadget_w3_shipped.rs`, one river board (40/41/42/43/44), hero
range `[[10,11],[12,13],[14,15]]`, villain range `[[16,17],[18,19],[20,21]]`,
shipped blueprint `artifacts/agent-honest-19dim/robust`:

    VBR(shipped blueprint)     = 245.206 chips = 2.45206 bb
    VBR(resolved, gadget on)   = 156.250 chips = 1.56250 bb

**Resolved is 36% less exploitable than the shipped blueprint.**

## Why this matters

The plan's F-8 finding (reproduced this session, `SEARCH-ON-OFF-
EXPLOITABILITY-2026-10-09.md`) is that the class-conditioned live search
makes the agent *more* exploitable, not less: search ON = +7.36 bb worse
than search OFF on `search_exploitability`. That's the problem Phase D
was built to fix.

This result is the first evidence that the combo-level solver with the
safe-resolving gadget reverses that. On a real river, with the real
shipped blueprint as the gadget's bound, the resolved strategy beats the
blueprint on the plan's own metric.

## The gate, in the plan's words

> VBR(blueprint + resolve) ≤ VBR(blueprint).

2.45 → 1.56 satisfies this.

## Caveats — read these before quoting the number

1. **One river, one board.** Not an average over boards. To claim a
   session-level improvement requires the full D1-style sweep.
2. **Synthetic 3-combo ranges.** The real tracker produces wider ranges
   (30+ combos). Behaviour may differ on a wider range — the solver has
   more combos to regret-match and the gadget's CFV bound is looser.
3. **Not through the pipeline.** This test drives `RiverCfr` directly.
   The pipeline dispatch landed (`9d417cb`) but the harness in
   `search_exploitability.rs` cannot exercise it (see
   `COMBO-PATH-MEASUREMENT-BLOCKER-2026-10-09.md`).
4. **2.45 bb on one river is not the 5.77 bb full-game D1 number.** The
   two measure different things (river subgame vs full 4-street game,
   3 combos vs 30, single board vs 180).

## What this settles

The gadget is correctly sized: the resolved strategy is *no worse* than
the bound, on a real blueprint. The direction is right: combo+gadget
reduces exploitability where class search increased it.

## What this does not settle

Whether the pipeline path (tracker ranges → combo solver → gadget →
sampled action) reduces the full-game VBR. That needs the harness fix in
`COMBO-PATH-MEASUREMENT-BLOCKER-2026-10-09.md` or a fresh exploitability
harness that threads `State`.

## Command

    env -i PATH="$PATH" HOME="$HOME" \
      CHAM_D1_BP="$PWD/artifacts/agent-honest-19dim/robust" \
      CHAM_D1_BUCKETS="$PWD/artifacts/agent-honest-19dim/buckets" \
      target/debug/deps/combo_gadget_w3_shipped-<hash> --ignored --nocapture
