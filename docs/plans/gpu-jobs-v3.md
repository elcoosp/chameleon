# v3 Track D1 — GPU jobs, ranked (roadmap §4.2, consumer identified)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


Determinism fence (unchanged): GPU output enters `artifacts/` only after
byte-exact CPU validation; the online decision path stays scalar CPU
forever; any kernel that can't be validated bit-exact is rejected
regardless of speed. Same bar as the G1.2 precedent, no exceptions.

1. **A2 abstraction rebuild (§4.1) — top of the queue.** Bulk-enumerate the
   exact next-street equity histograms (`next_street_cdf_exact` semantics:
   47 turn cards / 46 river cards per orbit, exact `equity_exact` each) for
   the full orbit enumeration (flop ≈ 1.29M, turn ≈ 55M orbits). CPU rate
   (~80 ms/flop-orbit) makes this GPU-or-nothing at full scale; at the
   proven 2.65–3.57e9 evals/s pipeline rate it is a weekend job. CPU
   `BuildParams::exact()` (2000 sampled orbits) is the bit-exact validator.
   Consumer: `train-buckets --profile exact` → same bucket count, same
   table shape, better partition. Kill: <10% exploitability improvement on
   the §2.3 bench → stop, feature set was adequate.
2. **Best-response / exploitability sweeps (§2.3).** Embarrassingly parallel
   across sampled subgames, same pipeline shape as job 1. Feeds the
   minutes-scale Track A loop.
3. **Preflop all-in completion table (§2.1 step 3).** 1,712,304
   `(hero, villain)` pairs ≈ 0.5 µs at steady-state GPU rate. Pre-populates
   the `vr::preflop_key` key space offline so even first-sight pairs are
   lookups. Small, cheap, immediately useful.
4. **Experimental only: seed-partitioned parallel ES-MCCFR traversals with
   fixed-order reduction.** Only job touching training math; per
   Constitution D2 must clear byte-exact CPU validation before any artifact
   it produces is trusted.
