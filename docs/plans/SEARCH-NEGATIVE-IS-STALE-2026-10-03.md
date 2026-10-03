# Search negative CONFIRMED (not stale): the tracker villain range did not fix it (2026-10-03)

> **RETRACTED / CORRECTED (2026-10-03, same day).** I claimed the
> search negative was "stale" because the villain-range fix landed 8 min
> after the A/B. **That was wrong.** Re-measured on the CURRENT binary
> (tracker range active), search is STILL net-negative — and worse:
>
> | opponent | OFF | ON (--search) | delta |
> |---|---:|---:|---:|
> | callbot | +24797 | +12326 | **-12 471** |
> | arch:station | +14092 | +5738 | **-8 354** |
>
> (stale doc: -11 016 / -6 731.) The tracker-derived villain range did
> NOT fix search. The negative is REAL, not stale. The lesson is the
> opposite of what this doc claimed: not "always re-measure after a
> fix", but "the heuristic villain range (approach c) is insufficient."


## The finding

`F1-SEARCH-CORRECTED-2026-10-01.md` concludes search is net-negative
(−2 749 mb/seating vs OFF) and names the cause: the villain range is an
**agnostic uniform spread**, not a model of the opponent.

But the timeline:

| time (2026-10-01) | commit | what |
|---|---|---|
| 15:51 | `1207783` | search wired into pipeline (`--search`) |
| 16:40 | `5b3a7af` | `to_call==0` guard (halved the loss) |
| **17:30** | `46b87be` | **the A/B doc: "villain range is the blocker"** |
| **17:38** | `3a1270e` | **tracker-derived villain range lands** |

`git merge-base --is-ancestor 46b87be 3a1270e` → true. **The A/B was
measured on the uniform spread; the fix landed 8 minutes after the doc
was written and was never re-measured.**

## What this means

Every statement "search is net-negative" / "search doesn't work" is
based on a build that **predates the villain-range fix**. The current
`search_bridge.rs` calls `villain_range_from_tracker` (line 211), which
derives the range from showdown-reach + river-bet frequency. Whether
that fixes search is **unmeasured**.

## Secondary bug: stale comment

`search_bridge.rs::try_solve` still documents:

> tracker, encoder, and robust are accepted for forward compatibility
> but not yet consumed — the villain range is an agnostic K-class
> uniform spread rather than tracker-derived.

This is false now (the range IS tracker-derived). The comment must be
fixed (deferred: the F6c-lite retrain is running and its final metric
step compiles the workspace — no source edits until it finishes).

## Action

1. **Re-run the search A/B on the CURRENT binary** (tracker range
   active). Until that lands, treat all search negatives as stale.
2. Fix the stale comment (post-retrain).
3. If search is net-positive-or-neutral now, it becomes the first-order
   lever again — the report's F10 (vector solver) work would build on
   it.

## Precedent

Same pattern as this session's other stale conclusions:
- "exploit training is single-threaded" (based on a mixture-run log).
- "router is degenerate" (a nit-only-pool artifact).
- "F4 has no f32 ceiling" (the increment was still f32).

Each was a *measurement* taken before a *fix*, then quoted as current.

Source: `git log --oneline -- crates/cham-agent/src/search_bridge.rs`,
`F1-SEARCH-CORRECTED-2026-10-01.md`.

## The clean re-measurement (2026-10-03, 19:33)

Locked, single-runner A/B on the current binary (tracker range active):

| opponent | OFF | ON (`--search`) | delta |
|---|---:|---:|---:|
| callbot | +24796.7 | +12325.9 | **-12470.8** |
| arch:station | +14091.7 | +5738.1 | **-8353.6** |

Both strongly negative. (The ledger/trace does not emit a
search-fired counter to stdout, but OFF != ON proves search fired and
changed decisions — for the worse.)

## The real conclusion

**Search (class-conditioned solver + tracker-derived villain range) is
net-negative on calling-heavy opponents.** The F1 villain-range upgrade
(approach (c), heuristic templates) is **insufficient**. The remaining
options per `F1-VILLAIN-RANGE-PLAN-2026-10-02.md`:

- **Approach (a): blueprint-reach villain range** — walk the public
  action sequence, reweight villain classes by the robust policy's
  action probabilities. 1-2 days. The principled version.
- **F10 vector solver** — replace the class-conditioned solver with a
  combo-level one. 1-2 weeks.

Until one of those lands, **`--search` stays OFF** (already the default).

## Lesson (corrected)

The "stale conclusion" pattern this session found repeatedly (exploit
threading, router degeneracy, F4) does NOT apply here: search's negative
result reproduces on the current binary. Not every old negative is
stale; some are just true.
