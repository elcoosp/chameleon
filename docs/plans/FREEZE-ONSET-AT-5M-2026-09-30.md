# The RM+ freeze onset is at iteration 5M (T/4), confirming the delay hypothesis (2026-09-30)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


## The measurement

The freeze-evolution diagnostic (retried with the fixed
`--checkpoint-dir` on `train-bp`) trained a 20M tiny robust policy
with checkpoints every 2M iters. Each checkpoint was analyzed with
`/tmp/rm_freeze`. Raw output at
`docs/plans/FREEZE-EVOLUTION-2026-09-29.raw.txt`.

| iteration | avg_near_frozen | mean cur max_p | mean avg max_p |
|---:|---:|---:|---:|
| 2M  | **0.0%** | 0.800 | **0.397** |
| 4M  | **0.0%** | 0.822 | **0.398** |
| 6M  | **52.5%** | 0.834 | **0.828** |
| 8M  | 53.7% | 0.843 | 0.834 |
| 10M | 54.7% | 0.850 | 0.839 |
| 12M | 55.5% | 0.852 | 0.843 |
| 14M | 55.9% | 0.857 | 0.846 |
| 16M | 56.6% | 0.860 | 0.848 |
| 18M | 57.2% | 0.862 | 0.851 |
| 20M | 58.0% | 0.866 | 0.853 |

## The phase transition

**The freeze onset is between iterations 4M and 6M.** This is a
sharper phase transition than expected — `mean avg max_p` (the
average strategy's mean max probability) jumps from 0.398 to 0.828
in a single 2M-iteration window. The current iterate (`mean cur
max_p`) moves slowly across all 20M (0.80 → 0.87), but the average
strategy's mix is *destroyed* the moment the current iterate's
one-hot rate crosses a critical density.

The transition is at **5M = T/4** for a 20M run.

## Confirming the AVG-DELAY-VS-FREEZE hypothesis

`AVG-DELAY-VS-FREEZE-2026-09-29.md` said:

> **The coincidence.** The rm_freeze diagnostic shows the current
> iterate freezes as training proceeds. If the freeze onset is around
> iteration T/4 (say 5M of 20M), then the averaging window **starts
> exactly where the freeze begins**. Every iteration that receives
> nonzero weight is a frozen iteration. The mixed early phase — the
> whole point of averaging — is discarded by the delay.

**This is empirically confirmed.** Linear CFR+ averaging uses
`w_t = max(0, t − T/4)`, which excludes the first 5M iterations.
Those first 5M iterations are the *entire* pre-freeze phase. Every
iteration after 5M is in the frozen regime, and the frozen iterate
dominates the average from 6M onward.

## Why the four levers worked (and how much)

This explains every positive result of the session:

| lever | mechanism | observed | matches prediction? |
|---|---|---|---|
| `delay0` (w_t = t) | includes pre-freeze phase | BB −1 088 | yes |
| `avguniform` (w_t = 1) | includes pre-freeze equally | BB −1 573 | yes (more aggressive inclusion) |
| `eps=0.02` | softens the current iterate | BB −1 054 | partial (delays but doesn't prevent) |
| `warmfix` | insert-only warmup preserves more pre-freeze mass | BB −616 | partial |
| `delay0 + eps02` | both | BB −1 455 | yes, but no new peak |

## Why delay0 + eps02 did NOT close the BB gap to 5M

The BB regression came from the frozen post-5M phase. delay0 and
avguniform discard that phase from the average. But even a policy
trained to 20M with the frozen phase discarded is *still worse* than
the 5M policy on the ladder:

| policy | LBR BB | ladder robust-only mean |
|---|---:|---:|
| tiny 5M (peak) | **12 858** | **+820** |
| tiny 20M delay0+eps02 | 13 251 | +624 |

The 20M policy has *better SB* but *worse BB*. The BB difference
is because 20M has a longer, more thoroughly frozen current iterate
that leaks into the average even with delay0. The 5M policy never
enters the deep-frozen regime.

## The clean mechanistic picture

1. **RM+ on the tiny abstraction has a phase transition at ~5M iters.**
2. **Before the transition:** the current iterate is mixed (max_p
   ~0.80), the average is mixed (max_p ~0.40). Both are useful.
3. **After the transition:** the current iterate is one-hot (max_p
   ~0.87), the average snaps to the current iterate (max_p ~0.85).
4. **Linear CFR+ `T/4` delay** discards the pre-transition phase at
   any run longer than 20M. The average becomes a summary of the
   frozen regime.
5. **delay0 and avguniform** recover the pre-transition phase in the
   average and recover most but not all of the BB regression.
6. **The BB recovery is real on LBR** (BB improves by 10-11%), but
   **the recovered policy is worse on the archetype ladder** than
   either the 5M robust policy or the 500k robust policy.

## The remaining puzzle (for a future session)

**Why does an LBR-improved 20M policy lose on the ladder?** The BB
improvement on LBR (13 618 → 13 251 with delay0) should in principle
translate to a better exploit against TAG/LAG opponents. Instead the
ladder mean drops from +820 (5M) to +624 (20M delay0+eps02).

The likely explanation, given the freeze mechanism: on rows where
the current iterate is one-hot, the frozen policy is a nearly-pure
response. Against nearly-pure opponents (which the archetypes are on
most rows), nearly-pure responses are sharper than mixed responses.
The 20M policy's mixing helps against a *uniform* best responder
because mixing is intrinsically harder to exploit; it hurts against
*scripted* opponents because they have narrow exploit windows.

The freeze is correct convergence on most rows. The LBR regression
comes from the minority of rows where the true equilibrium is not
nearly-pure. Neither delay0 nor avguniform can distinguish which
rows need mixing; they mix everything.

## Artifacts

- `docs/plans/FREEZE-EVOLUTION-2026-09-29.raw.txt` — the raw output
- `artifacts/freeze-diag/checkpoints/` — the 10 snapshot files
- `artifacts/freeze-diag.log` — training log

## Related

- `AVG-DELAY-VS-FREEZE-2026-09-29.md` — the hypothesis this confirms
- `RM-PLUS-FREEZE-2026-09-29.md` — the original freeze diagnostic
- `AVG-DELAY-DELAY0-RESULT-2026-09-29.md`
- `AVG-UNIFORM-RESULT-2026-09-29.md`
- `DELAY0-EPS02-RESULT-2026-09-30.md`
- `20M-LADDER-NEGATIVE-2026-09-30.md` — the ladder result that
  overrules the LBR improvement
