# SPECS/08 amendment (draft) — full AIVAT baseline-value stage (roadmap §2.1 step 2)

Status: SPEC ONLY — do not write code until this amendment is accepted,
because it changes the ledger's `mb/seating` semantics and every downstream
gate threshold must be re-derived against the new estimator's variance.

## Problem

`vr.rs::allin_ev_adjusted` (B4, shipped, wired into `matcheng`) removes
luck ONLY at all-in showdowns. With preflop all-ins now covered (memoized
`vr::preflop_equity`), the remaining variance is runout luck in NON-all-in
pots: the river card that completes the flush, the turn that pairs the
board. Full AIVAT (Shi et al.) removes that too.

## Proposal (baseline-value subtraction at every decision point)

At each public decision point (street boundary + each action), subtract an
estimated baseline value of the position and add it back at hand end —
control variates, mean-zero by construction:

- **Baseline source: the blueprint's own average strategy.** No new
  opponent model needed ("use tooling you already have", v2 self-exploit
  idea). For the observed public history, look up the blueprint's average
  strategy value of each hero holding class — offline-computable from the
  trained artifact, deterministic, bit-exact given the artifact hash.
- **Where it applies:** `matcheng::play_seating`, at every `state.apply`
  where the street advances or a bet is called — record
  `(baseline_before, baseline_after)`; adjusted net = realized net −
  Σ(baseline deltas). Unbiased iff baselines are fixed before the hand
  (they are: artifact-bound).
- **Symmetry:** applied identically on both duplicate seatings, so the
  duplicate mean stays fair and paired A/B streams stay paired.

## Kill criterion (roadmap §2.1)

If `vr_factor` telemetry (now first-class on every `ab`/`ladder` printout)
shows flop/turn/preflop all-in adjustment already captures >90% of
achievable variance reduction on the actual pool mix — i.e. non-all-in
runout luck is small at the trained agent's ranges — this stage moves DOWN
the stack: measure before building. The number to watch is the residual
`se_mb` after adjustment vs the ±25 mb/seating gate target.

## Ledger semantics change (must land with the code, not before)

`LedgerEntry` gains `estimator: "allin-ev" | "aivat"` (default:
`"allin-ev"` for all existing entries). Gate thresholds in
`experiments/*.toml` are estimator-qualified: an `allin-ev` CI and an
`aivat` CI for the same matchup are NOT comparable numbers.
