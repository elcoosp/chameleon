# 19-dim router at inference: better than synthetic but still skewed (2026-09-30)

## The comparison

Ran `chameleon probe --diag-fallback --agent full-hedged` (with
`CHAM_HEDGE_DEBUG=1 CHAM_HEDGE_THRESHOLD=0.00`) on two bundles:

| bundle | router | argmax_k distribution (10 deals × 9 opponents ≈ 288 picks) |
|---|---|---|
| `agent-honest` | synthetic (20-dim, label-encoded stub) | **621 top=2** (all LAG; 20 deals) |
| `agent-honest-19dim` | honest 19-dim calibrated | **271 top=0**, **17 top=3** |

The synthetic router is fully degenerate (constant class 2). The
19-dim router *does* vary its pick — for the first time — but is
still skewed: 94% class 0 (nit), with a minority class 3.

## What this tells us

1. **The router is no longer constant.** The 19-dim feature vector
   genuinely distinguishes opponents. The synthetic stub's label
   encoding was being ignored on live features; the honest features
   carry real signal.

2. **The router is not yet balanced.** Even with top-1 0.906 on
   B-test, the runtime distribution of argmaxes is heavily biased
   toward class 0. There are two possible causes:

   - **Distribution shift.** `collect --real` plays the hero against
     each archetype for 60 sessions of 500 hands. The runtime ladder
     plays the hero against the same archetypes for a similar number
     of hands. The feature vectors should be similar. But the
     classifier may still have a decision boundary that favors class
     0 on the *specific* hand sample that the ladder uses, even
     though the aggregate top-1 is high on the training sample.

   - **Class imbalance in the training data.** `collect` produced
     30,000 rows per archetype (nit, tag, lag, station) — balanced.
     The classifier should not have a class prior problem. So this
     is likely distribution shift.

3. **A skewed router still moves the ladder.** Even 6% top=3 picks
   differ from the synthetic router's 100% top=2. The mixture on the
   19-dim bundle is materially different from the mixture on
   agent-honest, and the argmax ladder is materially different too.

## The ladder is running

`scripts/ladder-19dim-queued-2026-09-30.sh` measures `--agent full`,
`--agent full-mixture`, `--agent robust-only` on
`artifacts/agent-honest-19dim`. Results land in
`artifacts/ladder-19dim-*.log`.

**The comparison to watch:**

| bundle | argmax (full) | mixture | robust-only |
|---|---|---|---|
| `agent-honest` (synthetic router) | +7 136 | +4 388 | +720 |
| `agent-honest-19dim` (honest router) | pending | pending | pending |

If the mixture on the 19-dim bundle beats +4 388, the router is
worth the integration cost. If it beats +7 136, the mixture becomes
the new shipped configuration. If it does not beat +4 388, the
mixture architecture itself is suspect and the honest-router work
should be shelved even though the gate passes.

## A caveat about `top=0` vs class labels

`argmax_k = 0` maps to whichever expert is stored at
`experts/0/policy.bin`. In `agent-honest`, that's the **nit** expert
(trained via `--mode exploit --opponent arch:nit`). So "94% top=0"
means the router picks the nit expert on 94% of decisions.

That is not obviously wrong — against the archetype pool, the nit
expert might genuinely be the best default. But it means the
"mixture" on the 19-dim bundle is close to a nit-focused policy
with occasional lag swaps. Whether that beats the LAG-focused
synthetic default (+7 136) is the ladder's question.

## Related

- `SYNTHETIC-ROUTER-IS-DEGENERATE-2026-09-30.md` — the degenerate default
- `ROUTER-19DIM-PASSES-2026-09-30.md` — the gate pass
- `ROUTER-INTEGRATION-DESIGN-2026-09-30.md` — the integration plan
