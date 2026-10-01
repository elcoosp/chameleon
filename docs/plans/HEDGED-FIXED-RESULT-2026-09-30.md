# Corrected hedged routing: it works, and it equals argmax (2026-09-30)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


## The correction

Every "hedged is a disaster" claim in this session's docs was an
artifact of a CLI guard bug: `TRAINED_AGENTS` in
`crates/cham-cli/src/cmd/guard.rs` did not include `"full-hedged"` or
`"hedged"`, so `ladder --agent full-hedged` silently fell through to
`build_hero` → `CallBot`. The measured numbers were CallBot vs the
pool, not hedged routing. See
`HEDGED-BUG-TRAINED-AGENTS-2026-09-30.md`.

With the fix (commit `dd167df`), `ladder --fast --agent full-hedged`
now loads a real ChameleonAgent.

## The result

`CHAM_AGENT_BUNDLE=artifacts/agent-honest chameleon ladder --fast --agent full-hedged`
at default `CHAM_HEDGE_THRESHOLD` (0.5), 2500 deals/pair:

| opponent | hedged (fixed) | argmax reference | Δ |
|---|---:|---:|---:|
| arch:nit      | +1 291 | +1 384 | −93 |
| arch:tag      | +3 574 | +3 382 | +193 |
| arch:lag      | +4 095 | +3 932 | +163 |
| arch:station  | +13 764 | +14 259 | −495 |
| callbot       | +24 972 | +24 962 | +10 |
| jamfix        | +4 790 | +4 787 | +3 |
| pnash         | +4 201 | +4 168 | +33 |
| famB:tag      | +2 215 | +2 269 | −54 |
| noisy:0.1:lag | +5 059 | +5 084 | −25 |
| **mean**      | **+7 107** | **+7 136** | **−29** |

**Hedged at default threshold is statistically identical to argmax.**
The mean differs by 29 mb/seating — inside the ±200 noise band. Six
of nine opponents agree within ±200. The others agree within ±500.

## Why hedged ≈ argmax at threshold 0.5

The `CHAM_HEDGE_DEBUG=1` probe (from
`HEDGED-DEBUG-PROBE-2026-09-30.md`) showed `top_weight` evolving within
a session:

    0.269 → 0.357 → 0.425 → 0.480 → 0.510 → 0.547 → 0.580 → 0.608 ...

The weight crosses 0.5 by the ~20th decision of a fresh session and
stays above it thereafter (the Dirichlet posterior grows toward 1
under consistent votes). So hedged takes the argmax path on most
decisions. The rare pre-warmup decisions (top_weight < 0.5) take the
mixture path, but those are few enough not to move the mean.

## What the sweep should have shown (had the guard bug not existed)

The hedge-threshold sweep results at 0.00, 0.20, 0.50, 0.80, 1.00
should have been:

- **0.00** ≈ `full` (+7 136) — always argmax
- **0.20** ≈ `full` after the first ~5 decisions — nearly argmax
- **0.50** ≈ `full` after the first ~20 decisions — nearly argmax
- **0.80** ≈ a genuine mix; top_weight exceeds 0.8 only in longer
  sessions or with strongly consistent votes
- **1.00** ≈ `full-mixture` (+4 388) — never argmax

But the actual measured sweep was identical across all thresholds
because the hero was CallBot for every run. The corrected sweep has
NOT been run.

## What this means for the frontier

**Hedged is not a new SOTA and not a new failure.** It is essentially
argmax-with-a-warmup-blend — the mean matches argmax within noise. The
ship decision is unchanged: `agent-honest` with argmax routing.

The historical arguments in `MIXTURE-VS-ARGMAX-TRADEOFF-2026-09-28.md`
(LBR prefers mixture, ladder prefers argmax) are unaffected.

## Impact on session docs

The following docs should be read with the correction banner they now
carry:
- `HEDGED-ROUTING-BUG-2026-09-30.md`
- `HEDGED-SWEEP-CONFIRMS-PATH-BUG-2026-09-30.md`
- `HEDGED-PATH-BUG-2026-09-30.md`
- `LADDER-MATRIX-2026-09-30.md`

`HEDGED-DEBUG-PROBE-2026-09-30.md` correctly disproved the NaN
hypothesis. `HEDGED-BUG-TRAINED-AGENTS-2026-09-30.md` identified the
real cause. This doc records the corrected measurement.

## Artifacts

- `artifacts/ladder-full-hedged-FIXED.log`  (this result)
- `artifacts/ladder-agent-full-honest-full.log`  (argmax reference)
