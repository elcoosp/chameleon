# Combo path measurement blocker — 2026-10-09

## The problem

The combo-gadget path (`SolverImpl::ComboGadget`) is landed and reachable
through `pipeline.rs::act_impl`, but **cannot be measured** with the
existing `search_exploitability` harness. Running it with
`CHAM_SEARCH_IMPL=combo-gadget` would silently measure the class path and
produce a false null.

## Why

Two structural mismatches between the harness and the combo path:

1. **The harness calls `try_solve` directly**, not through the pipeline.
   `SearchPolicy::dist` (search_exploitability.rs:57) has:

       try_solve(&self.scfg, &self.tracker, &self.enc, &self.robust,
                 obs, seq, None)

   The `None` is the `state` argument. The dispatch on `SolverImpl` lives
   in `pipeline.rs::act_impl`, which the harness never touches.

2. **The combo path needs a live `State`.** The harness closure's
   signature is `FnMut(&Observables, &ActionSeq) -> Vec<(Action, f64)>`
   (search_exploitability.rs:48) — no `State` is threaded through. Even
   if `dist` called `try_solve_combo_gadget`, it has no state to pass.
   The tabular BR harness (`tabular_br`) doesn't expose one.

## What would need to change

Two options:

**Option A: change the harness's closure signature.**
- `SearchPolicy::dist(obs, seq)` → `dist(obs, seq, state)`.
- `tabular_br`'s closure type gets a `&State` parameter.
- `measure` obtains a `State` at each decision (it already drives a
  game internally, so it has one) and passes it in.
- `dist` then dispatches on `self.scfg.impl_kind` the same way
  `pipeline.rs::act_impl` does.
- The env var `CHAM_SEARCH_IMPL=combo-gadget` selects the path.

Estimated: ~150 lines across `search_exploitability.rs` and possibly
`tabular_br` (in `cham-eval` or `cham-opponents`). Testable incrementally.

**Option B: measure through the pipeline.**
- Build a `ChameleonAgent` with `search.enabled = true` and
  `CHAM_SEARCH_IMPL=combo-gadget`.
- Run full hands against a fixed opponent.
- Read `last_trace.search` to confirm the combo solver fired.
- Measure exploitability of the resulting policy with the tabular BR
  harness (needs the policy to be extractable, which it isn't today —
  `ChameleonAgent` doesn't expose its post-search strategy).

Option A is smaller. Option B measures the *actual* pipeline end-to-end
but needs more plumbing.

## What this session does NOT do

Attempt either. The session is very long; the machine is loaded; the
next real measurement is a 50-minute run on a quiet machine, gated by a
harness change that is itself a substantial commit. The right time to
start Option A is a fresh session.

## What is documented and correct

- The class solver is +7.36 bb more exploitable than no search
  (`SEARCH-ON-OFF-EXPLOITABILITY-2026-10-09.md`).
- The combo solver passes all its unit tests, the W3 gate, and the
  integration smoke. It has not been measured on the exploitability
  harness because the harness can't reach it.
- The `SolverImpl` dispatch is in the pipeline and defaults to `Class`.
  No shipped behavior changed.

## Next session, first actions

1. Do Option A (~150 lines, testable incrementally).
2. Run `search_exploitability` twice: once with `CHAM_SEARCH_IMPL=class`
   (the +7.36 baseline), once with `combo-gadget`. Compare.
3. If `combo-gadget` shows ≤ 0 delta or a large improvement, the combo
   path is validated and Phase D's D2-adjacent claim is supported.
4. If `combo-gadget` is still ~+7 bb, there's a solver bug or the gadget
   is mis-sized against the blueprint — worth debugging before any
   live wiring.
