# Session handoff — 2026-09-29

## What is running right now (background)

| PID | job | ETA | output |
|---|---|---|---|
| 57877 | tiny Robust, 5M iters, 4 workers, hogwild | ~25 min | artifacts/par-5M/ |
| 57880 | full Robust, 9M iters, 4 workers, hogwild | ~16 h | artifacts/blueprints-full-parallel/ |

Both `nice`'d so they do not starve the interactive shell.

## What was accomplished (2026-09-27 → 2026-09-29)

### Bug report: 62 items fixed and committed

Every CRITICAL (5), HIGH (14), MEDIUM (17), and LOW (26) finding from
docs/plans/chameleon-bug-report.md landed with a targeted commit.
Workspace compiles, clippy clean with -D warnings, nextest green.

### Competitive findings

1. **gamma-underflow** — avg_gamma=0.9 decayed to 0 within 700 iters of
   a 500k run, wasting 40% of the strategy-average signal. Fixed to 1.0.
   See AVG-GAMMA-FINDING-2026-09-28.md.

2. **Argmax > mixture on the ladder** — routing config change worth +2
   bb/seating on every opponent. `full` now maps to argmax.
   See ROUTING-FINDING-2026-09-28.md.

3. **Mixture > argmax on the LBR** — hedge against router error trades
   EV for worst-case robustness. Both configs kept; CLI picks per context.
   See MIXTURE-VS-ARGMAX-TRADEOFF-2026-09-28.md.

4. **Full abstraction is sample-starved at 500k iters** — 1.3 visits per
   infoset vs tiny's 24. See FULL-VS-TINY-2026-09-28.md.

5. **Tiny abstraction is at its ceiling** — 50M iters buys 14% over 500k
   on aggregate, with seat-1 regressing. See 50M-CONVERGENCE-2026-09-28.md.

6. **Router fails on real data** — TAG recall 0.45 with the current
   20-dim feature vector. `collect`'s synthetic path is a stub whose gate
   is vacuous. See ROUTER-FAILS-ON-REAL-DATA-2026-09-29.md.

### Infrastructure

- **Parallel Robust trainer** — 2.4x wall speedup at 4 workers, LBR
  within ±6% of serial. See PARALLEL-TRAINER-RESULT-2026-09-29.md.
- **ChameleonAgent::action_distribution** — mixture LBR now measurable.
- **ChameleonAgent::tracker_features** — real-data router collection.
- **Router filename bug fix** — `train-router` wrote `model.bin` while
  every agent path read `router.bin`. Never detected before today.

## Current SOTA config

    bundle:      artifacts/agent-honest  (tiny abstraction, 500k/expert)
    averaging:   gamma = 1.0  (Linear CFR+)
    routing:     full = argmax, full-mixture = mixture
    ladder agg:  +7 146 mb/seating mean, wins 9/9
    LBR:         23 bb/hand seat 0 (robust-alone)

## What the next session should do

1. **Wait for the full-abstraction parallel run (PID 57880).** Compare its
   LBR to the tiny bundle's. If full at matched visits/infoset beats tiny,
   the abstraction is not the ceiling and the roadmap changes.

2. **If full does NOT beat tiny:** the trainer is the bottleneck. Options:
   (a) better router features, (b) Discounted CFR, (c) predictive CFR+.

3. **Do not re-run the synthetic router pipeline.** It trains against a
   stub that encodes the label. The real producer (instrument) exists and
   produces honest data whose gate FAILS at TAG recall 0.45.

## Operational notes

- `CHAM_AGENT_BUNDLE=<dir>` overrides the default artifacts/agent.
- `CHAM_ROUTER_TEMP` / `CHAM_ROUTER_N0` override router hyperparameters.
- `CHAM_AVG_DELAY=<N>` sets the averaging delay in iterations.
- `CHAM_IGNORE_ABSTRACTION_HASH=1` skips the hash guard (diagnostic only).
- `CHAM_FORCE_SEAT`, `CHAM_EXPLORE_EPS`, `CHAM_RBP_THETA0`,
  `CHAM_AVG_UNIFORM`, `CHAM_FALLBACK_MODE` exist for ablation.

## Known open issues

- **Router feature set is the mixture's blocker.** No fix attempted.
- **Parallel trainer coverage gap**: 26% fewer rows; not
  quality-relevant at 100k+ iters, but a Mutex-guarded insert would
  close it cleanly (~50 lines, not urgent).
- **`collect` still uses the synthetic stub** by default. Real producer
  exists as `instrument` but is not wired into the CLI.
- **Full-abstraction bundle has the 17-vs-65 river-edge mismatch**
  (L-2 warning). Needs a `train-buckets` rebuild before any
  full-abstraction claims are final.
