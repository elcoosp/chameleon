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

## The performance blocker

Measured: `pcs_artifact_bridge` runs 20 iterations on 4-combo ranges in
5.08 s (≈0.25 s/iteration at 8 combos total). The walk calls
`Encoder::key_for` once per combo per non-terminal node, and each call
rebuilds the public hash from scratch — including the per-street
history bytes. The tree has 4124 nodes; a full walk is ~4124 × 8 key
derivations. At 1326 combos per side that becomes ~4124 × 2652 calls
per iteration.

Rough scaling to full range: 100-200x slower per iteration than the
8-combo measurement, i.e. **25-50 s/iteration**. The D2 gate allows 4h
of wall clock; 10^6 iterations at 25 s each is 290 days. The trainer
cannot reach the gate without an order-of-magnitude speedup.

The likely fix path (next session):

1. **Batch the key derivation.** At a fixed node the public-part hash is
   identical for all combos (street, player, spr_band, legal_mask, seq
   bytes) — only the bucket differs. At abstraction v3 the key is
   `hash(public) XOR bucket_mix(bucket)`, so all combos' keys can be
   produced from one hash and a table of bucket mixes. At v2 the key is
   an inline FNV over `hole`-dependent bytes, so this optimization is
   v3-only. **Consider switching the PCS trainer to abstraction v3 by
   default**; the D1 harness accepts any config.

2. **Hoist per-node work out of the combo loop.** The walk rebuilds
   `Observables::view(&st, ...)` once per node (good), then
   `with_hole` per combo (cheap clone). The expensive part is
   `key_for`: 128-byte FNV hash per combo. Reduce to one FNV per node +
   a table lookup per combo if v3.

3. **Cache terminal CFVs by (node, board) across iterations.**
   Terminal showdown CFVs depend only on ranks and reach; the ranks for
   a given board don't change. Currently they are recomputed every
   iteration. If the same board is drawn twice (birthday-paradox
   likely over 10^6 iterations over 2.6M boards), the computation
   repeats. A memo table keyed by board would amortize.

4. **Persistent state reuse.** Per iteration the walk rebuilds ranges
   from the board. This is unavoidable. But the `State` construction is
   per iteration — bench it, it may be cheaper than the key derivation,
   or it may dominate.

Do (1) first — it is the largest win and it is structural, not
micro-optimization. Then re-measure.

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
