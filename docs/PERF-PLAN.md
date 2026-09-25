# PERF-PLAN — Performance Tasks for CHAMELEON (Agent Task List)

You are a coding agent working in the CHAMELEON repo root (a Rust workspace, edition 2024).
Your job: make training and evaluation faster WITHOUT changing behavior. Every task has
exact files, steps, hard constraints, and machine-checkable acceptance criteria. Do tasks
in order. Do not skip the verification block of any task.

## 0. Ground rules (MUST follow)

- Run all commands from the repo root.
- After EVERY task run this verification block and fix anything it reports before moving on:
  ```bash
  cargo fmt --all
  cargo clippy --workspace --all-targets -- -D warnings   # must be 0 warnings
  cargo nextest run --workspace                            # must be 141+ passed, 0 failed
  ```
- FORBIDDEN at all times:
  - `unsafe` code, nightly-only features (no `std::simd`, no `#[target_feature]` nightly attrs)
  - changing RNG streams, seed derivation, or consumption order anywhere
  - changing infoset key composition (tests `key_composition`, `key_legal_mask`,
    `key_depth_alignment`, `key_fixed_size_opens_differ` must keep passing bit-exact)
  - changing artifact binary formats or `abstraction_hash` inputs
  - adding dependencies beyond this whitelist: `arrayvec, half, bytemuck, blake3, serde,
    thiserror, anyhow, rand, proptest, criterion, insta, toml, rustc-hash, ureq, walkdir`
  - making `ThreadMode::Deterministic` multi-threaded or changing single-threaded MCCFR
    update math (task T3 only touches the Hogwild path)
- Reference baselines below were measured on Apple M1 with `target-cpu=native`. Save your
  own before/after bench outputs:
  ```bash
  cargo bench --workspace > bench-before.txt   # do this ONCE before T1
  # after each task:  cargo bench --workspace > bench-after-T<N>.txt
  ```

## 0.1 Current baseline (your numbers to beat)

| Bench                 | Baseline            | Meaning                     | Target (acceptance)      |
|-----------------------|---------------------|-----------------------------|--------------------------|
| `eval_evaluate7`      | 12.471 µs / 1000 evals | ≈ 80M hand-evals/s       | ≤ 10.0 µs (≥ 100M/s)    |
| `engine_apply`        | 622.2 µs / 2000 steps  | ≈ 3.2M actions/s         | ≤ 200 µs (≥ 10M/s)      |
| `mccfr_iter_200bb_tiny` | 2.702 ms / 20 traversals | ≈ 7.4k traversals/s   | ≤ 2.0 ms (≥ 10k/s)      |
| `encode_flop`         | 5.33 ms             | flop histogram encode       | ≤ 3.5 ms                 |
| `encode_river`        | 728 ns              | river encode                | keep ≤ 800 ns            |
| `match_20_deals`      | 25.98 µs            | match harness throughput    | keep ≤ 30 µs             |
| `solve_rnr_400`       | 6.81 ms             | river RNR solve             | keep ≤ 7 ms              |

Quality gates that must stay green: 141 tests, proofs P-1..P-4 (`chameleon verify --proofs`),
clippy `-D warnings` clean.

## T1 — Hand evaluator: 80M → ≥100M evals/s

**Files:** `crates/cham-core/src/eval/mod.rs` (function `evaluate7`, ~line 304),
`crates/cham-core/benches/eval.rs`.

1. Read `evaluate7` and its helpers end to end. Note every branch, lookup table, and shift.
2. Optimize the scalar path:
   - single pass over the 7 cards building rank counts and suit bitmasks (no re-iteration);
   - flush/straight detection with integer bitmask tricks, not per-suit loops with early
     returns that mispredict;
   - mark helpers `#[inline]`; avoid bounds-checked indexing in the hot loop by asserting
     lengths once before the loop (no `unsafe`, no `get_unchecked`).
3. Add a batch API to expose instruction-level parallelism:
   ```rust
   pub fn evaluate7_batch<const N: usize>(hands: &[[Card; 7]; N], out: &mut [u16; N])
   ```
   processing hands independently (the CPU pipelines 8 scalar evals in parallel; the
   compiler autovectorizes shared table lookups under `target-cpu=native`).
4. Extend `benches/eval.rs` with `eval_evaluate7_batch8` benching the batch API.
5. Verify + acceptance:
   - `eval_golden_50`, `eval_bitmask_vs_naive`, `eval_flush_wheel_edges`, `hand2_canonical_169`
     pass UNCHANGED (do not touch expected values);
   - `eval_evaluate7` ≤ 10.0 µs; `eval_evaluate7_batch8` ≤ 80 µs per 8×1000 batch.

## T2 — Engine: 3.2M → ≥10M actions/s (biggest gap)

**Files:** `crates/cham-core/src/engine/mod.rs`, `crates/cham-core/benches/engine.rs`,
`crates/cham-core/src/obs.rs`.

1. First find the hotspot — split the existing bench into two sub-benches:
   `engine_legal_only` (State::new + legal_actions loop, no apply) and
   `engine_apply_only` (apply a precomputed legal action). Run, record which dominates.
2. Likely fixes (apply the ones your measurements confirm, in this order):
   - `State` is `Copy` but large; every `apply` copies the whole struct. Add
     `pub fn apply_in_place(&mut self, a: Action) -> Result<StepInfo, ...>` that mutates
     internally, and make the bench loop use it. Keep the old `apply` as a thin
     copy-then-delegate wrapper so all existing callers still compile.
   - `legal_actions` recomputes min-raise and pot math that `apply` also computes; hoist
     into small `#[inline]` helpers or cache per-street invariants in the state.
   - Avoid re-deriving static per-street data inside the loop; assert-then-index where
     bounds checks show up.
3. Verify + acceptance:
   - `replay_matches`, `legal_order_pinned`, `min_raise_progression`,
     `short_allin_no_reopen`, `uncalled_return`, `split_odd_chip`, `illegal_action_errors`,
     `state_is_copy_no_heap` pass UNCHANGED;
   - `engine_apply` ≤ 200 µs per 2000 steps (≥ 10M actions/s).

## T3 — Trainer hot path: snapbatch regret merging (kill CAS contention)

**Files:** `crates/cham-blueprint/src/table.rs` (CAS loop at lines ~60-72, `fetch_add` at
~70), `crates/cham-blueprint/src/trainer.rs` (ThreadMode handling), 
`crates/cham-blueprint/src/traversal.rs`.

1. Add `ThreadMode::Snapbatch` (or a `flush_every: u32` field on the Hogwild mode):
   each worker thread accumulates regret/strategy/visit deltas in a thread-local
   `Vec<(u32 slot, u32 delta)>` buffer (pre-reserve 4096 entries; flush when full or
   every K traversals, K = 64 default). Flush = sequential `fetch_add` per slot.
   Integer adds are associative → Hogwild results stay statistically identical;
   single-thread `Deterministic` mode is NOT touched.
2. Route `traversal.rs` writes through a small `RegretSink` trait: `Deterministic` writes
   directly (existing behavior, bit-exact), `Snapbatch` buffers. No signature changes to
   `Traversal::walk` beyond the sink being passed via `&mut`.
3. Verify + acceptance:
   - `hogwild_smoke`, `determinism_same_seed_and_resume`, `resume_continues_bitstream`,
     `rm_plus_floors`, `opponent_regrets_never_exist` pass;
   - `mccfr_iter_200bb_tiny` ≤ 2.0 ms;
   - run `chameleon train-bp --mode robust --seed 7` (tiny recipe): wall-clock improves
     ≥ 1.3× vs before (record both numbers in the commit message).

## T4 — Traversal key computation (only if T3 alone doesn't hit target)

**Files:** `crates/cham-blueprint/src/traversal.rs` (lines ~78 and ~110 call
`enc.key(&obs, seq)` per visit), `crates/cham-engine/src/encoder.rs`.

1. Micro-bench first: add `encode_key_only` criterion bench calling `enc.key` on a fixed
   observation 1000×. If it's < 5% of traversal time, SKIP this task and say so.
2. If it matters: speed up `Encoder::key` internals — precompute at `Encoder::build` time
   everything that depends only on street/config; FNV-1a over fixed-size arrays with
   iterators (no bounds checks in hot loop). THE KEY VALUE MUST NOT CHANGE — the tests
   `key_composition`, `key_legal_mask`, `key_depth_alignment`,
   `key_fixed_size_opens_differ`, `no_mc_in_encode` must pass bit-exact.
3. Acceptance: key tests bit-exact; `mccfr_iter_200bb_tiny` improves further or task is
   skipped with the measurement noted.

## T5 — Worker threads configurable (M1 P-core alignment)

**Files:** `crates/cham-blueprint/src/trainer.rs` (hardcoded `8` at lines ~189 and ~209),
`crates/cham-cli/src/cmd/train_bp.rs`.

1. Default `threads` to `std::thread::available_parallelism()`, and add
   `--threads <N>` to the `train-bp` CLI (clap arg, override before config resolution).
2. Acceptance: `chameleon train-bp --mode robust --threads 4` runs; doc-comment notes that
   on Apple M1 (4 P-cores + 4 E-cores) 4 workers usually beats 8; no other behavior change.

## T6 — `verify --perf` must ENFORCE gates, not just print

**Files:** `crates/cham-cli/src/cmd/verify.rs` (lines ~77-79 currently just print).

1. After benches have run, read `target/criterion/<bench>/new/estimates.json` for the six
   benches, convert to the P1..P6 gates from SPECS/00 §6, print a table
   `gate / threshold / measured / PASS|FAIL`, and push a failure into the existing
   `failures` vec so exit code is nonzero.
2. If `estimates.json` files are missing, print "run `just bench` first" and exit nonzero
   (do not silently pass).
3. Acceptance: `chameleon verify --perf` fails at current engine speed (pre-T2), passes
   after T1+T2; `cli_parse_surface` / `verify_exit_codes` tests updated for the new path.

## T7 — Eval guardrail: refuse to ladder/probe with untrained artifacts

**Symptom (already observed):** `ladder --fast` rows `callbot +0.0 ± 0.0` and
`jamfix +0.0 ± 0.0` are exact mirror matches — the `full` agent silently fell back
(uniform/zero-weight policy), and LBR read ~29,973 mb/hand. Strength numbers are
meaningless in this state and it looks like a bot bug.

**Files:** `crates/cham-cli/src/cmd/ladder.rs`, `crates/cham-cli/src/cmd/probe.rs`,
`crates/cham-cli/src/cmd/ab.rs`, `crates/cham-agent/src/pipeline.rs`.

1. Mirror `play`'s artifact refusal: if the agent config requires trained policy/router
   artifacts and they are missing, print which files are missing and exit nonzero.
2. During any eval run, count `fallback_used` decisions (field already exists in
   `cham-agent/src/trace.rs`); if fallback rate > 20% of decisions, print a prominent
   `WARNING: agent fell back on X% of decisions — artifacts missing or stale` and write it
   into the ledger entry.
3. Acceptance: new test `ladder_refuses_without_artifacts` (pattern it on
   `play_budget_refusal_without_artifacts`); existing ladder tests updated for refusal.

## T8 — Final verification (run once, all must be green)

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace                       # 141+ pass
cargo bench --workspace > bench-after-all.txt       # compare vs bench-before.txt
cargo run -q -p cham-cli -- verify --perf --count-infosets --proofs   # GREEN incl. perf gates
just fast                                            # end-to-end: test, bench, verify, probe, ladder, dashboard
```

Then update README.md Status section: new bench numbers, note that `verify --perf` now
enforces gates, note the ladder artifact guardrail. Commit per task (T1..T7 separate
commits), message format: `perf(T<N>): <one-line> — <before>→<after> on <bench>`.

## Expected impact map (from the design brainstorm)

| Task | Brainstorm item | Expected gain |
|------|-----------------|---------------|
| T1   | Tier-1 native + eval SIMD/ILP | eval gate closes (~1.25×) |
| T2   | Tier-2 apply/legality surgery | engine 3.2M→10M+ (~3×) |
| T3   | Tier-2 snapbatch (#6)          | trainer 1.3–2× |
| T4   | Tier-2 kill per-visit hashing (#5) | trainer +1.1–1.5× if key is hot |
| T5   | Tier-1 P-core pinning (#2)    | trainer 1.2–1.5× wall-clock |
| T6   | meta (#19)                    | gates become real |
| T7   | eval correctness              | strength numbers become meaningful |
