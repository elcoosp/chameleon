# License notes

> **Status:** [DONE] — Phase 1.5 of V2-DEV-PLAN (2026-09-26)

## This workspace

`Cargo.toml` declares `license = "MIT"` for every crate. There is no
`LICENSE` or `COPYING` file at the repo root — the workspace metadata is
the source of truth; a top-level `LICENSE` text should be added before
any public release (unrelated to Phase 1.5's concerns).

## postflop-solver — the actual posture

`postflop-solver` is AGPL-3.0. The plan's Phase 1.5 asks to record
license id, fixture permission, and required attribution. Search of the
tree returns the following **existing** references (all deliberate, all
documenting the "never link, never vendor" rule):

| File | What it says |
|---|---|
| `deny.toml:11` | "The AGPL dev-time oracle (`postflop-solver`, SPECS/06 §5.3) is NEVER a workspace [dependency]" |
| `crates/cham-search/src/oracle.rs:5` | "The AGPL `postflop-solver` dev-time oracle (SPECS/06 §5.3) is NEVER linked or [vendored]" |
| `crates/cham-search/README.md:15` | "...in-repo LP river spots (postflop-solver is a dev-time-only AGPL [oracle])" |

**Verdict on Phase 1.5:** the workspace's policy on this dependency is
already documented at three load-bearing points and is stricter than the
plan's minimum requirement — no link, no vendor, no derived fixture. The
plan was written before this search confirmed the posture was already in
place; **the requirement is met and no changes are needed.**

## Fixture provenance

No `provenance.json` under any artifact records `postflop-solver` as a
source. `cham-search`'s river oracles are verified against an **in-repo**
enumerative LP (`crates/cham-search/src/oracle.rs`), not the AGPL
project.

## Standing rule for any future import

If a future change ever *does* want to import from postflop-solver —
algorithm, table, or derived numeric fixture — the following must be
resolved by a human **before** the import lands:

1. Confirm the license id of the specific commit/version (AGPL-3.0 as of
   last check, but confirm).
2. AGPL-3.0 is incompatible with MIT for distribution of *derived works*.
   Vendoring its source or its exact numeric outputs would require
   re-licensing the whole workspace as AGPL-3.0.
3. Algorithmic inspiration (published literature with pseudocode) is
   **not** a derived work. Keep the distinction explicit: cite the paper,
   don't copy the code or commit its outputs.

If (1) is ever "AGPL-3.0" and (2) applies, open a `BLOCKED` note in the
worklog — do not delete fixtures unilaterally; the human decides.
