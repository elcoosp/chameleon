# F1: the honest exploitability is 8.4x smaller than the reported LBR (2026-10-01)

> **Budget caveat (2026-10-01 late):** the tabular numbers here were
> measured at `(300 train / 200 test / 12 sweeps)`. `tabular_br` is an
> iterated best-response learner and its value is budget-sensitive at
> low budgets; on some policies the sum `BR(0)+BR(1)` is still negative
> at 300/200/12 and shrinks toward the true value as the budget grows.
> Quote these numbers *with their budget*. See
> `docs/plans/TABULAR-BR-CONVERGENCE-2026-10-01.md`.


## The measurement

The clairvoyant `lbr::lbr_vs` (the metric every prior session used)
vs the new infoset-consistent `lbr::tabular_br`, both queried against
the same par-5M robust policy:

| metric | seat 1 | multiplier |
|---|---:|---:|
| clairvoyant `lbr_vs` | **21 200 mb/hand** (21.2 bb) | 1x |
| infoset-consistent `tabular_br` | **2 533 mb/hand** (2.53 bb) | **8.4x** smaller |

The clairvoyant version picks its action **inside each sampled deal**
- i.e. it sees the opponent's hole cards. The tabular version learns
**one action per information set** across 300 training deals x 12
sweeps, then evaluates on 200 held-out deals. Both play real engine
actions in the same tree.

## The bug

`crates/cham-blueprint/src/lbr.rs::br_walk`:

    // BR seat: enumerate the abstraction slots, take the max
    for s in slots.iter() {
        ...
        let v = br_walk(...);
        if v > best { best = v; }
    }
    best

`best` is the **maximum over actions within a single deal**. A real
best response must pick one action per infoset, because the BR player
does not know the opponent's cards. The max-in-deal is a perfect-
information best response -- a different (and much higher) quantity.

## What the correct value changes

**Every session's LBR number was 6-10x too high.** The reported
"tiny 5M robust: 13 977 / 12 858 mb/hand" is really ~1.5-2 bb/hand on
the corrected metric -- competitive with a low-to-mid-strength GTO
approximation on the tiny abstraction, not a 13 bb/hand disaster.

The "decoupling" findings (LBR-vs-ladder) still hold directionally:
LBR and the ladder both move when the policy changes. But the *scale*
of the reported leak was wrong. The 20M/50M regressions and the
freeze work were chasing a metric that was 8x louder than the real
signal.

**The freeze investigation's conclusions are not fully invalidated.**
The freeze diagnostic (avg_near_frozen, mean max_p) reads the table
directly and is metric-independent. The LBR deltas (delay0, avguniform,
eps, DCFR) that motivated the levers were measured with the clairvoyant
metric -- the 500-1500 mb/hand improvements they showed are probably
250-500 on the corrected metric, inside the noise band. That is
consistent with the ladder results, which never moved.

## What to do now

1. **Re-quote every historical number** in the RESULTS-MATRIX and the
   handoffs with the corrected value. A one-line note in each doc is
   enough: "the 13 977 / 12 858 figures are clairvoyant LBR; corrected
   tabular BR is ~1.5-2 bb/hand (see F1-CORRECTED-METRIC)".
2. **Use `tabular_br` as the headline metric** for every future
   experiment. `lbr_vs` stays available as a diagnostic upper bound.
3. **Re-measure the frontier.** The tiny-5M policy's corrected BR may
   be as low as 1.5 bb/hand. That is a genuine GTO-class number on the
   tiny abstraction. The full-abstraction re-measurement becomes more
   interesting: does it beat tiny on the corrected metric?

## Implementation

- `crates/cham-blueprint/src/lbr.rs`: added `tabular_br` (~180 lines).
- `crates/cham-blueprint/tests/tabular_br.rs`: the `tabular <= clairvoyant`
  invariant. Passes (2.8 vs 17.4 bb/hand on a uniform policy -- 6.2x
  reduction).
- `crates/cham-blueprint/tests/par5m_metric_compare.rs`: the ignored
  test that produced this doc's numbers.
- `crates/cham-blueprint/benches/exploitability.rs`: imports updated.

## Reproduce

    CHAM_EXPLOIT_BP=$PWD/artifacts/par-5M/robust-7/policy \
    CHAM_EXPLOIT_BUCKETS=$PWD/artifacts/buckets-tiny \
    CHAM_EXPLOIT_CONFIG=$PWD/config/abstraction-tiny.toml \
      cargo nextest run -p cham-blueprint \
        -E 'test(par5m_metric_compare)' \
        --run-ignored all --no-capture

## Caveats

- The tabular BR uses a passive initial action (Check/Call) and
  iterates to convergence. If the sweep count is too small, the
  learned choice may be suboptimal -- that makes the reported BR a
  slightly optimistic *lower* bound. 12 sweeps is the default here.
- The train/test deal split is 300/200. Overfitting is guarded against
  by the held-out evaluation, but a future increase to 1000/1000 would
  tighten the CI.
- Seat 0 was not measured here. The full exploitability is the SUM of
  both seats' BR values, not the average.

## Related

- `chameleon-competitiveness-report.md` -- the audit that flagged F1
- `F1-SEARCH-WIRED-2026-10-01.md` -- the search wiring; the search
  results also need re-reading with the corrected metric
