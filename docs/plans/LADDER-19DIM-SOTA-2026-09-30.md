# The 19-dim honest router is the new ladder SOTA (2026-09-30)

## The result

`ladder --fast` on `artifacts/agent-honest-19dim` (same experts, same
robust, honest 19-dim calibrated router):

| routing | old (synthetic router) | new (honest 19-dim) | Δ |
|---|---:|---:|---:|
| `full` (argmax) | +7 136 | **+8 276** | **+1 140** |
| `full-mixture` | +4 388 | **+6 078** | **+1 690** |
| `robust-only` | +720 | +720 | — |

**Both argmax and mixture improved.** The mixture nearly matches the
previous argmax SOTA; the new argmax is clearly the best ladder number
the project has ever measured.

## Per-opponent breakdown (`--agent full`)

| opponent | synthetic | honest 19-dim | Δ |
|---|---:|---:|---:|
| arch:nit      | +1 384 | +2 461 | +1 077 |
| arch:tag      | +3 382 | +4 547 | +1 165 |
| arch:lag      | +3 932 | **+7 600** | **+3 668** |
| arch:station  | +14 259 | +14 316 | +57 |
| callbot       | +24 962 | +25 130 | +168 |
| jamfix        | +4 787 | +4 787 | 0 |
| pnash         | +4 168 | +4 585 | +417 |
| famB:tag      | +2 269 | +3 494 | +1 225 |
| noisy:0.1:lag | +5 084 | **+7 571** | **+2 487** |
| **mean**      | **+7 136** | **+8 277** | **+1 140** |

The biggest wins are against `arch:lag` (+3 668) and
`noisy:0.1:arch:lag` (+2 487) — the two opponents the honest router
is trained to identify. It also picks up solid gains on nit, tag, and
famB. It is roughly neutral on station, callbot, and jamfix.

## Why it works

The synthetic router is **degenerate** (`SYNTHETIC-ROUTER-IS-DEGENERATE-2026-09-30.md`):
it picks class 2 (LAG) on every hand, so the "argmax" mode plays only
the LAG expert. The 19-dim router actually routes:
- It picks class 0 (nit expert) on ~94% of decisions
- It picks class 3 (station expert) on the remainder
(measured in `ROUTER-19DIM-AT-INFERENCE-2026-09-30.md`).

So the new ladder measures a **dynamic** dispatch across experts. The
LAG opponent in particular sees the nit expert much of the time, which
apparently exploits it harder than the LAG expert did (the LAG expert
is too similar to the LAG opponent to punish it effectively — an
"expert as opponent" problem).

## What this changes

- **The honest-router path is now the new frontier.** Every
  `<synthetic> vs <honest>` comparison should be re-run; the honest
  version wins on both metrics.
- **The mixture architecture works.** The previous +4 388 mixture was
  crippled by the degenerate router; with a real opponent model, the
  mixture is +6 078, closer to argmax than to robust-only.
- **The mix between nit and station is not tuned.** The 94%/6% split
  is whatever the classifier learned; a principled re-blend (using
  `RouterRuntime.temp` sharpening) could be better still. Worth an
  ablation.
- **The 09-28 SOTA doc is superseded.** The shipped recommendation
  should now be `artifacts/agent-honest-19dim` with `--agent full`,
  not the synthetic-router `agent-honest`.

## The 5M expert retrain

The `retrain-tiny-5M-experts-2026-09-30.sh` pipeline was killed or
completed partially. From the file timestamps, at least the `nit`
expert completed (`artifacts/blueprints-tiny-honest-5M/nit/exploit-7/policy/policy.bin`,
453 KB, Sep 30 17:27, vs the 500k version's 368 KB). The tag/lag/station
experts were not produced.

**Follow-up:** with the parallel exploit fix (commit before this doc),
the retrain should be re-run — it will be ~2x faster than serial. The
4-expert bundle can then be ladder-compared to the 19-dim
honest-router result.

## Artifacts

- `artifacts/ladder-19dim-full.log`
- `artifacts/ladder-19dim-full-mixture.log`
- `artifacts/ladder-19dim-robust-only.log`
- `artifacts/agent-honest-19dim/` — the new SOTA bundle
- `artifacts/routers/v19-integrated/router.bin` — the honest model

## Related

- `SYNTHETIC-ROUTER-IS-DEGENERATE-2026-09-30.md` — why the old router failed
- `ROUTER-19DIM-PASSES-2026-09-30.md` — the gate pass
- `ROUTER-BET-SIZE-FEATURE-DESIGN-2026-09-30.md` — the feature
- `ROUTER-INTEGRATION-DESIGN-2026-09-30.md` — the integration
