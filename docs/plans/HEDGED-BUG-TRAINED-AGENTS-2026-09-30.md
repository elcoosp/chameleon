# The hedged bug was `TRAINED_AGENTS` missing "full-hedged" (2026-09-30)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


## The bug

`crates/cham-cli/src/cmd/guard.rs::TRAINED_AGENTS` listed 10 agent
names. Every one of them triggers `build_chameleon(agent, ...)` in
`hero.rs` when the ladder/probe/ab tools ask for an agent. Any name
NOT in the list falls through to `build_hero` → `CallBot`.

`"full-hedged"` and `"hedged"` were NOT in the list. So every
`ladder --fast --agent full-hedged` measurement produced by this
session (and every earlier measurement that used `full-hedged`) was
actually measuring **CallBot vs the archetype pool**, not the hedged
routing.

## The evidence

- **All four hedge-sweep thresholds (0.00, 0.20, 0.50, 0.80, 1.00)
  produced identical results within noise.** A correct implementation
  would have dramatically different outcomes at the endpoints. This
  was the initial signal.
- **`CHAM_HEDGE_DEBUG=1` instrumentation showed top_weight values of
  0.27–0.65, finite, growing.** So the hedged decision path itself is
  reachable and sensible — the probe command reaches it.
- **The probe command was also affected by the bug.** At 12:14 the
  probe printed "loader: unknown routing: hedged" — but that was
  before commit `88b8b4a` added "hedged" to `AgentMode::validate`.
  After the fix, the probe *did* reach the hedged code, which is why
  the debug instrumentation worked at 13:17. The ladder code path is
  different: it checks `requires_trained_artifacts` FIRST, and only
  calls `build_chameleon` when the agent is in the list.
- **`callbot: +0.0 ± 0.0` on every hedged ladder.** CallBot plays a
  fixed policy; against the `callbot` opponent (a mirror match), the
  expected value is exactly zero. That matches. And the +0.0 number
  is a smoking gun that the hero is the same CallBot the opponent is.

## Why this took hours to find

The probe command and the ladder command have different code paths for
constructing a hero:

- **probe** → `hero::build_chameleon_with_router(agent, ...)` directly.
  It validates the mode and refuses if `validate()` fails. After the
  `88b8b4a` fix, it works correctly.
- **ladder** → `requires_trained_artifacts(agent)` first; only if true,
  `build_chameleon`. If false, `build_hero(agent, ...)` → since
  `requires_trained_artifacts` was false, `build_hero` returns `CallBot`
  without even trying to build a ChameleonAgent.

The two paths disagree. `hero.rs::build_hero` has a redundant check:

    pub fn build_hero(agent: &str, depth_bb: i64) -> Result<Box<dyn Agent>, String> {
        if !crate::cmd::guard::requires_trained_artifacts(agent) {
            return Ok(Box::new(CallBot));
        }
        Ok(Box::new(build_chameleon(agent, depth_bb)?))
    }

Both `build_hero` and the ladder caller check
`requires_trained_artifacts`. If that set is incomplete, the whole
path silently degrades to CallBot. There is no logging, no warning,
no gate. The `FALLBACK_WARN_RATE` guard in guard.rs exists
specifically to catch silent-uniform-policy contamination — but it
triggers on `fallback_used` per decision, and CallBot doesn't set
that bit, so it stays quiet.

## The fix

Commit `[hash]`:
- `TRAINED_AGENTS` extended from 10 to 12 names (added `full-hedged`
  and `hedged`).
- New test `every_routable_agent_requires_artifacts` iterates the full
  set of aliases that `hero::routing_for` recognizes and asserts each
  is in the list.

## What the hedged ladder numbers actually meant

The "hedged is a disaster (−1 800)" result from
`HEDGED-ROUTING-BUG-2026-09-30.md` and
`HEDGED-SWEEP-CONFIRMS-PATH-BUG-2026-09-30.md` is **CallBot vs the
archetype pool**, not hedged routing. The correct interpretation:

- CallBot loses ~1 800 mb/seating against this pool (all cells
  negative except callbot's +0 mirror).
- The hedge routing, whatever its flaws, has never been measured.

## Re-measuring

The `full-hedged` ladder should be re-run with the fixed binary. A
single-mode run at threshold 0.50 is ~6 min in a quiet system. The
`full-hedged` line in `RESULTS-MATRIX-2026-09-29.md` should be
overwritten once the correct number exists.

## Impact on other work

- **Nothing else in this session's results is affected.** Every
  non-hedged agent ("full", "argmax", "full-mixture", "robust-only")
  was already in `TRAINED_AGENTS` and produced valid ChameleonAgent
  measurements.
- **The `PAR5M-ROBUST-LADDER`, `LADDER-ARMMAX-REPRODUCED`,
  `HYBRID-LADDER`, and `LBR-VS-LADDER` docs are all correct.**
- **The hedge-sweep script and its five logs should be discarded.**

## Related

- `HEDGED-ROUTING-BUG-2026-09-30.md` (identified the modes.rs
  validate bug and structural issues with the hedged router design)
- `HEDGED-SWEEP-CONFIRMS-PATH-BUG-2026-09-30.md` (mis-attributed the
  identical sweep results to a code-path bug; the real cause was the
  guard list)
- `HEDGED-DEBUG-PROBE-2026-09-30.md` (correctly disproved the NaN
  hypothesis and correctly left "the failure is elsewhere" — this doc
  answers where)
