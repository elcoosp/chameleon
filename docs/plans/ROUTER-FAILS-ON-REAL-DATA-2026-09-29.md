# The router fails on real data (2026-09-29)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


## The measurement

Trained on real instrumented data from running the shipped agent against
the four archetypes (120 000 rows, 60 sessions x 500 hands per opponent):

    train-router: rows=120000 epochs=100
      loss_b_dev=0.8034  top1_b_dev=0.761  top1_b_test=0.719
      ece_b_test=0.207   ece_family_c=0.158
      recall=[0.806 (nit), 0.453 (tag), 0.790 (lag), 0.951 (station)]
      gates=FAIL

Gates require: top1 >= 0.80, ECE <= 0.15, per-class recall >= 0.70.
Three of four gates fail. TAG recall is 0.453.

## What this means

The mixture never had a chance. It blends 5 experts per decision by
router weights. When the weights are near-uniform on 3 of 4 archetypes,
the mixture dilutes each specialist's edge. Argmax wins the ladder by
50% because it does not need a good router — it needs a consistently-
highest weight, and even a weak router gives that.

## Why the synthetic router looked perfect

`collect` synthesizes feature vectors where the class is encoded directly
(`f[sig] += 0.45` where `sig` = label). The 100% top-1 on synthetic data
was the model reading back the answer key. That is not an accidental bug
— it is the documented "stub" behavior of `collect`, and the gate it
produces is vacuous.

## What this says about the router's features

The 20-dim feature vector:
  [0]     maturity
  [1..14] 13 EWM stats (VPIP, PFR, 3bet, call-3bet, cbet, fold-to-cbet,
          barrel-turn, WTSD, aggression, showdown-won, fold-vs-bet, limp)
  [14..18] 4 opportunity counts (log-scaled)
  [18]    session EV trend z
  [19]    hands since showdown

TAG and LAG differ mainly in aggression frequencies at specific decision
points. The EWM tracker averages those over all streets, positions, and
stack depths — the distinguishing signal gets washed out. Station and Nit
are extreme so their EWMs are far from the mean and easy to classify.
TAG/LAG sit in the middle and are statistically similar.

Conclusion: the tracker features are the bottleneck, not the router model.
A different classifier on the same 20 features will not do materially
better. The mixture architecture needs features that actually separate
TAG from LAG — position- and street-conditional aggression frequencies
would be the natural starting point.

## What to do

1. Ship argmax. It is the config that wins the ladder and doesn't depend
   on the router being accurate. Already the default (`full` -> argmax).
2. Do not invest further in the mixture until the features improve.
3. If we want the mixture's LBR advantage (35% less exploitable), we
   need better features.

## Honesty notes

- `collect`'s synthetic gate is a lie if anyone quotes it. The real
   number is `top1_b_dev = 0.761`.
- The mixture's ladder loss to argmax is now explained.
- The mixture's LBR win is also explained: hedging across imperfect
   routing costs EV but reduces worst-case exploitation.
