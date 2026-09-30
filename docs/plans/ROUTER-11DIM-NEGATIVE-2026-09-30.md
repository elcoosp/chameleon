# 11-dim honest router: NEGATIVE — the tilt scalar does not separate TAG from LAG (2026-09-30)

## The result

Ran `collect --real --raw-opponent-11` (120 000 rows, 4 opponents × 60
sessions × 500 hands) and `train-router --rows artifacts/router_raw_11.rbin`.

| metric | 10-dim baseline | 11-dim with tilt | direction |
|---|---:|---:|---|
| top-1 B-test | **0.797** | 0.766 | worse |
| TAG recall   | 0.515 | **0.517** | flat |
| LAG recall   | **0.584** | 0.503 | worse |
| ECE B-test   | 0.363 | **0.305** | better |
| gate status  | FAIL | FAIL | FAIL |

Gate targets: top-1 ≥ 0.80, per-class recall ≥ 0.70, ECE ≤ 0.15.

**The tilt feature did not help.** On top-1 and LAG recall the 11-dim
model is *worse* than the 10-dim one. ECE improved somewhat (0.363 →
0.305) but is still 2x above the gate.

## Why it didn't work

The design (`ROUTER-TILT-FEATURE-DESIGN-2026-09-30.md`) predicted:

    tilt = mean(flop_bet_freq, turn_bet_freq, river_bet_freq) − preflop_raise_freq
    TAG tilt ≈ +0.37, LAG tilt ≈ +0.14

That prediction came from the archetype parameters *in isolation*
(cbet_flop 0.52 vs 0.42, etc.). Those parameters describe the
opponent's policy conditional on reaching each decision. The
**observed** tilt is a different quantity:

1. **Hole-card selection bias.** The opponent only bets the flop when
   its EHS proxy (from the real hole cards) is above the cbet
   threshold. The preflop raise freq is affected by the cutoff chart.
   Both are conditioned on the actual hole cards the opponent was
   dealt, which are not the same distribution for a `nit` vs a `lag`
   in a given session — the opponents get different cards.

2. **Hero-policy coupling.** The instrumented hero runs `argmax`, so
   the sample of hands the opponent reaches the flop on is the *same*
   for TAG and LAG — those the hero's argmax policy decided to play
   with. This is the exact (opponent, hero-policy) coupling we were
   trying to remove; my tilt scalar still contains it because its
   denominator (`reached_flop_count`) is determined by hero choices.

3. **Postflop denominators differ.** `flop_bet_freq = flop_bets / flops`
   and `preflop_raise_freq = preflop_raises / hands`. If a TAG raises
   preflop, the hero *folds less* and they see more flops, diluting
   the flop_bet_freq. If LAG raises more, more hands end preflop. The
   ratio isn't a clean conditional probability.

4. **Small effect size.** From the raw 10-dim numbers, the mean of
   flop/turn/river bet frequencies across all TAG and LAG sessions is
   itself noisy. The design's predicted spread of +0.23 may be within
   the actual variance for real hands.

## What this rules out

- **Tilt scalar alone is not the discriminator.** Whatever separates
  TAG from LAG in real play, it isn't the aggregate postflop-vs-preflop
  ratio.

- **The raw-frequency feature engineering path is diminishing.** Going
  from 20-dim → 10-dim helped (TAG recall 0.375 → 0.515). Going from
  10-dim → 11-dim with the tilt did not (0.515 → 0.517). The
  low-hanging fruit is exhausted.

## What's actually needed (recommendation for a future session)

The signal is in **which hands the opponent raises with**, not how
often. The two paths that capture this:

1. **Showdown-strength distribution.** When the opponent reaches
   showdown, what is the distribution of hole-card equities? TAG
   reaches showdown with strong holdings; LAG with a wider mix. This
   requires exposing showdown holes to the tracker (I9 leak rules —
   a non-trivial change but tractable).

2. **Bet-size distribution.** TAG sizes 66% pot; LAG sizes 100%.
   The bet size is already in the ActionSeq; the tracker just doesn't
   aggregate it. A histogram of `Raise{to} − current_bet` would give
   a 4-8 bin feature that captures this directly.

Both are more invasive than adding one scalar. Either is likely
necessary. This session did not attempt them.

## The 20-dim router is still the shipping choice

The shipped `agent-honest` bundle uses the **synthetic-trained
20-dim router**, not the honest 10/11-dim. The synthetic gate is
vacuous (feature dimension contains the label). The honest gate fails.
There is no shippable router today; the mixture's +4 388 vs argmax's
+7 136 gap remains.

## Artifacts

- `artifacts/router_raw_11.rbin` — 120 000 rows × 11 features
- `artifacts/collect-raw-11.log`
- `artifacts/train-router-raw-11.log`
- `artifacts/routers/v11/` — trained model (FAILS the gate)

## Related

- `ROUTER-TILT-FEATURE-DESIGN-2026-09-30.md` — the design this tested
- `ROUTER-RAW-FEATURES-RESULT-2026-09-29.md` — the 10-dim baseline
- `ROUTER-11DIM-EXPERIMENT-PLAN-2026-09-30.md` — the plan
- `ROUTER-FAILS-ON-REAL-DATA-2026-09-29.md` — the original failure
