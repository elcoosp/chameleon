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

## Pipeline verified end-to-end (2026-10-08, CLI at 4f45779)

`train-pcs` writes an artifact that the D1 harness loads and queries
with 0.03% miss rate. The full chain runs without manual intervention:

    train-pcs (2 iterations, 30 combos/side)
      -> run_pcs (PCS walk, per-board ranks)
      -> to_tabular (PCS table -> tabular table)
      -> BlueprintPolicy::build_artifact (policy.bin)
      -> BlueprintPolicy::load
      -> d1_fullgame_vbr (queries per combo)

Reproduce:

    chameleon train-pcs --iters 20 --combos 30 --force \
        --wall-budget-s 300 --out /tmp/pcs-30smoke

    CHAM_D1_BP=/tmp/pcs-30smoke/robust \
    CHAM_D1_CONFIG=config/abstraction-tiny.toml \
    CHAM_D1_BUCKETS=artifacts/buckets-tiny \
      target/debug/deps/d1_fullgame_vbr-<hash> --ignored --nocapture

Result (3 boards):

    policy miss: 0.03%  (24 / 68850)

That the shipped bundle's abstraction (`artifacts/agent-honest-19dim/
abstraction.toml`) is byte-identical to `config/abstraction-tiny.toml`
means the shipped blueprint and any PCS-trained tiny artifact key the
same, so a D2 comparison is apples-to-apples.

The **VBR number itself is not meaningful at 20 iterations** — the
policy is cold-started. What this verifies is the integration: a PCS
artifact is a real, loadable, queryable `BlueprintPolicy`, not a
partially-written file that happens to parse.

### The `train-pcs` CLI

    chameleon train-pcs \
        --iters N --combos M \
        --config config/abstraction-tiny.toml \
        --buckets artifacts/buckets-tiny \
        --out artifacts/pcs-<name> \
        --dcfr-alpha 1.5 --dcfr-beta 0.0 --dcfr-gamma 2.0 \
        --wall-budget-s 14400 \
        [--force]  # bypass the wall guard, for tiny diagnostic runs only

The command estimates the wall clock from the measured per-combo-per-node
cost and **refuses to launch** a run that exceeds `--wall-budget-s`. For
example, `--iters 10000 --combos 30 --wall-budget-s 60` prints
`REFUSING — estimate 0.7 h exceeds --wall-budget-s 60` and exits 1.

This is deliberate: at the current ~1 s/iter full-range walk, a real
training run cannot finish inside 4h. The guard makes the CLI honest
about that instead of letting a user launch a run that will be killed
by the next session boundary.

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

### Optimization attempts: all reverted, measurement environment unreliable

Three attempts on 2026-10-08 evening, all reverted:

1. **Stack arrays** (9b1d712): `Vec<Vec<f64>>` → `Vec<[f64; 12]>`. A/B
   test showed no difference in the stable profile; reverted.
2. **FxHashMap + reused strategy buffers** (df6f35d): swapped std
   `HashMap` for `rustc_hash::FxHashMap` in `pcs/table.rs` and the
   walk's local aggregation maps, and replaced `current_strategy()`'s
   per-call `Vec` with a reused buffer via `current_strategy_into`.
   Reverted without a stable measurement.
3. **River-reduction and component-breakdown benches**
   (141f2d8, 3b71b40): retained — they are diagnostics, not
   optimizations.

### Why the measurements are untrustworthy

The `df6f35d` re-measurement at 19:33-19:35 showed this:

    walk_throughput n=64:  0.776 s/iter  (run 1)
                           0.107 s/iter  (run 2)
                           0.080 s/iter  (run 3)

10x variance between consecutive runs of the same binary, same
build. Earlier in the session the noise floor was ~5% across five
runs; the machine was clearly loaded by something else during this
window (load average rose from 3.27 to 5.65 across the A/B, then
higher).

Critically: `key_loop_breakdown` stage 4 code is **unchanged** by
`df6f35d` (the bench still calls `current_strategy()`, not
`current_strategy_into()`). Its reported cost jumped from 52 ns to
296 ns — a 6x "regression" on unchanged code. That single number
proves the measurement is not reflecting the code change.

### What to do about it

Any future perf work on `pcs/walk.rs` must:

1. **Verify the machine is idle** before starting (`uptime` load < 1.0
   per core; kill browsers, editors, other cargo builds).
2. **Run the 5-run noise floor** on the current HEAD before changing
   anything. If the 5-run spread at n=64 is > 10%, stop — the
   environment is unusable for this.
3. **A/B on the same build tree** with `git checkout` between the two
   SHAs, running the same tests 5 times each.

The scaffolding to do this is all in place:
- `pcs_walk_bench.rs` has `walk_throughput`, `profile_phase_split`,
  `key_for_by_street`, `key_loop_breakdown`.
- The phase-split timers are in `walk.rs` (`profile_enable` /
  `profile_take`).

### The stable facts (from before the environment went bad)

- Key-deriv block: ~60% of walk time.
- Terminal CFV: ~24%.
- Aggregation: ~8%.
- Other: ~5-9%.

The `key_for` isolated cost: 50-53 ns warm across all streets; 1.97 us
cold on river nodes. The walk's key-deriv share is dominated by warm
`key_for` calls, not by the FNV hash and not by the surrounding Vec
allocations (attempt 1 disproved).

### The next optimization, once a quiet machine is available

Candidate list, in order of expected payoff. Each must be A/B tested
per the protocol above:

1. **FxHashMap** alone (without the buffer change). `row_mut` is 45 ns
   with std `HashMap`; FxHashMap is known to help integer-keyed maps.
   Expected 5-15% on the key block.
2. **Terminal CFV memo by (node, board).** ~24% of total is recomputed
   every iteration. A memo table keyed by `(node_idx, board_hash)` for
   showdown terminals amortizes across repeated board draws. Expected
   ~15-20% of total.
3. **Bucket memo at the encoder level.** The 1.97 us cold river
   `key_for` is largely `river_equity` on a cache miss. The encoder has
   `eq_cache` (cap 400k) but PCS's access pattern (every combo × every
   distinct board) may exceed the effective cache. Measure the hit
   rate; if low, size the cache to the working set or use a
   board-shared memo.

None of these is the 100x speedup D2 would need. Realistically the
walk lands at 0.5-0.8 s/iter at full range after these, which is 4h /
5000-8000 iterations. **D2 remains out of reach without a structural
change** — e.g. multiple threads (hogwild over the same table), or
sampling fewer boards (the current PCS scheme already samples one
board per iteration; more per iteration would amortize setup).

### What to do next (revised, measured)

The stable measurements say:

- **key deriv block: 59-65%** — inside this, `Encoder::key_for` is ~78
  ns (isolated, preflop); the walk measures ~128 ns per call at n=64.
  The gap is `Observables::with_hole` + `RegretTable::row_mut` +
  `current_strategy`. All are small individually. The rest of the 59-65%
  is **the bucket computation for river nodes**, where `river_equity`
  dominates. The isolated `key_for` bench uses a preflop state, which
  underestimates river cost by an unknown factor.
- **terminal: 22-24%** — showdown CFVs, recomputed per iteration.
- **aggregation: 8%** — small.
- **other: 3-5%** — small.

**Concrete next steps (in order):**

1. **Measure `key_for` on a river state** (not preflop). If river
   `key_for` is 5-10x preflop, then the "key deriv" share is entirely
   river node bucket work, and the fix is a memo table on
   `(board, hole) → bucket` with a proper LRU (the existing eq_cache
   is per-encoder and evicts on cap; the size vs PCS's working set is
   unknown). Add a bench for it.

2. **Terminal CFV memo by (node, board).** Terminal CFVs depend only
   on ranks and reach; ranks for a fixed board don't change across
   iterations. A memo keyed by (node_idx, board_hash) amortizes across
   repeated board draws (birthday-paradox likely over 10^6 iterations
   over 2.6M boards). Worth ~20% of total.

3. **Do NOT touch strat/sigma_agg/regret_delta data structures again**
   — they were tried and the change is neutral.

### Correction history of this section

Rev 1 blamed the FNV hash. Wrong.
Rev 2 said "the FNV is not the bottleneck" without measuring. Right conclusion, wrong reason.
Rev 3 (this) confirmed the alloc hypothesis and tested it. Disproved.
The stable, measured facts are: key-deriv share ~60%, terminal ~24%, both recomputed per iteration.

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
