# G3.0 — `chameleon verify --gpu` design

> **Status:** [DONE] — implemented in crates/cham-cli/src/cmd/verify.rs

## Purpose

One command that reads every table under `artifacts/gpu-tables/` and asserts
the three GPU-track gates that are enforceable without a fresh build:

- **P7 GPU-CONSISTENCY** — bit-exactness of the kernel against the CPU
  reference on a fixed sample. Since G2.1/G2.2 SKIP'd (no consumer), the
  consistency set is: the eval7 test's pinned 1M-hand corpus, plus N
  random `(board, hole)` per built table, verified against `ehs_reference`.
- **P8 BUILD-THROUGHPUT** — the manifest's recorded rate must be at least
  a floor relative to the *same-session* measured CPU reference. We do not
  re-measure CPU in `verify` (that's a benchmark task); instead we compare
  the manifest's `throughput_evals_per_s` against a stored benchmark
  reading from `bench-before-gpu.txt`, and enforce a floor.
- **P9 INTEGRATION** — for each shipped consumer, its fixture must remain
  deterministic and its speedup within bounds. **With G2 SKIP'd there are
  zero consumers, so P9 reports "0 consumers — informational".**

Non-macOS / feature-off → print `SKIP: metal unavailable` and exit 0. This
matches the plan's "skipped cleanly on CI" promise.

## CLI
