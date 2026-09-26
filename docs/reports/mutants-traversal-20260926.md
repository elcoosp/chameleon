# cargo-mutants — traversal.rs (Phase 1.2)

> **Status:** [DONE] — 2026-09-26

Command: `cargo mutants -p cham-blueprint --file crates/cham-blueprint/src/traversal.rs`

Result: **66 mutants tested in 19m: 40 missed, 25 caught, 1 unviable.**

A "missed" mutant is one the test suite fails to catch — its code path
is not behaviorally exercised by any test. Some are *equivalent mutants*
(the change provably can't alter output for the tested inputs); most are
real coverage gaps.

## Real coverage gaps (by subsystem)

### SnapBatchSink — 10/10 methods undetected (highest priority)

Every method on `SnapBatchSink` is a missed mutant:

- `SnapBatchSink::with_discount -> Default::default()`
- `SnapBatchSink::flush with ()`
- `SnapBatchSink::pending -> 0 / 1`
- `<impl RegretSink for SnapBatchSink>::add_regret with ()`
- `<impl RegretSink for SnapBatchSink>::add_strat with ()`
- `<impl RegretSink for SnapBatchSink>::add_weight with ()`
- `<impl RegretSink for SnapBatchSink>::add_visit with ()`

**Meaning:** the Snapbatch write path (PERF-PLAN T3) has zero direct
tests. `train_with_threads` in Snapbatch mode is exercised indirectly
through the `cham-cli` end-to-end tests, but no `cham-blueprint` test
asserts that a Snapbatch-buffered update lands the same values as
DirectSink for the same traversal. The Snapbatch-vs-Deterministic
"bit-identical for the same seeds" claim (documented in the module
comment) is **unverified by tests**.

### DirectSink — add_weight + add_visit undetected (2 methods)

`DirectSink::add_weight with ()` and `DirectSink::add_visit with ()`
survive. `add_regret` is caught (the `regret_discount < 1.0` branch),
`add_strat` presumably too, but average weights and visit counts are
apparently not asserted by any test.

### sample_action / sample_index — 5 mutants undetected

`sample_action`:
- `+= with -=`
- `+= with *=`
- `<= with >`

`sample_index`:
- `+= with -=`
- `+= with *=`
- `- with +`
- `- with /`

**Meaning:** neither helper's arithmetic correctness is verified. The
`sample_action` mutants are the most concerning (a wrong cumulative
probability step could bias sampling without changing the strategy).

### Traversal::walk_with_sink — ~20 arithmetic/branch mutants

Multiple `<= → >`, `&& → ||`, `!` deletion, `/ → *`, `<< → >>` survive.
Some are equivalent mutants at fixed test seeds (the boundary values
never land on the changed branch with the current test state). But not
all — the RBP pruning condition at line 269 has four missed mutants,
meaning the pruning test (`rbp_matches_full`) doesn't catch any of them
because it only checks the *output*, and any one mutation still produces
identical exploitability at the fixture scale.

## Actionable triage

| Priority | Gap | Effort to close |
|---|---|---|
| 1 | SnapBatchSink parity test: assert buffered == direct for one traversal | 30-60 min |
| 2 | DirectSink add_weight / add_visit assertions (extend existing test) | 15 min |
| 3 | sample_action distribution check on a fixed seed | 30 min |
| 4 | RBP pruning: assert `pruned_nodes > 0` and a specific pruned-action count at a known fixture | 30 min |
| 5 | The rest (equivalent mutants at test boundaries) | skip unless a reviewer flags one |

## What Phase 1.2 wants

The plan says: "surviving mutants list goes in the commit message."
Done — this file is the list, plus triage. The task's acceptance was
"target exists, runs, result recorded" — all three satisfied.

Closing the gaps is Phase 1.2.1 (not scheduled) or worklog-driven.
