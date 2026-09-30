# The synthetic router is degenerate: it always picks class 2 (LAG) (2026-09-30)

**Correction 14:52:** an attempted regression test asserted that the
default `SoftmaxModel::new(20, 4)` is degenerate on uniform-random
inputs in [0,1]^20. It is NOT — the default init produces mixed
argmaxes on random uniform inputs. The degenerate behaviour is
specific to `agent-honest`'s real feature distribution (the
opportunity-gated tracker vector), which is heavily concentrated
near 0.5 on most dimensions because of the maturity shrink, and where
the default init picks class 3 (station) on every decision. A
bundle with the synthetic router trained on the label-encoded stub
picks class 2 (LAG) on every decision.

So the finding stands but the generalisation was wrong: the router
is degenerate *on this feature distribution*, not universally.

## The observation

`CHAM_HEDGE_DEBUG=1 CHAM_HEDGE_THRESHOLD=0.00 chameleon probe --diag-fallback
--agent full-hedged` on `agent-honest`, 20 deals × 9 opponents, produced:

    621 top=2

**Every decision picks class 2 (LAG).** The router produces an argmax
that is constant across all opponents and all hands.

## What this means for the "argmax+synthetic" SOTA

The shipped `agent-honest` bundle's `router.bin` was trained via
`collect` (synthetic stub, not `--real`). The synthetic stub writes
the class id into a feature dimension (`f[sig] += 0.45`). Trained on
that data, the softmax model learns essentially "look at dim 0 and
map it to the class label". Real 20-dim feature vectors (the
opportunity-gated tracker vector) do not have that dim-0 signature.
At inference, the model's output collapses to a constant class.

Specifically, every hand produces the argmax over the 4 classes = 2.
So `--agent full` (= `argmax` routing) is **functionally
`--agent full` playing the LAG expert on every hand**, ignoring the
other 3 experts and ignoring the robust fallback.

## The implications

1. **"argmax+synthetic" is not routing.** It's a single expert's
   policy (the LAG expert), wrapped in a router that doesn't fire.
   The +7 136 mean ladder number attributed to "argmax routing" is
   actually the LAG expert's mean ladder score.

2. **Why does it dominate the mixture?** Because on nearly-pure
   opponents, a single well-chosen expert's policy beats an average
   of 4 experts (3 of which are bad picks). The mixture averages
   across all 4 experts' policies on every decision; the "argmax"
   mode plays only the LAG expert. That works well against callbot
   (LAG exploits calling) and reasonably against nit/tag/lag but
   fails against jamfix/pnash (the LAG expert misses on those).

3. **The 500k-LAG expert's ladder score (+7 136)** is the entire
   contribution of the router to the shipped configuration. The
   remaining 3 experts and the robust policy are dead weight on the
   argmax path.

4. **No real routing is happening in the shipping config.** The
   "routing" lever worth +6 400 (argmax vs robust-only) is actually
   a single expert (LAG) vs the robust policy. That's still a real
   comparison, but it doesn't validate the mixture architecture.

## What this changes

- **The "LBR-vs-ladder" decoupling is even sharper.** The "argmax"
  ladder number is one expert's score; the "mixture" ladder number
  averages 4 experts; the "robust-only" number is the robust policy
  alone. Three different populations, three different numbers.

- **The mixture path is not yet tested.** A meaningful mixture would
  require a router that produces per-opponent argmaxes. The synthetic
  router produces a constant. So `--agent full-mixture` (+4 388) is
  averaging all 4 experts with equal weight per hand, not a mixture
  that adapts.

- **The 09-28 SOTA doc's claim** that "argmax+synthetic wins the
  ladder" is technically correct but doesn't mean what it appears to
  mean. It's not that argmax over a trained router wins; it's that
  the LAG expert alone wins.

## Follow-up

1. **Test `argmax` on a bundle with a UNIFORM-weight router** to see
   whether the LAG expert is uniquely good or whether this is a
   random constant. This requires constructing an `agent-honest`
   bundle with `router.bin` replaced by a trivial uniform model.

2. **Test each expert alone.** `--agent full` with 3 experts removed
   from the bundle gives that expert's ladder score directly. Compare
   nit/tag/lag/station. If LAG is genuinely best, the SOTA can be
   simplified to a single LAG expert bundle (no router needed).

3. **Retrain the router on real data** (i.e. fix `collect --real`
   to produce a model that actually learns per-opponent weights).
   The 10/11-dim honest routers fail the gate (TAG recall 0.52,
   LAG 0.50), so this remains blocked on feature engineering.

## The pre-existing doc reference

`ROUTER-TRAINING-GAP-2026-09-28.md` documented the synthetic stub
problem and the gate is vacuous. It did not detect the degenerate
argmax. This doc records the concrete symptom.

## Artifacts

- `artifacts/ladder-agent-full-honest-full.log` (the +7 136 number
  that this doc re-interprets as "LAG expert alone")
- The probe output: `CHAM_HEDGE_DEBUG=1 chameleon probe --diag-fallback
  --agent full-hedged --bundle artifacts/agent-honest`

## Related

- `ROUTER-TRAINING-GAP-2026-09-28.md` — the synthetic stub problem
- `ROUTER-FEATURE-LEAK-2026-09-29.md` — why the honest features fail
- `ROUTER-11DIM-NEGATIVE-2026-09-30.md` — the latest attempt
- `HANDOFF-2026-09-30-AFTERNOON.md` — the routing-dominates-ladder
  summary that this finding reframes
