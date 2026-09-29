# Prediction: DCFR alpha=0.5 will be worse than alpha=0.9 (2026-09-30)

## Rationale

DCFR's positive-regret discount halves (or otherwise shrinks) the
accumulated regret every iteration by a factor of alpha. On an
infinite-length run, the effective memory of the regret signal is

    memory_half_life = ln(0.5) / ln(alpha)

| alpha | half-life (iterations) |
|---|---:|
| 0.9  | ~6.6 |
| 0.5  | 1.0  |

At alpha=0.9, the regret sum is (roughly) the last 7 iterations of
deltas. The RM+ solve is then a fresh solve on 7 iterations of noise,
which is nearly uniform.

At alpha=0.5, the regret sum is the last iteration's delta (plus a
tiny tail). That is *pure* noise, so RM+ produces uniform play almost
exactly.

**Prediction:** alpha=0.5's 1000-deal LBR will be equal to or worse
than alpha=0.9's. If the two are within noise of each other, the
regret signal has been entirely erased at both discount rates. If
alpha=0.5 is *better*, something surprising is happening (perhaps
the 7-iteration window at alpha=0.9 is causing pathological variance
that disappears at the tighter window).

## Numbers to beat (lower is better)

| variant | SB | BB | mean |
|---|---:|---:|---:|
| tiny 20M no-fix        | 13 319 | 14 706 | 14 012 |
| tiny 20M alpha=0.9     | 35 344 | 25 834 | 30 589 |

If alpha=0.5 lands anywhere in the range [25 000, 50 000] on either
seat, the prediction holds and the DCFR lever is dead at any discount
rate <= 0.9. The lever should then be tested only at alpha >= 0.99 or
abandoned.

If alpha=0.5 lands below 20 000 on BB, the DCFR lever is more
interesting than expected and warrants a discount-shape sweep
(Brown-Sandholm also discount negative regrets, which this workspace
does not implement — an untested alternative).

## Why this doc exists

The session running the alpha=0.5 experiment wants a written prediction
to check the result against. Otherwise the result will be interpreted
post-hoc, which is worse science.
