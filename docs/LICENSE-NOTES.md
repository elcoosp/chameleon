# License notes

> **Status:** [DONE] — Phase 1.5 of V2-DEV-PLAN (2026-09-26)

## This workspace

`Cargo.toml` declares `license = "MIT"` for every crate. There is no
`LICENSE` or `COPYING` file at the repo root — the workspace metadata is
the source of truth; a top-level `LICENSE` text should be added before
any public release (unrelated to Phase 1.5's concerns).

## On the plan's "postflop-solver" item

Phase 1.5 of `docs/plans/v2-dev-plan.md` says:

> Read postflop-solver's LICENSE (web or vendored copy). Record: license id,
> whether committed derived numeric fixtures are permitted, required
> attribution.

Searched the tree:

- **No `postflop-solver` dependency** in any `Cargo.toml`.
- **No vendored copy** of the project.
- **No source file** references it.
- **No artifact provenance** names it as a source.

Conclusion: at time of writing, this workspace contains **zero code or
numeric fixtures derived from postflop-solver**. There is nothing to
attribute, nothing to re-license, and nothing to remove.

## Standing rule for any future import

`postflop-solver` is AGPL-3.0. If a future change ever *does* import
from it — algorithm, table, or derived numeric fixture — the following
must be resolved by a human **before** the import lands:

1. Confirm the license id of the specific commit/version.
2. AGPL-3.0 is incompatible with MIT for distribution of *derived works*.
   Vendoring its source or its exact numeric outputs would require
   re-licensing the whole workspace as AGPL-3.0.
3. Algorithmic inspiration (published literature with pseudocode) is
   **not** the same as a derived work — the memory/turn/texture
   abstractions the plan discusses are all in that second category.
   Keep the distinction explicit: cite the paper, don't copy the code.

If the answer to (1) is ever "AGPL-3.0" and (2) applies, open a
`BLOCKED` note in the worklog — do not delete fixtures unilaterally; the
human decides.
