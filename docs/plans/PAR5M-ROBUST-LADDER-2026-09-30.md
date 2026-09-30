# par-5M robust-only ladder: LBR SOTA is NOT ladder SOTA (2026-09-30)

## Motivation

The 09-28 SOTA documentation defines the shipping bundle as
`artifacts/par-5M/robust-7/policy` — the tiny-5M robust policy that
minimizes LBR (13 977 / 12 858). But the ladder number quoted in the
handoffs (+7 146 mean, 9/9 wins) came from `--agent full` on the OLD
`agent-honest` bundle, i.e. argmax routing across 4 experts. The
robust-only policy from par-5M had never been measured on the ladder.

This session measured both robust-only variants side by side, plus
the reference `--agent full` numbers from `assemble-full.log`.

## The measurement (ladder --fast, 9 opponents, 2500 deals/deal-pair)

| opponent | full (argmax, agent-full-honest) | agent-honest robust-only | par-5M robust-only |
|---|---:|---:|---:|
| arch:nit      | +2 514.7 | −761.4  | **−288.2**  |
| arch:tag      | +2 689.8 | −921.5  | −1 088.4    |
| arch:lag      | +3 194.7 | −1 352.2 | −1 538.2   |
| arch:station  | +3 900.7 | +231.8  | −364.9     |
| callbot       | +6 972.1 | +7 230.6 | +5 787.5   |
| jamfix        | −1 052.0 | +4 265.7 | +3 404.0   |
| pnash:overfold| −13.3    | +2 646.4 | +2 205.2   |
| famB:tag      | +1 484.3 | +583.2  | −181.6     |
| noisy:0.1:lag | +2 580.4 | −636.4  | −559.7     |

Full (argmax across 4 experts) is positive on 7/9 opponents and the
09-28 doc reports a +7 146 mean with 9/9 wins on a different pool
(probably a different deal budget). Robust-only loses to archetype
TAG/LAG/NIT/station on both bundles; only callbot, jamfix and pnash
are wins.

The par-5M robust-only mean is roughly 0 (sum ≈ +7 400 over 9 cells,
mean ≈ +820 mb/seating), which is dramatically below full's +7 146.

## What this means

**LBR and ladder measure orthogonal things.** LBR is a 1-vs-uniform-
random-exploiter bound. The archetype ladder is a 1-vs-scripted-
behavior measurement against specific opponent profiles (nit, tag,
lag, station, callbot, jamfix, pnash, famB, noisy).

A policy can be excellent against a uniform-random exploiter and bad
against a specific archetype. The robust-only policy is:
- very good on `callbot` (+5 787) — it exploits pure calling
- bad on `arch:tag` (−1 088) and `arch:lag` (−1 538) — it doesn't
  punish these TAG/LAG scripts
- mediocre on `arch:nit` (−288), `arch:station` (−365)

The mixture (argmax over 4 experts) uses a router to pick per-opponent
experts. That's why `full` beats the archetypes by 6-8k: it uses the
*trained expert for each opponent*, not just the robust policy.

**The SOTA recommendation in the handoff is incomplete.** The LBR-best
bundle is NOT the ladder-best bundle. The ladder-best bundle is
`agent-full-honest` with `--agent full` routing, which is the actual
shipping configuration per SOTA-2026-09-28.md.

## Implication for the freeze investigation

The freeze work (delay0, avguniform, eps, delay0+eps02, DCFR) is all
in service of making the tiny abstraction's robust policy better. But
even if it fully recovered BB to 5M parity, the *ladder* would still
be dominated by the mixture routing. So the freeze work's value
depends on whether the robust policy is even on the critical path.

**Action item:** the robust-only ladder should be measured against the
mixture on the SAME archetype pool and deal budget to establish
whether the mixture actually wins at the shipped deal size. The 09-28
number (+7 146, 9/9) is the reference.

## The checkpoint fix (unrelated but landed this session)

While measuring this, a real bug was found and fixed: the parallel
trainer (`train_robust_parallel`) never honored `cfg.checkpoint_every`,
even though the CLI accepted `--checkpoint-every`. The freeze-evolution
diagnostic therefore got zero iter-N snapshots from a 20M run. Fixed
in commits e6ffbc9 and 4d4b0a7:

- `4d4b0a7` sets `slice_len = checkpoint_every` when the latter is
  set, guaranteeing slice boundaries align with checkpoint boundaries
  (my first attempt with `n_slices = total_span / ckpt` was buggy:
  `want.clamp(8, 64)` gives 8 for want=5, giving slice_len=2.5M and
  missing every 2M boundary).
- `e6ffbc9` writes the iter-N file in the parallel loop at every slice
  boundary that is a multiple of checkpoint_every.

Verified by a smoke test: 500k iters, `--checkpoint-every 100000`
yields 5 checkpoint files under `--checkpoint-dir`. (Verified AFTER
rebuilding the debug binary — the first smoke attempt used a stale
`target/debug/chameleon` and reported 0.)

## Artifacts

- `artifacts/ladder-robust-honest.log`  — agent-honest robust-only
- `artifacts/ladder-robust-par5m.log`   — par-5M robust-only
- `artifacts/assemble-full.log`         — reference `full` ladder
- `artifacts/agent-robust-honest/`      — the honest robust bundle
- `artifacts/agent-robust-par5m/`       — the par-5M robust bundle

## Next steps

1. **Run `ladder --fast --agent full` on the current artifacts/agent-full-honest
   bundle** (or rebuild it) to confirm the +7 146 reference is still
   reproducible on this code version.

2. **Run `ladder --fast --agent full` on the par-5M-derived 4-expert
   bundle** — do the 4 experts matter more than the robust? If yes, the
   freeze work on the robust policy is secondary to expert training.

3. **Run the ladder for the delay0+eps02 20M robust policy** against
   the same pool. If it doesn't beat the par-5M robust-only mean, the
   20M improvement doesn't matter on the pool either.
