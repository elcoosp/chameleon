# Router feature diagnostic (2026-09-29)

## What I measured

Ran a separability analysis on the real 120k-row instrumented dataset
(30 000 rows/class). For each of the 20 features, per-class mean and
Cohen's d for TAG-vs-LAG (the class pair the router does worst on —
recall 0.45).

## Findings

### 1. Four features are literally zero

    idx 14 opp_faces_open   mean_tag=0.0000  mean_lag=0.0000
    idx 15 opp_faces_3bet   mean_tag=0.0000  mean_lag=0.0000
    idx 16 opp_cbet_opp     mean_tag=0.0000  mean_lag=0.0000
    idx 17 opp_bets_faced   mean_tag=0.4490  mean_lag=0.4332

The three opportunity-count features (14-16) are **never populated** in the
instrumented data. Only `opp_bets_faced` (17) has values. This is 3/20
= 15 % of the feature vector that is always zero. Whatever they were
supposed to carry is missing.

### 2. Six EWM stats are pinned at exactly 0.5000

    idx  3 ewm_3bet        0.5000 / 0.5000
    idx  4 ewm_fold_to_3bet 0.5000 / 0.5000
    idx  5 ewm_call_3bet    0.5000 / 0.5000
    idx  6 ewm_cbet_flop    0.5000 / 0.5000
    idx  7 ewm_fold_to_cbet 0.5000 / 0.5000
    idx 13 ewm_limp         0.5000 / 0.5000

These are their initial values. **The tracker never updates them in this
dataset.** Combined with the zero features above, 9/20 = 45 % of the
feature vector is constant across every hand and every class.

### 3. Four features DO separate TAG from LAG

    idx 10 ewm_aggression   d = -0.928   (strong)
    idx 12 ewm_fold_vs_bet  d = +0.654   (strong)
    idx 18 trend_z          d = -0.525   (medium, but see below)
    idx  9 ewm_wtsd         d = -0.262   (weak)

So the signal that could separate TAG from LAG **exists** in the feature
set — the router just isn't using it. Two plausible reasons:

1. **45 % of the vector is dead weight.** A softmax with 20 inputs, 9 of
   which are constant, is fitting 11 real dimensions. The dead inputs
   act as a constant offset absorbed by the bias, but they also dilute
   any L2 regularization and can hide signal through sheer parameter
   count.
2. **`trend_z` is a session-level accumulator, not a hand-level signal.**
   It is the z-score of the last 200 hero net results. It correlates
   with opponent identity only because the whole session plays against
   one opponent — in a live setting where past opponents vary, this
   feature would carry no opponent information at all. It is a
   **training-only leak** and its d = -0.525 is misleading.

## The three concrete next steps, in priority order

### A. Find out why the opportunity counters are zero

`opp_faces_open`, `opp_faces_3bet`, `opp_cbet_opportunities` are
computed in `Tracker::observe_hand` from the PublicHistory action stream.
If they're zero across 120k rows, either the actions never match the
patterns or the counters are broken. This is a bug hunt, not a research
question.

The gating conditions from the H-5 fix:
    if facing_open  -> opp_faces_open += 1
    if facing_3bet  -> opp_faces_3bet += 1
    if opp_is_pfa && reached_flop -> opp_cbet_opportunities += 1

`opp_is_pfa = pfr || opp_3bet`. If neither ever fires, no flop
opportunity is recorded. `facing_open` and `facing_3bet` come from the
action walk; they should fire many times per session.

A 30-second diagnostic: re-run `collect --real` with a per-stat update
counter. If the counters show zero, the bug is in the tracker; if they
show large numbers but the features are still 0, the bug is in
`opportunity_features` (log-scaled 0-1 with cap — maybe the values are
being lost in the log).

### B. Gate the dead EWM stats on real opportunity

The six flat EWMs are the ones the H-5 fix gated behind per-hand
opportunity flags. If the flags never fire in practice against these
archetypes, the stats stay at 0.5 forever. That's a design problem: the
spec says "3bet only when facing an open", but if the shipped agent
opens so rarely that the opponent never has a 3bet opportunity, the
feature is dead in deployment too.

The fix is either:
- Widen the gating (e.g. count "facing a preflop raise" as the same
  event for both 3bet and fold-to-3bet), or
- Accept that these stats are informative only when the archetype
  actually gets a chance, and drop them (replace with features that
  don't depend on opportunity).

### C. Drop `trend_z` from the router features, or make it per-opponent

`trend_z` is a leak (session-level statistic correlates with opponent
only because each session is one opponent). It should either be removed
or replaced by something per-opponent-hand, e.g. "this opponent's
running EV against us over the last N hands against them".

## The impact of fixing A and B

Removing 9 dead inputs and adding real ones would give the softmax a
chance to learn TAG from LAG using `aggression` and `fold_vs_bet`, which
already separate them at d = -0.93 and +0.65. Even without new features,
the router might reach its 0.80 top-1 gate — and the mixture might then
beat argmax.
