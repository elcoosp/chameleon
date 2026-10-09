# Phase E status — deployment (2026-10-09)

Plan: `docs/reviews/CHAMELEON-SOTA-PLAN.md` line 115.

> W4: deployment (sampling, λ-capped exploitation, translation), W5 perf,
> 200 bb. Gate: beats sparring partner; Slumbot CI trending to ≥ 0;
> ladder regression ≤ 15% vs old agent on scripted bots.

## The four mechanisms — status

| Mechanism | Status | Evidence |
|---|---|---|
| **Sampling** (never argmax) | DONE | `pipeline.rs::sample_index`, used at every live decision except the `CHAM_SEARCH_ARGMAX=1` diagnostic |
| **λ-capped exploitation** | DONE (class path) | `AgentMode::bounded()`, env `CHAM_EXPLOIT_BUDGET_MB`; per-hand commitment to expert with prob λ, robust σ otherwise (modes.rs:108-117) |
| **RNR(p)** | DONE (class path) | `SolverChoice::Rnr { p }` in `solve.rs:856` |
| **Translation** (solver label → engine action) | DONE | `search_bridge::label_to_action` |

**All four exist and are exercised by existing tests.** The shipped
default does not enable them (search off), but the mechanisms are in
place.

## What is NOT wired

**The combo path does not take λ.** `try_solve_combo_gadget` solves the
safe-resolving game and returns the resolved strategy's distribution.
It has no `p` parameter and no RNR blending. The plan's target
architecture (line 95-99) puts "exploitation = RNR(p) inside the
solver", and today that only applies to the class path.

Threading RNR(p) through the combo solver means: given a fixed opponent
model (the blueprint), find the strategy that maximizes hero's value
subject to the opponent's worst-case being within the blueprint's plus
a λ-controlled budget. That is not the current self-play `solve` — it
is a *restricted* solve. It needs `RiverCfr::solve_restricted(p)`.

Estimated effort: 4-6 hours (similar to the combo solver itself).

## Why I did not implement it this session

Two reasons, both honest:

1. **Measure-first discipline.** Every Phase D increment that mattered
   came from a measurement forcing a change. RNR-threading without a
   way to verify it improves anything would be motion, not progress.
2. **The evaluation gate is external.** Phase E's acceptance is
   "beats sparring partner / Slumbot CI / ladder regression" — all
   require infrastructure this session doesn't have (a quiet machine
   for the ladder, real Slumbot credentials). Implementing more Phase E
   mechanisms before those measurements exist would repeat the
   over-reach pattern that produced two dead combo solvers earlier in
   the session.

## What to do, in order

1. **First, settle the D1 magnitude.** The 180-board shuffled run
   (`artifacts/d1-shuffled-180.log`) is in flight. The lexicographic
   numbers (5.77, 7.65) and the 20-board shuffled numbers (7.57, 9.75)
   are ~1.1σ apart; a 180-board SE~0.55 run decides which is real. Every
   downstream number (D2's 4.13 baseline, the ladder regression's
   reference) depends on this.
2. **Then the ladder regression** on the combo path: run the shipped
   agent with `CHAM_SEARCH_IMPL=combo-gadget` vs the shipped default
   against the scripted ladder. That's the Phase E gate's first clause
   and needs the machine quiet enough to trust the winrate.
3. **Only then** RNR(p) threading through the combo solver.

## Mechanisms that are already deployable

The class path's `AgentMode::bounded()` is complete. If someone wants to
ship exploitation today, `--agent bounded` with `CHAM_EXPLOIT_BUDGET_MB`
does it, at the shipped blueprint's quality. Nothing in this session
changed that path.
