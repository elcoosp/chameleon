# The averaging delay may coincide with the freeze onset (2026-09-29)

## The arithmetic

Linear CFR+ averaging weight at iteration `t` of a run of `T`:

    d    = T / 4
    w_t  = max(0, t - d)          (gamma = 1.0, no decay)

So the first `T/4` iterations contribute ZERO to the average. The
second `T/4` ramps linearly. The last iteration has weight `3T/4`.

| iteration range | weight range |
|---|---|
| 0 .. T/4        | 0 (excluded) |
| T/4 .. 2T/4     | 0 .. T/4     |
| 2T/4 .. 3T/4    | T/4 .. 2T/4  |
| 3T/4 .. T       | 2T/4 .. 3T/4 |

## The coincidence

The rm_freeze diagnostic shows the current iterate freezes as training
proceeds:

| iters | soft (<0.5) rows | mean cur max_p |
|---|---:|---:|
| 500k | 13.5% | 0.762 |
| 20M  | 3.3%  | 0.871 |
| 50M  | 2.3%  | 0.888 |

If the freeze onset is around iteration T/4 (say 5M of 20M), then the
averaging window **starts exactly where the freeze begins**. Every
iteration that receives nonzero weight is a frozen iteration. The
mixed early phase — the whole point of averaging — is discarded by the
delay.

This is testable: run 20M tiny robust with `CHAM_AVG_DELAY=0` (already
supported). That makes the weight `w_t = t`, so the average includes
the pre-freeze mixed phase. If the resulting average is more mixed
(mean avg max_p drops) and BB LBR improves, the delay is (part of)
the problem.

## Why this is a real candidate

The delay is not a CFR+ requirement. It is a convention from Brown &
Sandholm 2019 and it works well on small games where the iterate
converges without freezing. On a game where the iterate freezes, the
delay systematically discards the best (most mixed) part of the
trajectory.

The `CHAM_AVG_UNIFORM=1` diagnostic (which sets w_t = 1 for all t)
would include even MORE of the early phase and is the strongest version
of this test. `CHAM_AVG_DELAY=0` is the middle option.

## What to run next (after the eps experiments finish)

Two candidate runs, in priority order:

1. **20M with CHAM_AVG_DELAY=0** — no delay, linear ramp from iteration
   0. Tests whether including the mixed early phase alone fixes the
   freeze without an exploration floor.

2. **20M with CHAM_AVG_UNIFORM=1** — uniform average, includes every
   iteration equally. If this beats (1), the ramp itself is a problem.

Both are ~50 min at 4 workers. Compare both against:
- pre-fix 20M (13319 / 14706)
- warmup-fix 20M (13554 / 14090)
- eps=0.02 20M (running now)

If CHAM_AVG_DELAY=0 alone gets seat 1 near 12000 (the 5M peak), the
exploration floor is unnecessary and the fix is purely in the
averaging schedule.

## Cross-check with the reported result

The tiny 5M LBR is the current peak (15040 / 12050). If 5M is close to
where the freeze starts, its averaging window still includes a lot of
mixed iterations (D = 1.25M, so iterations 1.25M-5M are averaged, of
which the early part is still mixed). At 20M, D = 5M excludes the
comparable window entirely.

That is consistent with the observed curve: peak at 5M, regression
after. The average is most useful exactly when the freeze has not yet
discarded the mixed phase by delaying past it.
