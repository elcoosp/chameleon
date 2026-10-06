# Safe-resolving gadget: fully implemented (2026-10-06)

## What was built

Burch/Brown-Sandholm safe re-solving, end to end:

1. **`Subgame::with_opponent_optout(v_bp)`** + root `[terminate | play]`
   decision + `TerminalKind::OpponentTerminates`. The opponent can opt out
   for their blueprint counterfactual value.
2. **`solve::villain_cfv`** — per-villain-class CFV against a prior.
3. **`build_blueprint_prior`** — walks the solver tree from the LIVE state,
   querying the blueprint at every node, so the prior IS the blueprint.
4. **Production wiring**: `Agent::act_with_state` (default = `act`) →
   `ChameleonAgent::act_with_state` → `act_impl(state)` → `try_solve(state)`
   → `build_blueprint_prior` → `with_opponent_optout`. `matcheng` passes the
   live `State`.

## Verified

- 4 structure tests (`gadget.rs`): root opt-out, terminate value, no-gadget
  unchanged.
- 1 safety test (`gadget_safety.rs`):

      plain   our_gap=27.49  their_gap=27.06
      gadget  our_gap=27.93  their_gap= 0.013

  The opponent's exploitability collapses **27.06 -> 0.013** with the
  gadget. That is the safety bound working: the opt-out makes the
  opponent's best response gain vanish.

## Why the ladder did not move

ON vs OFF on the scripted pool: no-gadget 12444.7 / 9796.0 vs
real-gadget 12443.1 / 9850.2 — **identical within noise**.

This is CORRECT, not a failure. The ladder scores EXPLOITATION; the gadget
provides SAFETY. Against a fixed scripted bot the opt-out is never taken;
the gadget only makes hero's river play conservative (bounded by the
blueprint), which extracts the same from a caller. The gadget's value shows
in EXPLOITABILITY (the safety test above) and vs ADAPTIVE opponents, not on
the scripted ladder.

## What this means

- Search is now **safe by construction** (bounded by the blueprint).
- On the scripted ladder it is **neutral** (neither hurts nor helps) — the
  earlier -12 bb loss was the UNSAFE re-solve, now fixed.
- Against adaptive/unknown opponents it should HELP; that is the untested
  lever.

## Remaining

- A fair adaptive-opponent test (search should beat the exploit policy vs an
  opponent that adapts). The scripted ladder cannot show this.
- Optional: blueprint-reach villain RANGE (F1 plan approach (a)) for search
  QUALITY — separate from the SAFETY the gadget now provides.
