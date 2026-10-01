# Full-abstraction river_eq_edges mismatch — a long-standing warning (2026-09-30)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


## What the warning says

Every `ladder` run against a full-abstraction bundle prints, once per
opponent:

    cham-engine: WARNING — river_eq_edges has 17 entries, config
    river_eq_bins=64 expects 65; running with the committed edges (a
    bucket rebuild is needed for full conformance)

The warning is emitted by `crates/cham-engine/src/tables.rs:140` when the
committed `meta.json`'s `river_eq_edges` length doesn't match the
abstraction config's `river_eq_bins + 1`.

## Verified numbers

| bundle              | abstraction config           | buckets meta edges |
|---------------------|------------------------------|-------------------:|
| `artifacts/agent-honest`        | tiny (`river_eq_bins=16`)  | 17 ✓ |
| `artifacts/agent-full-honest`   | full (`river_eq_bins=64`)  | 17 ✗ |
| `artifacts/buckets-tiny`        | tiny (16)                  | 17 ✓ |
| `artifacts/buckets-full`        | full (64)                  | 17 ✗ |

Both tiny and full `meta.json` files contain the *same* 17 edges
(0.0, …, 1.0). The full one should have 65.

## What this means

The **tiny** abstraction is consistent: `river_eq_bins=16` and 17 edges
is the correct form (16 bins → 17 boundaries). All tiny policies trained
so far (including `par-5M`, `agent-honest`'s robust + 4 experts) used
the correct bucket file. The warning does **not** fire on tiny runs.

The **full** abstraction is inconsistent: the config declares 64 bins
but the committed bucket file has 17 edges. Every full-abstraction
ladder run gets the warning, and its `river_eq_edges` are the tiny
ones. So the full abstraction's river bucketing is 16 bins, not 64 —
the abstraction is not what its config claims.

## Why not fix it now

Rebuilding `artifacts/buckets-full` with 65 edges changes the
river-equity bucket assignment for every possible river spot. That
changes the encoder's key stream. Every full-abstraction policy ever
trained (in particular `artifacts/agent-full-honest/experts/*` and
its robust policy) was trained against the *current* 17-edge bucket
file. Rebuilding invalidates all of them, forcing a full retrain of
the full-abstraction agent.

That's an overnight-scale job, not a mid-session fix. And it does not
affect the current SOTA: the shipping bundle is `agent-honest`
(tiny), whose buckets are consistent.

## What to do eventually

1. Rebuild `artifacts/buckets-full` with `river_eq_bins=64`.
2. Retrain the four full-abstraction experts and the robust policy.
3. Re-run the full-9M LBR and full-abstraction ladder.

Until (1)-(3) happen, the full-abstraction ladder numbers carry a
"wrong bucketing" caveat, and the full-vs-tiny comparison from
`FULL-9M-RESULT-2026-09-29.md` is against the tiny bucketing on the
full-abstraction's outer skeleton, not against the full config.

## Impact on this session's work

None for the shipped configuration. The warning is advisory and only
fires against `agent-full-honest`. Every measurement in this session's
`RESULTS-MATRIX-2026-09-29.md` was on tiny (consistent) buckets.

## Artifacts

- `artifacts/buckets-tiny/meta.json`  — 17 edges, matches config
- `artifacts/buckets-full/meta.json`  — 17 edges, does NOT match 64-bin config
- `crates/cham-engine/src/tables.rs:140` — the warning source
