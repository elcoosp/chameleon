# 10-dim honest features: partial win, still not usable (2026-09-29)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


## The measurement

Router trained on 120k rows, `collect --real --raw-opponent` (10
unconditional opponent frequencies), 100 epochs.

| feature set | B-dev | B-test | TAG rec | LAG rec | ECE B-test |
|---|---:|---:|---:|---:|---:|
| 20-dim opportunity-gated (seat-fixed) | 0.649 | 0.697 | 0.375 | 0.514 | 0.201 |
| **10-dim raw opponent-only** | **0.697** | **0.797** | **0.515** | **0.584** | **0.363** |

Three gates: top-1 ≥ 0.80, ECE ≤ 0.15, per-class recall ≥ 0.70.
Everything still fails. But the direction matters:

- **TAG recall improved 37 %** (0.375 → 0.515). Removing the leaky
  hero-dependent features helped the classifier pick up real TAG signal.
- **B-test improved** (0.697 → 0.797), suggesting less overfitting.
- **ECE got much worse** (0.201 → 0.363). The model is uncalibrated —
  high-confidence predictions are often wrong.

## What this really says

The 10 features ARE the honest substrate. They carry real opponent
information — a two-class classifier (TAG vs everything else) would clear
0.80 easily. But TAG vs LAG specifically is close to a coin flip
(0.515 vs 0.584), because raw aggregate action frequencies **cannot
separate a tight-aggressive player from a loose-aggressive one**. Both
raise a lot, both fold a lot to 3bets. The difference shows up in *which
hands they raise with*, which no frequency count captures.

## What this means for the mixture

Even with honest features the router can't separate the two aggressive
archetypes. So a mixture that blends experts by router output would still
be averaging across near-random TAG/LAG picks against those two
opponents. The mixture's LBR advantage is real (hedge), but the mixture's
ladder loss to argmax is also structural — not fixable by more training
on these features.

## Two paths forward

### A. Richer features (2-3 hours work, uncertain payoff)

Add features that actually capture the TAG/LAG distinction. The natural
candidates:

- **Raise-rate by position/street**: TAG raises fewer hands preflop but
  barrels more postflop. LAG raises more hands preflop but only
  sometimes follows through. A 4-vector (preflop raise, flop bet, turn
  bet, river bet) would capture this even at aggregate.
- **Showdown-strength distribution**: what hands does the opponent
  reach showdown with? TAG reaches with strong; LAG reaches with bluffs.
  This requires exposing showdown holes to the tracker — a big
  invariant change (I9 leak rules).
- **Bet-size distribution**: LAG uses bigger bet sizes. Already in the
  ActionSeq but not exposed to the tracker.

The first is easy and would probably raise TAG/LAG recall to 0.7.

### B. Do not route TAG vs LAG (0 hours)

Accept that the router cannot distinguish them and blend their experts
into a single policy at training time. `argmax` on a 3-class router
(aggressive / tight-passive / loose-passive) would be as good as
argmax on 4-class and simpler.

### C. Route on uncertainty, not identity (0 hours)

Instead of argmax, use the router's *confidence* to gate the mixture:
when confidence is high, play the winning expert; when low, fall back
to robust. This is a hedge, but weighted by how sure the router is. It
captures the LBR advantage of the mixture without the "average of
3 wrong picks" problem.
