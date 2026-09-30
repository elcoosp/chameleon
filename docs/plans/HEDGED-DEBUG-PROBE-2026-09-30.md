# Hedged debug probe: top_weight is finite — the failure is elsewhere (2026-09-30)

## The hypothesis being tested

`HEDGED-SWEEP-CONFIRMS-PATH-BUG-2026-09-30.md` listed two possible
mechanisms for the hedged-routing failure:

1. `top_weight` is NaN, so `NaN >= 0.0` is false and the hedged branch
   always takes the mixture path (which would explain why the sweep
   showed identical results at every threshold).
2. The hedged argmax path differs from the argmax mode in some code
   detail.

The debug probe (`CHAM_HEDGE_DEBUG=1`) instruments the hedged branch in
`pipeline.rs::act_impl` to log `top_weight` on every decision.

## The observed values (from `probe --diag-fallback`)

Threshold 0.00 (should always take the argmax path):

    hedged: top=2 top_weight=0.268823 threshold=0.000
    hedged: top=2 top_weight=0.268823 threshold=0.000
    ...
    hedged: top=2 top_weight=0.357243 threshold=0.000
    ...
    hedged: top=2 top_weight=0.509556 threshold=0.000
    ...
    hedged: top=2 top_weight=0.654941 threshold=0.000

Threshold 0.99 (should always take the mixture path):

    hedged: top=2 top_weight=0.268823 threshold=0.990
    ...

**`top_weight` is finite and non-NaN.** Hypothesis (1) is disproven.

## What this means

At threshold 0.00, `top_weight >= 0.00` is **true on every decision**,
so the hedged branch takes the "confident" arm and plays the top
expert's argmax action. This is the exact same action the `argmax`
routing mode plays. And yet:

| configuration | ladder mean |
|---|---:|
| `--agent full` (routing = `"argmax"`) | **+7 136** |
| `--agent full-hedged` @ thr=0.00 (routing = `"hedged"`) | **−1 826** |

**Δ = 8 962 mb/seating.** Code that "looks equivalent" produces
opposite outcomes. There is a real path difference outside the branch
I instrumented.

## Where the difference must be

The instrumented code is inside `act_impl`. The outer paths diverge
before `act_impl`:

1. **`hero::build_chameleon_with_router`**: called with
   `routing = routing_for(agent)`, which maps `full` → `"argmax"` and
   `full-hedged` → `"hedged"`. Both are valid AgentModes now
   (`88b8b4a`). So this is fine.

2. **`pipeline.rs::on_hand_start`** (or wherever `weights`/`argmax_k`
   are computed): the check is
   `self.mode.routing == "argmax" || self.mode.routing == "hedged"`.
   Both modes set `argmax_k`. Fine.

3. **The R3 fallback re-decision (not shown in `act_impl` above)**:
   the argmax branch might have a *second* re-decision after the R3
   block that hedged lacks. Look at the full `act_impl` R3 policy
   (lines 555-620) and compare with the same region of the hedged
   branch (lines 611-655).

4. **`fallback_used` downstream effects**: if any code after
   `act_impl` (including `on_hand_end` or the trace recorder) branches
   on `fallback_used` and modifies agent state, the two modes would
   diverge despite identical per-decision actions.

None of these have been verified. The puzzle stands.

## Reproduction

    CHAM_HEDGE_DEBUG=1 CHAM_HEDGE_THRESHOLD=0.00 \
    CHAM_AGENT_BUNDLE="$PWD/artifacts/agent-honest" \
      target/release/chameleon probe --diag-fallback --agent full-hedged \
        --bundle "$PWD/artifacts/agent-honest"

    CHAM_AGENT_BUNDLE="$PWD/artifacts/agent-honest" \
      target/release/chameleon ladder --fast --agent full-hedged

And for the reference:

    CHAM_AGENT_BUNDLE="$PWD/artifacts/agent-honest" \
      target/release/chameleon ladder --fast --agent full

## Action items for a future session

1. Re-read the full `act_impl` block, both branches, side by side. Look
   for a per-decision re-decision (R3 policy) in the argmax branch that
   the hedged branch lacks, or vice versa.

2. Instrument `fallback_used` usage — is it read anywhere that affects
   state, or is it trace-only?

3. If nothing is found, test the wrapper: swap `--agent full-hedged`
   for `--agent argmax` in the same bundle+mode and check byte-for-byte
   whether the trace files match. This isolates whether the divergence
   is inside `act_impl` or downstream.

## The instrumentation

Commit `[hash]`: adds an env-gated `eprintln!` at the start of the
hedged branch in `pipeline.rs::act_impl`. Activation:
`CHAM_HEDGE_DEBUG=1`. The print shows `top`, `top_weight`, and
`threshold` on every decision. Zero cost when the env var is unset.

## Related

- `HEDGED-PATH-BUG-2026-09-30.md`
- `HEDGED-SWEEP-CONFIRMS-PATH-BUG-2026-09-30.md`
