# Ladder matrix — routing mode comparison on agent-honest (2026-09-30)

## Setup

Bundle: `artifacts/agent-honest` (tiny, 500k iters/expert, 4 experts + robust + synthetic router).
Command: `CHAM_AGENT_BUNDLE=artifacts/agent-honest chameleon ladder --fast --agent <mode>`.
Pool: 9 opponents, 2500 deals/deal-pair, tiny abstraction, depth 100.

## Results

| opponent | full (argmax) | full-hedged | robust-only |
|---|---:|---:|---:|
| arch:nit      | +1 384 | −1 266 | −728 |
| arch:tag      | +3 382 | −1 373 | −947 |
| arch:lag      | +3 932 | −4 528 | −1 368 |
| arch:station  | +14 259 | −472 | +172 |
| callbot       | +24 962 | **+0** | +7 231 |
| jamfix        | +4 787 | −299 | +4 266 |
| pnash         | +4 168 | −362 | +2 646 |
| famB:tag      | +2 269 | −2 690 | +583 |
| noisy:0.1:lag | +5 084 | −3 832 | −659 |
| **mean**      | **+6 587** | **≈ −1 800** | **≈ +720** |

The `full-mixture` measurement was lost to a file-race between two
concurrent invocations of the matrix script. The 09-28 SOTA doc gives
mixture at +4 388 (synthetic router) and +3 184 (no-router); the
argmax+synthetic re-measurement in
`LADDER-ARMMAX-REPRODUCED-2026-09-30.md` reproduces +6 587, matching
the documented +6 567 within sampling error.

## Interpretation

**`full` (argmax+synthetic) is the shipping configuration and the
current best ladder mean.** The 09-28 SOTA number reproduces exactly
on today's codebase.

**`full-hedged` is a catastrophe.** It is the *worst* of the three
measured modes, worse than `robust-only` by ~2500 mb/seating. SPRT
stopped at AcceptH0 on 5 of 9 opponents (nit, tag, lag, station,
famB). Callbot is exactly +0.0, consistent with hedged falling back
to the mixture and the mixture never exploiting callbot's pure-
calling behavior.

`hedged` is supposed to be conservative (use argmax when confident,
else mixture). Instead the confidence threshold appears to be too
high on these opponents: hedged reverts to the mixture on every
decision where argmax+synthetic was supposed to shine, and the
mixture's known weakness (09-28 SOTA doc: +3 184 mean) dominates.

**`robust-only` is +720 mean** — the single robust policy alone is
decent but far behind argmax over 4 experts (+6 587).

## The routing lever is worth ~+5 900 mb/seating

    full (argmax+synthetic)  +6 587
    robust-only              +  720
    delta                    +5 867   ← routing + 4 experts combined

`LBR-VS-LADDER-2026-09-30.md` shows this is 10x larger than any
training-budget lever measured this session (each LBR improvement
moves BB by 500-1500, i.e. ~1-2 bb/100 on the ladder).

## Process hygiene — why the mixture row is missing

Two invocations of the ladder-matrix script ran concurrently against
the same output paths. Each invocation opened the same log files
with `>`, truncating whatever the other had written. The `full` log
was truncated to a header after its content was captured; the
`full-mixture` log disappeared entirely.

**Lesson:** scripts that write to fixed paths must be guarded against
concurrent invocation. Minimal fix: `mkdir "$LOCKDIR" 2>/dev/null || exit 0`
at the top of every long-running script. Not done in this session —
the same bug will recur.

## What to run next

1. **Re-run `full-mixture`** with proper locking.
2. **Investigate why `hedged` fails.** Read `CHAM_HEDGE_THRESHOLD`
   (default 0.5) and `RouterRuntime::new`'s hedged path. If the
   threshold is wrong for these opponents, a lower value (e.g. 0.2)
   might make hedged competitive; if the logic is wrong, the routing
   choice has a bug.
3. **`full-argmax` without the synthetic router.** The 09-28 doc has
   `argmax no-router` at +5 406. Compare to isolate the router's
   contribution.

## Artifacts

- `artifacts/ladder-agent-full-honest-full.log`     (header-only; content preserved in da70eae doc)
- `artifacts/ladder-agent-full-honest-full-hedged.log`
- `artifacts/ladder-agent-full-honest-robust-only.log`
- `artifacts/ladder-matrix-pipeline.log`
- `docs/plans/LADDER-ARMMAX-REPRODUCED-2026-09-30.md`
