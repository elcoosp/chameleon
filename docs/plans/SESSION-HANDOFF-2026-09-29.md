# Session handoff — 2026-09-29

Supersedes the earlier 2026-09-29 handoff of the same name. Covers both
the 09-27 to 09-29 bug-fix block and the 09-29 competitive findings.

## Background jobs at time of writing

| PID | job | ETA | output |
|---|---|---|---|
| 27970 | tiny robust 20M, CHAM_TRAIN_EPS=0.02 | ~50 min | artifacts/par-20M-eps02/ |
| 28900 | warmfix-20M LBR (1000 deals) | ~5 min | artifacts/par-20M-warmfix-lbr.log |
| 28901 | 50M LBR (1000 deals) | ~5 min | artifacts/par-50M-lbr-1k.log |

## Bug report status

62 items fixed and committed. All CRITICAL (5), HIGH (14), MEDIUM (17),
LOW (26). Workspace compiles, clippy clean with `-D warnings`, nextest
green. Every fix landed in its own commit with an anti-regression test
where the bug was testable.

## Competitive findings (this session)

### The central result: tiny peaks at 5M

| iters | seat 0 (SB) | seat 1 (BB) | mean |
|---|---:|---:|---:|
| 500k | 23 280 | 13 957 | 18 619 |
| 5M   | 15 040 | 12 050 | 13 545 (peak) |
| 20M  | 13 319 | 14 706 | 14 012 |
| 50M  | 13 886 | 17 008 | 15 447 |

SB keeps improving to ~20M. BB peaks at 5M and degrades monotonically
after. Mean peaks at 5M.

### The root cause: RM+ freeze

`rm_freeze` diagnostic (`/tmp/rm_freeze/`) reads a `table.snap` and
reports how concentrated the current iterate and the average strategy
are. Measured on the tiny curve:

| iters | soft (<0.5) rows | avg_near_frozen (>=0.9) | mean cur max_p | mean avg max_p |
|---|---:|---:|---:|---:|
| 500k | 13.5% | 4.1% | 0.762 | 0.453 |
| 20M  | 3.3%  | 60.0% | 0.871 | 0.859 |
| 50M  | 2.3%  | 66.0% | 0.888 | 0.879 |

Regret-matching+ floors regrets at zero. Once the positive part
concentrates on one action, the current iterate never re-explores. The
Linear CFR+ averaging then faithfully reproduces the frozen iterate,
so the average strategy collapses from genuinely mixed (0.45) to
nearly one-hot (0.86).

This is why SB improves (its equilibrium is near-pure, so sharpening
helps) while BB regresses (its equilibrium needs mixing, and the frozen
policy abandons it). See `RM-PLUS-FREEZE-2026-09-29.md`.

### The fix that's being tested right now

A process-wide exploration floor: `RegretTable::sigma_rms` now uses
`sigma_rms_eps(off, w, train_explore_eps())`. The floor keeps at least
`eps/w` mass on every action so no regret channel can permanently
bottom out. Set at trainer startup from `CHAM_TRAIN_EPS` (default 0.0,
bit-identical to before). See commits e16451e, 82a9bdb, cab55fc.

Smoke test at 200k iters with eps=0.02 vs the no-eps table:

| | avg_near_frozen | mean avg max_p |
|---|---:|---:|
| 200k, eps=0    | baseline | ~0.55 |
| 200k, eps=0.02 | 24.1%    | 0.682 |

The floor is active. The real experiment — does the BB regression
disappear at 20M with eps=0.02? — is running now (PID 27970).

## Other competitive findings this session

- gamma-underflow (2026-09-28): `avg_gamma=0.9` decayed to 0 within
  ~700 iters of a 500k run, wasting 40% of the average signal. Default
  changed to 1.0. See `AVG-GAMMA-FINDING-2026-09-28.md`.

- argmax > mixture on the ladder: routing config change worth ~+2
  bb/seating. `full` now maps to argmax; `full-mixture` preserves the
  old path. See `ROUTING-FINDING-2026-09-28.md`.

- mixture > argmax on LBR: but only by ~35% on the robust-only
  measurement; the mixture also depends on a router, and the router
  doesn't work. See `MIXTURE-VS-ARGMAX-TRADEOFF-2026-09-28.md`.

- router fails on real data: TAG recall 0.45 with the shipped 20-dim
  feature vector. NIT and Station classify fine; TAG and LAG are a coin
  flip. See `ROUTER-FAILS-ON-REAL-DATA-2026-09-29.md` and follow-ups.

- router features are (opponent, hero-policy)-dependent: 9 of 20
  features are opportunity-gated on hero's own actions. `trend_z` is a
  session-level leak. See `ROUTER-FEATURE-LEAK-2026-09-29.md`.

- 10-dim honest feature set still can't separate TAG from LAG: top1
  0.697, TAG recall 0.515. See `ROUTER-RAW-FEATURES-RESULT-2026-09-29.md`.

- instrument seat asymmetry: `collect --real` always ran hero at SB,
  biasing the tracker and killing 9 of 20 features. Fixed (alternating
  seat). See `INSTRUMENT-SEAT-ASYMMETRY-2026-09-29.md`.

- full abstraction is sample-starved, not worse: at matched
  visits/infoset (tiny 500k vs full 9M, both 24), full beats tiny by
  22% on SB and ties on BB. But full at 9M is still worse than tiny at
  5M on wall-clock terms. See `FULL-9M-RESULT-2026-09-29.md`.

- parallel trainer: Hogwild worker pool with sliced warmup. 2.4x wall
  speedup at 4 workers, LBR within +-6% of serial. See
  `PARALLEL-TRAINER-RESULT-2026-09-29.md`.

## Current SOTA

    bundle:      artifacts/agent-honest  (tiny, 500k/expert)
    routing:     full (= argmax)
    averaging:   gamma = 1.0
    ladder:      +7 146 mb/seating mean, wins 9/9
    LBR:         tiny 5M robust: 15 040 / 12 050

Not using: full-abstraction bundle, mixture routing, hedged routing.
All three measured worse than tiny-5M-argmax.

## What to do when the three background jobs finish

1. Read `artifacts/par-20M-eps02-lbr.log`. Compare seat 1 against the
   no-eps 20M value (14 706). If it drops to ~12 000, the freeze
   hypothesis is confirmed and the exploration floor should ship as a
   default (eps ~ 0.01 to 0.02).

2. Read `artifacts/par-20M-warmfix-lbr.log`. This is 20M with the
   insert-only warmup fix (commit 1fa3762), no eps. If seat 1 improves
   vs the pre-fix 20M, both fixes are complementary.

3. Read `artifacts/par-50M-lbr-1k.log`. 1000-deal re-measure of the 50M
   table (was 200-deal: 13 886 / 17 008).

4. If the freeze fix works, run 20M and 50M with eps=0.02 and check
   whether the peak moves past 5M. If yes, the tiny abstraction is no
   longer at its ceiling.

5. If the freeze fix doesn't work, the ceiling is elsewhere — probably
   the averaging schedule (T/4 delay) or the CFR+ update itself.

## Longer-term priorities

1. Make the parallel trainer the default (opt-in for now via
   `--thread-mode hogwild --threads N`).

2. Router feature engineering — the mixture cannot ship until the
   router can separate TAG from LAG. That requires features that
   capture which hands an opponent raises with, not just how often.

3. Full-abstraction experiment at 90M iters (~30 h at 4 workers). Only
   worth running after the freeze fix, so the long run doesn't simply
   freeze harder than tiny does.
