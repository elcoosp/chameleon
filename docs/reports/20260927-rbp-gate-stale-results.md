# RBP-gate fix — which prior results are stale

> **Status:** 2026-09-27. Follows commit `fe84467`.

## What changed

`crates/cham-blueprint/src/traversal.rs`'s RBP gate was:

    if zero_regret && visits > theta_t && sigma[a] <= 0.0 { skip }

With the documented default `theta0 = 0`, `theta_t = 0` for every
iteration, so `visits > theta_t` was `visits > 0` — TRUE from the first
visit. The doc-comment claims pruning is *disabled* at `theta0 = 0`;
the code pruned unconditionally. Every action whose CFR+ regret floored
at zero was permanently frozen out, and the policy collapsed to a pure
strategy at every infoset.

Diagnosed via `crates/cham-blueprint/tests/sb_internals.rs` (train and
inspect the SB-root sigma/regret/avg). With the old gate:

    iters  visits  regrets                      sigma (rm+)
    1k     1-6     [0.00, 36.75, 0, 0]          one-hot
    10k    16      [0.00, 36.75, 0, 0]          one-hot
    100k   157     [0.00, 36.75, 0, 0]          one-hot

With the fix:

    100k   157     [426, 493, 431, 465]         mixed [0.23, 0.27, 0.24, 0.26]

## Which prior results are stale

Every LBR-family measurement taken before `fe84467` was measured on a
policy trained under the collapse regime. Do not cite or build on these
without re-running.

| artefact | where | status |
|---|---|---|
| EXP-011 alpha/gamma sweep | ledger `exp-011-dcfr-alpha-gamma-sweep` + commit `c861b08` | STALE |
| EXP-014 widened-tiny A/B | ledger `exp-014-widened-tiny` | STALE |
| EXP-014 widened-full A/B | ledger `exp-014-widened-full` | STALE |
| Overnight LBR convergence | `artifacts/overnight-lbr-*` (gitignored) | STALE |
| Any "trained robust LBR" printed in the 2026-09-26/27 session | chat only | STALE |
| EXP-015 60-cell router grid | ledger `exp-015-router-manipulation-sweep` | REGIME-DEPENDENT — probes the collapsed bot's router dynamics; informative for router behavior, not for policy strength |
| EXP-016 shadow gauntlet dry run | ledger `dryrun-item5-shadow-gate-skip` | UNAFFECTED — the gate mechanism works; the delta magnitude reflects the collapsed regime |

## Which prior results remain valid

| artefact | status |
|---|---|
| EXP-017 EMD bucket build + audit ratio | VALID — measures bucket quality, not policy strength |
| EXP-019 preflop-equity cold/warm bench | VALID — pure timing benchmark |
| EXP-012/013 fallback-telemetry changes | VALID as mechanism; magnitudes stale |
| RBP-gate fix itself | VALID — code is now correct |

## The first honest numbers

`artifacts/nopruning-diag/theta-inf-3M-s7` and `theta-inf-10M-s7`
(both in flight at 2026-09-27 ~13:15, tiny abstraction, seed 7, deals
200) are the first LBR figures produced with a correctly-trained policy.
They supersede every prior LBR figure in the ledger. A subsequent commit
will record them.

## Re-measurement plan (in order)

1. Land the 3M honest LBR — check against uniform (s0 ~ 40413, s1 ~ 35099).
2. Retrain the full tiny agent (robust + 4 experts at 500k iters) with
   the fix. Re-run `probe --diag-fallback`, `ladder --fast`, and the audit.
3. Re-run EXP-011 alpha/gamma sweep with correct training.
4. Re-run EXP-014 widened-tiny A/B with correct training.
5. Only then decide between more abstraction work (EMD/GPU) and more
   training-quality work.
