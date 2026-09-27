# Solver criticals C-2 / C-3 / C-4 — pending, with patch saved (2026-09-27)

## What this is

Follow-up to `docs/plans/chameleon-bug-report.md` criticals C-2, C-3, C-4
(all in `crates/cham-search/src/subgame.rs` + terminal handling in
`solve.rs`). C-1 (regret accumulation in `solve.rs`) was landed this
session and all `cham-search` tests passed.

The C-2/3/4 patch was authored but **reverted** because it changes the
game's payoff scale, which invalidates the numeric calibration of two
existing tests:

- `rnr_p_interpolation` — the "hero aggression monotone in p" proxy
  (`sum of probs[last_action]` over all infosets) no longer moves
  monotonically once the solver actually converges. The proxy was
  calibrated against a near-uniform solver.
- `reach_gadget_safety` — `gadget.lbr_gap.0.abs() < 10.0`. Under the
  corrected payoff model (which now includes `pot_bb` in showdown wins),
  the same fixture's `lbr_gap` is ~30.

Neither failure indicates a bug in the C-2/3/4 fixes; both indicate that
the tests were calibrated to the old (buggy) model. Rebasing them to
test the CORRECT property (per-fixture Nash value, real EV monotonicity)
is real work — do it in a dedicated session, not as a rider on top of a
running full-abstraction training job.

## The saved patch

`/tmp/c234-subgame-model.patch` (session-local; regenerate if gone — see
below for what it does).

## What the patch changes

### `Node::Terminal` gains a `kind: TerminalKind`

    pub enum TerminalKind {
        FoldByHero,     // hero loses his river investment
        FoldByVillain,  // hero wins pot_bb + villain's river investment
        Showdown,       // class-dependent, includes pot_bb
    }

The old `Node::Terminal` had no kind and every terminal — including folds
— went through `showdown_value`. That caused C-3 (winning bluffs with the
weaker class LOST money: a fold payoff depended on hole strength). The
new `terminal_value(kind, ...)` pays folds class-independently.

### `showdown_value` now includes the pre-river pot (C-4)

    // win:  pot_bb + villain_invested
    // lose: -hero_invested
    // split: (pot_bb + villain_invested - hero_invested) / 2

The old code credited only the villain's river money, so a checked-down
winner netted 0 and the win/lose swing was `h+v` instead of `pot + h+v`
— a different game than poker.

### `villain_node`'s call child matches the bet (C-2)

The call child now passes `villain_invested: hero_invested` (was passing
`villain_invested` unchanged — so a called bet paid the same as a check).
Value-betting the nuts paid 0.

### `villain_node`'s raise cap uses villain's stack (M-1)

    // before: raise = (hero_bet * 2.2).min(self.stack_bb - hero_invested)   ← hero's stack, wrong
    // after:  raise = (hero_bet * 2.2).min(self.stack_bb - villain_invested) ← villain's stack

Prevents the raise option from silently vanishing on large hero bets.

### `hero_face_raise` gets the correct terminal kinds

- Hero folds → `FoldByHero` (class-independent).
- Hero calls → `Showdown` at `villain_invested` (both invested the same).

## Suggested test rebasing (when the model fix is landed)

1. Add a small Kuhn-style ground-truth fixture with a hand-computed Nash
   value. Assert the fixed solver's `evaluate()` on that fixture equals
   the hand value to a tolerance. If the fixed model can't reproduce a
   2×2 hand-computed Nash, the fix has a bug.
2. Replace `rnr_p_interpolation`'s proxy with an actual EV computation:
   `cham_search::solve::evaluate(&sg, &r.our_strategy, &r.their_strategy)`
   at each `p`, and assert that EV is non-decreasing in `p` (the property
   the test's own comment claims).
3. `reach_gadget_safety`'s `lbr_gap < 10.0` bound: re-measure on the
   fixture, set the bound to reflect the corrected scale, and add a
   comment citing this doc so the number's provenance is clear.

## Fix order dependency

This is item 1 in the bug report's "Suggested fix order" and must precede
any re-baseline of the committed oracle JSONs. C-1 was landed first
(independently useful and test-clean); C-2/3/4 + oracle rebaseline is a
single atomic change.
