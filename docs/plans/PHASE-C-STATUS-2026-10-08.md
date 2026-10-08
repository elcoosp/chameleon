# Phase C — status 2026-10-08 (evening)

**Where the PCS trainer stands, honestly.** The walk exists and is
correct on verifiable toy trees. It is **not yet trainable at full
range**: the current implementation is ~100x too slow for the D2 wall
budget. Profiling is the next task.

## What landed (Phase C, this session)

| Component | File | Status |
|---|---|---|
| DCFR schedules | `pcs/dcfr.rs` | DONE (α=1.5, β=0.0, γ=2.0 pinned by tests) |
| Board sampler | `pcs/sampling.rs` | DONE (uniform without replacement, deterministic on seed) |
| Regret table | `pcs/table.rs` | DONE (per-key `Row`; `to_tabular` bridge) |
| Tree walk | `pcs/walk.rs` | DONE (regret + strategy at every infoset, both players) |
| Training loop | `pcs/trainer.rs` | DONE (`PcsConfig`, `run_pcs`) |
| Artifact bridge | `RegretTable::to_tabular` | DONE (round-trips through `BlueprintPolicy::build_artifact`) |
| CLI (`train-pcs`) | — | NOT DONE |

## What's verified

- **Small-tree reduction** (`tests/pcs_reduction.rs`): the walk finds the
  analytic equilibrium of a hand-built jam-or-fold toy in two opposite
  rank configurations. This is the correctness gate — it uses the
  production walk and a tree with a provable answer.
- **Artifact round-trip** (`tests/pcs_artifact_bridge.rs`): a trained
  `pcs::RegretTable` converts to the tabular `RegretTable`, writes via
  the existing `BlueprintPolicy::build_artifact`, loads back, and the
  recovered strategies match to within quantization (worst error
  1.45e-4).
- **Board-overlap filter** (`tests/pcs_board_filter.rs`): range combos
  sharing a card with the sampled board are dropped; a fully-overlapping
  range is a no-op.
- **DCFR unit tests**, **sampler unit tests**, **engine-legality guard**
  on the toy tree.

## What is NOT verified

- **Full-range training.** Never run. See the perf blocker below.
- **Convergence.** No policy has been trained to convergence.
- **Decision D2.** No trained policy has been measured against the
  baseline. D1's full-game VBR (6.41 ± 1.50 bb) is still the number to
  beat, and a tighter D1 measurement is in progress (180 boards).

## The performance blocker (MEASURED 2026-10-08, bench at 4ce2b41)

Bench: `crates/cham-blueprint/tests/pcs_walk_bench.rs`. Run with:

    target/debug/deps/pcs_walk_bench-<hash> --ignored --nocapture

### Measured unit costs

- `Encoder::key_for` (per combo, one bucket + FNV hash): **78 ns**
- `Encoder::bucket` alone (per combo): **17 ns**
- `State::new` + deal (per iteration): **719 ns**

### Measured walk throughput (linear scaling)

    n/side   s/iter    iter/s   ns/combo-visit
         4   0.0019     516.3   ~475
        16   0.0066     151.1   ~413
        64   0.0585      17.1   ~914
       256   0.2265       4.4   ~885

Extrapolating linearly, **full range (1326/side, 2652 combos total) ≈
1.2 s/iter**. At 4h wall clock that allows ~12 000 iterations. PCS
convergence on poker typically needs 10^6-10^7. **The gap is ~100x**, not
the 20-40x the earlier (unmeasured) estimate in rev 1 of this document
claimed.

### What is NOT the bottleneck

The earlier draft blamed `key_for` (the bucket + FNV hash per combo).
**That was wrong.** `key_for` is 78 ns; total per-combo-visit cost is
~500-900 ns. The missing ~400-800 ns is elsewhere.

### Where the missing time lives (hypothesis, unprofiled)

The walk allocates per node, per combo:

- three `HashMap`s per node (`regret_delta`, `reach_sum`, `sigma_agg`);
- `strat[i].clone()` on every `sigma_agg` insert;
- `child_h`/`child_v` `Vec<f64>` per recursive call;
- `current_strategy()` allocation per combo at key time.

At ~4124 nodes × n combos × n iterations these allocator interactions
dominate.

### What to do next

**Profile first, do not guess.** Recommended:

    cargo install samply
    samply record target/debug/deps/pcs_walk_bench-<hash> --ignored walk_throughput

or on macOS: `instruments -t "Time Profiler" -file out.trace ...`.

Then optimize the actual hot path. Candidate fixes if the hypothesis
holds (in order of expected payoff):

1. **Replace the per-node HashMaps** with a pre-sorted key list + binary
   search + parallel `Vec<Vec<f64>>`. All keys at a node are known
   before the child recursion, so the map churn is avoidable.
2. **Pre-size and reuse scratch buffers.** Introduce a `Scratch` struct
   carried through the recursion: `Vec<f64>` for child EV accumulators,
   pre-allocated to n and reused per node instead of allocating fresh.
3. **Bypass `current_strategy()`** by computing regret-matching in place
   against the row.
4. **Terminal-CFV memo by (node, board).** Ranks for a fixed board don't
   change across iterations.

Do (1) and (2) together — they share the "no per-node allocation"
refactor. That should recover most of the 100x.

### What is NOT worth doing

The v3 key split (`hash(public) XOR bucket_mix(bucket)`) is not the fix:
`key_for` is already cheap, and the walk is linear in n, which rules out
key-hashing or bucket-derivation as the dominant cost. Keep it in mind
for later but do not prioritize it.

## Decision needed before D2

The design doc said "PCS ≤ 4h wall beats the shipped full-game VBR by
> 3 SE". With the current implementation the 4h wall clock is not
achievable for any training run of useful length. Either:

- Optimize first (recommended; see above).
- Relax the D2 gate's wall clock; but this weakens the design doc's
  premise that PCS is a tractable alternative to the tabular trainer.

The design doc is the authority; this status note flags the fact that
its wall-clock clause is currently unreachable.

## D1 SE run in progress

`CHAM_D1_BOARDS=180` on the same harness and bundle as the earlier
20-board run. Log: `artifacts/d1-se-2026-10-08.log`. Expected SE ≈ 0.5
(vs 1.50 at 20 boards), giving a harder D2 baseline. The result is not
part of this document; whoever picks this up should read the log tail
and append the number.

## Files this session added or changed

    crates/cham-blueprint/src/pcs/{mod,dcfr,sampling,table,walk,trainer}.rs
    crates/cham-blueprint/tests/{pcs_smoke,pcs_walk_smoke,pcs_reduction,pcs_artifact_bridge,pcs_board_filter}.rs
    crates/cham-blueprint/Cargo.toml  (adds cham-search dep)
    crates/cham-core/src/obs.rs       (adds Observables::with_hole)
    crates/cham-core/tests/obs_with_hole.rs
    docs/plans/PHASE-C-PCS-DESIGN-2026-10-08.md (design, rev 2)

## Next task, in one line

**Profile and optimize `pcs/walk.rs` key derivation** so a full-range
iteration fits the D2 wall budget. Do not write the `train-pcs` CLI
until the walk's throughput makes training feasible.
