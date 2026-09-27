> **⚠ PARTIALLY STALE (2026-09-27, commit fe84467)**
> The per-expert fallback breakdown this report describes was
> measured on the PRE-RBP-fix collapsed policy. The two mechanisms
> (Cause A training-reachability gap, Cause B fallback-order
> over-report) are still the right explanation of WHY fallbacks
> happen, but every NUMERIC rate in the tables below is now
> stale — the honest-trained policy has never been measured with
> this diagnostic. See `docs/reports/20260927-rbp-gate-stale-results.md`.
> The `probe --diag-fallback` instrumentation itself is unaffected.

# P1 fallback diagnosis — per-expert miss attribution

> **Status:** [DONE] — 2026-09-26 — measurement only, no runtime change shipped.

Commands (one per bundle):

    DIAG_DEALS=60 cargo run -q -p cham-cli -- probe --diag-fallback --bundle artifacts/agent      --agent full
    DIAG_DEALS=25 cargo run -q -p cham-cli -- probe --diag-fallback --bundle artifacts/agent-full --agent full

Instrumentation added (uncommitted at time of writing):
`DecisionTrace` gained `expert_missed: [bool; 4]`, `robust_missed: bool`,
`reach_mass_zero: bool`, `mix_zero: bool`; `probe` gained
`--diag-fallback` and `--bundle`. All changes are additive; the decision
path is unchanged.

## TL;DR

The 26.7 % full-abstraction fallback (and a previously un-reported 17.3 %
tiny-abstraction fallback) is **not a router-weighting problem**. The
mixture machinery is behaving correctly: `mix_zero = 0` and
`reach_mass_zero = 0` on both bundles, so the reach-weighted mixture never
collapses. Every fallback decision measured is one of two kinds:

1. **Cause A — training-reachability gap.** For `jamfix` and
   `pnash:overfold:0.15`, all four experts miss the SAME infosets, and
   robust misses on most of them too. Those trajectories are outside the
   trained support of every artifact in the bundle.
2. **Cause B — fallback-order over-report.** For `callbot` and
   `arch:station`, one or more experts CAN cover the infoset, but **robust
   misses it**. The `fallback_used` bit is set by the robust tier's miss
   even though the mixture still contains real expert mass.

A third issue: on `mode = robust-only`, `fallback_used` is reported from
the mixture path, which that routing mode computes and then throws away.
Ladder-style fallback counters therefore over-report on `robust-only`.

## Measurements

Tiny bundle, `artifacts/agent`, 60 deals/opponent (1000 decisions):

| opponent | decis | fb_used | e0_miss | e1_miss | e2_miss | e3_miss | r_miss |
|---|---:|---:|---:|---:|---:|---:|---:|
| arch:nit | 96 | 7 | 2 | 2 | 2 | 2 | 7 |
| arch:tag | 89 | 3 | 1 | 0 | 0 | 0 | 3 |
| arch:lag | 105 | 4 | 0 | 1 | 0 | 0 | 4 |
| arch:station | 127 | 18 | 1 | 3 | 5 | 0 | 18 |
| callbot | 160 | 29 | 3 | 1 | 2 | 0 | 29 |
| jamfix | 110 | 50 | 50 | 50 | 50 | 50 | 13 |
| pnash:overfold:0.15 | 104 | 36 | 27 | 31 | 29 | 30 | 16 |
| famB:tag | 114 | 17 | 2 | 6 | 12 | 10 | 17 |
| noisy:0.1:arch:lag | 95 | 9 | 4 | 4 | 4 | 4 | 6 |
| **TOTAL** | **1000** | **173** | 90 | 98 | 104 | 96 | 113 |

`fallback rate: 17.3 %  mix_zero: 0  reach_mass_zero: 0`

Full bundle, `artifacts/agent-full`, 25 deals/opponent (436 decisions):

| opponent | decis | fb_used | e0_miss | e1_miss | e2_miss | e3_miss | r_miss |
|---|---:|---:|---:|---:|---:|---:|---:|
| arch:nit | 40 | 6 | 1 | 0 | 3 | 3 | 5 |
| arch:tag | 40 | 5 | 0 | 1 | 2 | 1 | 4 |
| arch:lag | 40 | 6 | 4 | 4 | 1 | 4 | 5 |
| arch:station | 54 | 9 | 0 | 1 | 0 | 0 | 8 |
| callbot | 79 | 25 | 1 | 0 | 0 | 0 | 25 |
| jamfix | 46 | 21 | 21 | 21 | 21 | 21 | 7 |
| pnash:overfold:0.15 | 44 | 13 | 9 | 9 | 10 | 13 | 2 |
| famB:tag | 49 | 7 | 5 | 6 | 7 | 6 | 5 |
| noisy:0.1:arch:lag | 44 | 7 | 6 | 6 | 3 | 6 | 5 |
| **TOTAL** | **436** | **99** | 47 | 48 | 47 | 54 | 66 |

`fallback rate: 22.7 %  mix_zero: 0  reach_mass_zero: 0`

`robust-only` on tiny, 30 deals/opponent (373 decisions):

`fallback rate: 13.1 %  mix_zero: 0  reach_mass_zero: 23`

## Cause A — training-reachability gap

`jamfix` and `pnash:overfold:0.15` are the clear witnesses. On every
fallback decision in those matches, **all four experts miss the same
infoset** (columns e0..e3 are identical). Robust occasionally covers the
spot (13/50 for `jamfix`, 16/36 for `pnash`), which is the only reason the
fallback rate is not higher.

Interpretation: the four Exploit blueprints (`nit`, `tag`, `lag`,
`station`) and the robust self-play blueprint were trained against
trajectories their opponents never produced. `jamfix` and `pnash` open
with bet sequences the training set does not contain, so the encoder's
keys never enter the trained support. Adding more router weight cannot
recover this class.

This class is stable across both abstractions: `jamfix` misses 100 % of
experts at both tiny and full, so it is not a bucket-resolution issue.

## Cause B — fallback-order over-report

`callbot` is the clearest witness on both bundles: `expert_missed` is
nearly zero (tiny: 3, 1, 2, 0; full: 1, 0, 0, 0) yet `fallback_used`
matches `robust_missed` exactly (29/29, 25/25). `arch:station` shows the
same pattern (18 = 18, 9 = 8). The mixture in these decisions **contains
a real expert strategy**; the robust contribution is uniform, which sets
`fallback_used = true` for the whole decision.

The pipeline as it stands has the semantics "if the robust tier contributes
a uniform probability vector, the decision counts as a fallback". That is
strictly over-reporting versus the handoff's `< 5 %` target: it counts a
recoverable miss as a full fallback and inflates the reported number.

## `robust-only` reporting

On `robust-only`, the mixture is computed and then discarded — the routing
match arm calls `robust.strategy(...)` directly. The trace nonetheless
records the mixture path's `fallback_used` and `reach_mass_zero`. That is
how `robust-only` on tiny reports `fallback rate 13.1 %` with
`reach_mass_zero = 23` even though no decision fell back at the action
level. The ladder counter is therefore wrong on this mode.

## What this means for the roadmap

- `docs/plans/v3-execution-roadmap.md §1.1` (parallel `AbRunner`) and §1.2
  (LRU + warm cache) are correct but **orthogonal**: they speed up A/B
  iteration, they do not move fallback.
- §4.1 (EMD bucket rebuild) improves within-reach quality; it does not
  widen reach, so it cannot close Cause A.
- The `< 5 %` fallback acceptance from the P1 handoff requires either
  widening the training opponent distribution to match
  `config/pool.toml` (Cause A) or re-defining what counts as a fallback
  at the mixture level (Cause B).

## What would close the gap

R1. **Widen the training opponent distribution** to cover the pool
    opponents (`jamfix`, `pnash:overfold:0.15`, `callbot`). Cause A
    disappears only when the trained support includes the pool's
    trajectories. This is the substantial piece of work; it belongs to
    roadmap §5.1 (B1 opponent grid).

R2. **Skip, don't substitute, missed tiers in the mixture.** Today, a
    missed expert replaces its σ with robust's σ; a missed robust replaces
    its σ with uniform. At the mixture level, drop the missed tier's
    contribution entirely and renormalize the remaining weights, then set
    `fallback_used` only if the resulting mixture reduces to uniform
    (i.e. when `mix_zero`). This removes the Cause B over-report without
    weakening the "not covered by any trained policy" signal, which is
    preserved by `expert_missed` and `robust_missed` on the trace.

R3. **Report `fallback_used` from the decision path, not from the
    mixture path.** For `robust-only`, `argmax`, and `bayes` the mixture
    is not the decision, so the mixture's `fallback_used` bit is
    irrelevant. Move the trace field to the routing match arm.

These are separate; R3 is a two-line change and is the cheapest.

## Caveats

- The tiny bundle is reported here at 17.3 % (1000 decisions). The v2
  handoff quoted 0 % for the tiny agent; the two are inconsistent. Either
  the tiny artifacts were regenerated since, or the earlier 0 % was
  measured against a different pool or mode. Worth confirming before
  accepting 0 % as a target for either bundle.
- The above numbers are per-opponent samples at 25-60 deals; per-row rates
  carry wide uncertainty. The pattern (Cause A on `jamfix`/`pnash`,
  Cause B on `callbot`/`arch:station`) is stable across both bundles and
  both sample sizes, so the class attribution is solid even at these
  sample sizes.

## Reproduction

Both bundles measured with the instrumentation on disk (uncommitted at
time of writing); see the file headers in
`crates/cham-agent/src/trace.rs` and `crates/cham-cli/src/cmd/probe.rs`
for the four new trace fields and the two new CLI flags. No environment
overrides are needed beyond `DIAG_DEALS` (deals/opponent, default 40) and
`DIAG_POOL` (pool TOML path, default `config/pool.toml`).
