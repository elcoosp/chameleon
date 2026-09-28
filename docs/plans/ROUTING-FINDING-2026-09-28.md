# Argmax routing dominates mixture on the current bundle (2026-09-28)

## The measurement

Same bundle (`artifacts/agent-honest`, 500k iters per expert, tiny
abstraction, γ=1.0), same pool, same ladder, three routing modes:

| opponent | mixture | robust-only | argmax |
|---|---:|---:|---:|
| arch:nit | +1 692 | −764 | **+2 536** |
| arch:tag | +2 501 | −904 | **+3 671** |
| arch:lag | +4 360 | −1 370 | **+4 042** |
| arch:station | +4 366 | +203 | **+6 691** |
| callbot | +10 157 | +7 231 | **+15 195** |
| jamfix | −449 | +4 266 | **+4 753** |
| pnash:overfold:0.15 | +353 | +2 646 | **+4 134** |
| famB:tag | +1 186 | +583 | **+2 850** |
| noisy:0.1:arch:lag | +3 310 | −667 | **+4 178** |

**argmax wins on every opponent, often by 2×.** The shipped default routing
(`full` → `mixture`) is the worst of the three measured modes.

## Why mixture loses here

The mixture blends per-hand a weighted average of the 5 tiers:

    σ_mix(a|i) ∝ Σ_k w_k · π_k(i) · σ_k(a|i)

For this to beat a single expert, the router must assign w_k high when
expert k is actually the right choice. The router is a softmax over 20
handcrafted features trained offline — and this bundle was trained with
**no `router.bin`** on disk, so the CLI fell back to
`RouterRuntime::new(SoftmaxModel::new(20, 4), 0.7, 8.0, 0.5, -1.5)` with
**randomly initialized weights** (see `cmd/hero.rs` lines 60-72). A
random router produces random weights per hand, so the mixture averages
in every expert's noise.

Meanwhile argmax takes the max-weight expert and plays it purely, which
under a random router is at least a well-defined single policy per hand
(and empirically a strong one).

## What this implies

1. **Ship argmax as the default until a trained router exists.**  The
   one-line change to `routing_for` in `cmd/hero.rs` (or to the default
   in the config) is worth ~2 bb/seating on every opponent.
2. **The router training pipeline needs to actually run.** The repo has
   `cham-router::train::train_model` and `chameleon train-router`, but no
   `router.bin` in the bundle, so the fallback path is what has been
   serving all ladder numbers to date.
3. **The router features should be re-examined.** Even with a trained
   router, if the 20 features don't distinguish the archetypes, the
   weights will be near-uniform and the mixture will still lose to
   argmax. Worth testing: train the router against the archetype pool
   and re-run all three routings.

## Action

- Immediate: rerun the ladder with `--agent argmax` (or change `full`'s
  routing to argmax) — the numbers above are already the answer.
- Follow-up: run `chameleon train-router` against a labelled dataset and
  re-measure mixture vs argmax with the trained router.
- Longer: consider whether the mixture architecture is earning its
  complexity. If a trained router + mixture still loses to argmax,
  the reach-weighted mixture is an expensive proxy for "pick one expert".
