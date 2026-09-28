# Trainer convergence — the actual bottleneck (2026-09-28)

## The number, measured correctly

Instrumented self-play on the 500k-tiny policy:

* **100% key-hit rate** — the policy is being consulted, not falling back.
* **SB root distributions are poker-like** — not uniform, not degenerate.
* **Self-play SB net: +1.84 bb/hand.**  Equilibrium HU at 100bb has SB ≈
  +0.15 bb/hand. So the policy is losing ~1.7 bb/hand *to itself* beyond
  what position entitles it to.

And LBR (abstract best response) at 200 deals:

| policy          | iters | d100 seat0 | d100 seat1 |
|-----------------|-------|-----------:|-----------:|
| uniform         | —     |     48 186 |     28 013 |
| trained tiny    | 50k   |     44 004 |     20 271 |
| trained tiny    | 500k  |     39 692 |     15 068 |
| trained tiny    | 5M    |     36 665 |     18 205 |
| (pre-L-1 diag)  | 3M    |     36 746 |     16 536 |
| (pre-L-1 diag)  | 10M   |     36 796 |     21 333 |

## What the curve says

* 50k → 500k: ~10% improvement on seat 0, ~25% on seat 1.
* 500k → 5M: no improvement (within the ~2–3 bb/hand seed noise).
* 3M → 10M (independent pre-L-1 data): seat 0 flat, seat 1 **worse**.

So the trainer improves for a while, then plateaus. **More iterations do
not lower exploitability past ~500k on this abstraction.**

## Two hypotheses, and the discriminating experiment

**(A) Sample-limited.**  5M / 21k infosets = ~250 visits/infoset.  CFR+
on HUNL commonly needs millions of visits per infoset to reach low
exploitability.  Under this hypothesis, LBR keeps dropping if you push
to 50M/500M.

**(B) A high-visit-count bug.**  f32 accumulation, averaging-weight
underflow, or an integer index that misbehaves past some size.  Under
this hypothesis LBR plateaus or rises.

**The seat-1 regression from 3M to 10M leans toward (B)**, but it is
inside the plausible noise band, so we need a longer run to decide.

**Experiment launched: 50M iters, tiny robust, seed 7, `nice -n 10`.**
Output: `artifacts/blueprints-retrain-tiny-50M/robust-7/`.
Runtime estimate: ~16 h at the observed 5M rate (5975 s).

## Meanwhile: the f32 magnitude check

`strat_probe <table.snap>` prints max regret, max strat_sum, max
avg_weight, max visits.  If `max strat_sum` exceeds ~1e7 on the 5M table,
f32 arithmetic is already dropping digits on every add — a candidate
mechanism for (B).

## Other things already ruled out

* The abstraction hash guard, key format, and payload hash are correct:
  self-play at 100% hit proves the encoder/artifact pairing.
* The betting tree is not the bottleneck: a rich-ladder variant at 50k
  iters (16k → 305k infosets) came out no better (43.8 vs 44.0 seat 0).
* RBP is disabled by default (theta0 = 0 → prune_enabled = false).

## What to do with the 16-hour wait

1. Let the full-abstraction expert chain finish (nit done; tag/lag/
   station/robust pending) so we have a full-abstraction bundle to
   compare against the tiny plateau.
2. Look for the actual algorithm bottleneck.  Candidate reviews:
   * The averaging-weight formula (`w_t = (t−D)·γ^(T−t)` with `D = T/4`)
     — is the delay too aggressive at high T?
   * `w_t` is applied as `add_strat(off, w, a, w_t * sigma[a] as f32)`.
     At T = 50M, w_t is ~3.75e7 — f32 has ~7 digits, so the *product*
     loses precision.  This is a real candidate for (B).
3. Prepare a GPU/parallel trainer variant if (A) is confirmed — the
   single-threaded trainer at 16 h for 50M is the wrong shape for the
   iterations we might need.
