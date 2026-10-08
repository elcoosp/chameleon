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

### Where the time actually lives (MEASURED 2026-10-08, commit 2e75b32)

Phase split from `profile_phase_split` (in-walk thread-local timers,
enabled via `pcs::walk::profile_enable`):

    n=16/side, 20 iters:  total 1288 ms
      key deriv   :  758.829 ms  (58.9%)
      aggregation :  163.251 ms  (12.7%)
      terminal    :  241.292 ms  (19.7%)
      other       :  124.978 ms  ( 9.7%)

    n=64/side, 20 iters:  total 1791 ms
      key deriv   : 1092.441 ms  (61.0%)
      aggregation :  167.545 ms  ( 9.4%)
      terminal    :  436.411 ms  (24.4%)
      other       :   94.545 ms  ( 5.3%)

**The "key deriv" region is 59-61%.** This corrected TWO earlier claims
in this document:

1. Rev 1 blamed the FNV hash. Wrong.
2. Rev 2's correction said "key_for is not the bottleneck." Also wrong.

Both were wrong because the isolated `key_for` bench measured 78 ns on
a **preflop** node (hole-only bucket, no board work), while the walk's
timer wraps a block: `Observables::with_hole` + `Encoder::key_for` +
`RegretTable::row_mut` lookup + `Row::current_strategy()`. The last of
those allocates a `Vec<f64>` per combo per node. At 4124 nodes × n
combos × iterations, that is tens of thousands of heap allocations per
iteration and is the dominant cost inside the block.

The same reasoning explains why the isolated `key_for` bench was
cheap: it warmed the eq_cache and then measured a hash over ~100 bytes.
In the walk, the allocation around it is what costs.

### What to do next (concrete, ordered by measured payoff)

1. **Eliminate `current_strategy()`'s Vec allocation.** Replace the
   walk's `strat: Vec<Vec<f64>>` with `strat: Vec<[f64; 12]>` (na ≤ 12
   is invariant I8), and add `Row::current_strategy_into(&self, out:
   &mut [f64])`. The walk writes into a stack array; no heap. Same for
   `sigma_agg: HashMap<u64, Vec<f64>>` → `HashMap<u64, [f64; 12]>`
   (arrays are `Copy`, so `sigma_agg[k]` no longer clones).

   Expected: recover ~40-50% of total (the allocation-dominated portion
   of the 59-61%). One refactor; mechanical; testable by re-running
   `profile_phase_split` and confirming the key-deriv share drops.

2. **Terminal CFV memo by (node, board).** Terminal showdown CFVs
   depend only on ranks and reach; ranks for a fixed board don't change
   across iterations. Currently recomputed every iteration. With 4124
   nodes and a per-iteration board, a memo table keyed by (node, board)
   amortizes across repeat draws. Lower priority than (1) because
   terminal is 20-25% and the memo adds bookkeeping.

3. **Do NOT do the v3 key split.** Still true: FNV is cheap; the
   profile shows the cost is elsewhere.

After (1) and a re-measure, the walk should land at ~700-900 ns/combo
visit instead of the current ~900-1600. Full range would be ~0.6-0.8
s/iter. Still ~50x off the D2 budget but the next round of optimization
has a clear target (whatever dominates after (1)).

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
