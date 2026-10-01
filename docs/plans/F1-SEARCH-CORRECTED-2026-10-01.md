# F1 search after the to_call guard: still negative, but halved (2026-10-01)

## The result

Same bundle, `--agent full`, 2500 deals/pair:

| routing | mean |
|---|---:|
| `--agent full` (search OFF) | **+8 240** |
| `--agent full --search` | **+5 492** |
| Δ | **−2 749** |

Before the `to_call == 0` guard, the delta was **−5 013**. The guard
**halved the loss** but did not eliminate it.

Per-opponent:

| opponent | OFF | ON | Δ |
|---|---:|---:|---:|
| arch:nit      | +2 398 | +1 926 | −472 |
| arch:tag      | +4 841 | +3 409 | −1 433 |
| arch:lag      | +7 525 | +4 639 | −2 886 |
| arch:station  | +13 870 | +7 139 | **−6 731** |
| callbot       | +25 130 | +14 114 | **−11 016** |
| jamfix        | +4 787 | +4 787 | 0 |
| pnash         | +4 585 | +4 516 | −69 |
| famB:tag      | +3 494 | +3 372 | −121 |
| noisy:0.1:lag | +7 535 | +5 525 | −2 010 |

## The diagnostic pattern

Losses concentrate on the **calling-heavy opponents**:
`callbot` (−11 016), `arch:station` (−6 731). The other opponents
lose little or nothing.

`jamfix` is still identical to OFF — that opponent's line does not
reach the trigger (probably because it jams pre-river, so the hero
never reaches a river decision where search fires).

## The remaining cause

The villain range in `search_bridge::try_solve` is the **agnostic K-class
spread** — `DEFAULT_VILLAIN_CLASSES = 3` classes, uniform weights,
strengths spread uniformly over `[0.5 − 0.5, 0.5 + 0.5] = [0, 1]`.

Against a **calling station**, the villain's actual range is much weaker
than the uniform spread (they call with everything). The solver,
believing one third of villain's hands are stronger than the hero's,
refuses to bet the hero's value hands. The hero then checks down versus
stations, giving away the value-bet EV the search-free policy has.

This is the exact mechanism the review predicted:

> The villain range in the bridge is agnostic; real gains require
> tracker-derived ranges.

## What the fix would look like

Replace the villain K-class uniform spread with a range built from:

1. **The tracker's raw opponent frequencies** — `opp_flop_bets`,
   `opp_turn_bets`, `opp_river_bets`, `opp_showdowns`. From those,
   estimate the opponent's overall "river continuation" strength.
2. **The robust policy's reach** — for each villain class in the
   abstraction, its reach to the current river node is a real number
   the encoder can compute. That gives a genuine per-class weight.
3. **The showdown strength distribution** — what hands does this
   opponent actually show up with? Requires exposing showdown holes
   (I9 audit) or logging them separately.

Steps 1-2 are tractable (a half-day). Step 3 is the long-term fix.

## What to do now

1. **Leave `--search` off** in every shipped bundle (already the
   default). The wiring is now safe (it refuses states it cannot
   model), but it is still not *helpful* on any opponent in the pool.
2. **Implement tracker-derived villain range** as a follow-up
   (`F1-VILLAIN-RANGE-PLAN-2026-10-02.md`, TODO).
3. **Re-run the A/B after (2).** If the search then matches OFF on
   `callbot` and `station`, the search is finally at parity. Whether
   it *beats* OFF depends on the pool's unmodeled hands — probably a
   small win, not a big one.

## What the negative result still tells us

The F1 wiring is correct: the pipeline reaches the solver, the solver
runs, the trace records the decision. The measured regression is
entirely a *modelling* problem (villain range) — not a *wiring* bug.
That separation is what F1 was for. F1 stays in place; the search
lever is documented as "wired but not yet beneficial, pending a
villain-range model".

## Artifacts

- `artifacts/ladder-f1-search-off.log`
- `artifacts/ladder-f1-search-on.log`
- `artifacts/ladder-f1-ab-pipeline.log`

## Related

- `F1-SEARCH-NEGATIVE-2026-10-01.md` — the pre-guard result
- `F1-SEARCH-WIRED-2026-10-01.md` — the wiring
- `F2-COMPLETE-2026-10-01.md` — the class-conditioned solver
- `COMPETITIVE-REVIEW-2026-10-01.md` — finding F1
