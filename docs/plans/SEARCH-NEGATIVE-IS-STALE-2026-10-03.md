# The search negative result is STALE — the villain-range fix landed 8 min later (2026-10-03)

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
