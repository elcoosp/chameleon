# Broad-perf-plan — NON-TRAINING Performance Tasks (Agent Task List)

You are a coding agent working in the CHAMELEON repo root (Rust workspace, edition 2024).
This plan covers performance improvements EVERYWHERE EXCEPT the blueprint trainer:
evaluation harness, play-time decision path, memory/artifacts, preprocessing, dev loop,
and the Slumbot client. Companion file: `PERF-PLAN.md` (training-side tasks). Either plan
can be executed independently; where both touch `verify.rs`, do PERF-PLAN T6 first.

## 0. Ground rules (MUST follow)

- Run all commands from the repo root.
- After EVERY task run this verification block and fix anything it reports before moving on:
  ```bash
  cargo fmt --all
  cargo clippy --workspace --all-targets -- -D warnings   # must be 0 warnings
  cargo nextest run --workspace                            # must be 141+ passed, 0 failed
  ```
- FORBIDDEN at all times:
  - `unsafe` code, nightly-only features, new crates beyond the whitelist:
    `arrayvec, half, bytemuck, blake3, serde, thiserror, anyhow, rand, proptest, criterion,
    insta, toml, rustc-hash, ureq, walkdir`
  - changing RNG streams, seed derivation, or per-deal seed values (parallelism must
    RE-DERIVE the same seeds a sequential run would use, never re-roll them)
  - changing infoset keys, artifact binary formats, `abstraction_hash` inputs
  - statistical shortcuts that change error rates (SPRT alpha/beta, CI confidence, Holm
    correction are fixed by SPECS/08 — never loosen them to save time)
- QUALITY-PRESERVATION RULE — every change in this plan must be one of exactly three kinds:
  1. **Pure-function memoization** keyed by content hash (same input → same cached output);
  2. **Statistically identical-by-construction** (unbiased estimators, fixed error rates);
  3. **Transport/layout change** proven bit-exact by existing golden/hash tests.
  If a change does not fit one of these three, stop and re-read the task.

## 0.1 Current state of the eval stack (why these tasks exist)

Measured on Apple M1 (`target-cpu=native`): `match_20_deals` 26.0 µs (harness itself is
fast); `solve_rnr_400` 6.81 ms. The bottlenecks are structural:

- `crates/cham-cli/src/cmd/ladder.rs` runs opponents **sequentially** in a plain
  `for opp in &specs` loop (~line 39), fixed 2500 deals/pair, no early stop.
- **ladder's hero factory is a stub**: `let factory = || Box::new(CallBot)` — the real
  agent config is ignored. That is why your ladder showed `callbot +0.0 ± 0.0` (mirror
  match) and LBR ~29,973 mb/hand. NOTHING in this plan is meaningful until B1 fixes this.
- `crates/cham-eval/src/matcheng.rs` hardcodes `vr_factor: 1.0` (lines ~130/140) — the
  variance-reduction machinery in `crates/cham-eval/src/vr.rs` exists but is not wired in.
- `crates/cham-search/` has **no caching** — every river trigger rebuilds subgames from
  scratch (`solve()` at `solve.rs:305`, `Subgame::build` at `subgame.rs:48`).
- `crates/cham-blueprint/src/policy.rs` loads whole owned-bytes artifacts eagerly
  (mmap is forbidden by D-008 `forbid(unsafe_code)`).

## B1 — Wire the REAL hero agent into ladder (prerequisite for everything)

**Files:** `crates/cham-cli/src/cmd/ladder.rs` (the stub factory ~line 37),
`crates/cham-cli/src/cmd/play.rs` (shows the correct construction to copy),
`crates/cham-agent/src/loader.rs` (`pub fn load_agent(bundle, routing, depth) -> LoadedAgent`).

1. In `play.rs`, read how the agent is built: `cham_agent::loader::load_agent(bundle,
   routing, depth)` → `LoadedAgent { encoder, experts, robust, bayes, ... }` →
   `ChameleonAgent::new(mode, encoder, router, experts, robust, bayes, None)`.
2. Reproduce exactly that construction in `ladder.rs` (and `probe.rs`/`ab.rs` if they have
   the same stub), replacing the `CallBot` closure. The factory must return a fresh
   `ChameleonAgent` per call (agents own per-hand tracker state — do NOT share one instance
   across matches; construct per match).
3. If artifacts under `artifacts/agent/` are missing, print which files are missing and
   exit nonzero (mirror `play`'s refusal path).
4. Count `fallback_used` decisions; if > 20 % of decisions fell back, print a prominent
   `WARNING: fallback on X% of decisions — artifacts missing or stale` and record it in
   the ledger entry (field `fallback_used` already exists in
   `cham-agent/src/trace.rs`).
5. Acceptance:
   - new test `ladder_refuses_without_artifacts` (pattern it on
     `play_budget_refusal_without_artifacts`);
   - with trained artifacts present, `ladder --fast` rows vs `callbot` are NOT exactly
     ±0.0 unless the policy truly mirrors it;
   - all existing cli tests pass.

## B2 — Parallelize the ladder across opponents (bit-identical results)

**Files:** `crates/cham-cli/src/cmd/ladder.rs` (sequential loop ~line 39),
`crates/cham-eval/src/matcheng.rs` (`MatchRunner::run` at line 83; `run_pool` at 147 is
also sequential — leave it or upgrade it the same way).

1. Use `std::thread::scope` to spawn one thread per opponent (≤ 9 threads; M1 has 8
   cores — matches are light on memory, this is safe). Each thread calls
   `MatchRunner::run` exactly as the sequential loop does today.
2. SEED RULE (critical): keep per-opponent seeds derived, not shared:
   `base_seed ^ ((index as u64) << 32)` — same scheme `run_pool` already uses
   (matcheng.rs ~line 158). Document it in a comment. Never use the same `base_seed`
   for two opponents (current ladder code reuses `0x1AD` everywhere — fix that).
3. Collect results into a `Vec` and print in **pool order** (not completion order) so
   output stays deterministic.
4. Acceptance:
   - wall-clock for 9 opponents ≤ 2× the slowest single match (was ≈ 9×);
   - `match_20_deals` bench unchanged; `matcheng_end_to_end` passes;
   - determinism: same command twice → identical numbers (per-deal seeds unchanged).

## B3 — Chunked SPRT early-stop in the ladder

**Files:** `crates/cham-cli/src/cmd/ladder.rs`, `crates/cham-eval/src/stats.rs`
(`pub fn sprrt(diffs, delta0, delta1, alpha, beta) -> Result<SprtState, EvalError>` at
line 121; `MatchResult.per_deal_profits` already carries per-deal profits).

1. Run each opponent match in chunks of 250 deals (append mode: loop
   `MatchRunner::run` with `deals = 250` and a chunk-derived seed, or refactor `run` to
   accept a starting seat index — choose the one that keeps per-deal seeds identical to
   the current single-shot run for the same total deals; verify this with a test).
2. After each chunk, feed cumulative per-deal profits to `sprrt` with
   `alpha = beta = 0.05`, `delta0 = 0.0`,
   `delta1 = sprt_delta_mb` (new per-opponent key in `config/pool.toml`, default 25.0
   mb/seating). Stop early only on a decisive `SprtState`; otherwise run the full budget.
3. Print `sprt-stop` next to early-stopped rows and record the decision + seatings saved
   in the ledger entry.
4. Acceptance:
   - a matchup against a clearly-beatable/losing opponent stops before the full 2500
     deals; a coin-flip matchup runs the full budget;
   - error rates unchanged (alpha/beta are constants from SPECS/08, not configurable);
   - new test: `sprt_chunking_matches_full_run_seeds` (chunked seats equal single-shot
     seats deal-for-deal);
   - existing `sprrt_boundaries` test stays green.

## B4 — Wire the variance-reduction (VR/AIVAT) machinery into match results

**Files:** `crates/cham-eval/src/matcheng.rs` (hardcoded `vr_factor: 1.0` at ~130/140),
`crates/cham-eval/src/vr.rs` (existing machinery — read it first),
`crates/cham-eval/src/stats.rs` (`session_cluster_ci`).

1. Read `vr.rs` end to end and its test `aivat_variance_reduction` — the estimator exists;
   the task is wiring, not inventing.
2. Compute the VR-adjusted standard error from `per_deal_profits` and set the real
   `vr_factor` on `MatchResult` (1.0 means "no reduction applied" — after this task a
   noisy matchup must show VR > 1.0).
3. Use the reduced SE in the ladder's printed `± mb/seating` and in the ledger CIs.
4. Acceptance:
   - ladder output shows `VR ×` > 1.0 on at least the archetype rows;
   - CI widths shrink at the same seating count; `aivat_variance_reduction`,
     `session_cluster_ci_covers`, `duplicate_profit_formula` stay green;
   - if `vr.rs` turns out to require extra per-deal metadata the harness doesn't record,
     implement the duplicate-pairing reduction only and document what AIVAT still needs
     (do not fake the factor).

## B5 — River search: content-keyed subgame cache (L1)

**Files:** `crates/cham-search/src/subgame.rs` (`Subgame::build` line 48),
`crates/cham-search/src/solve.rs` (`solve` line 305), `crates/cham-search/src/lib.rs`.

1. Add a thread-safe memo (`Mutex<HashMap<u64, Arc<Cached>>>` — std only) keyed by a
   blake3-derived u64 of EVERYTHING `Subgame::build` reads: canonical board class, pot,
   SPR band, action ladder, abstraction hash. Cache stores the immutable built subgame
   (tree structure, codelists) — NOT solver output.
2. `solve()` stays a pure function of (subgame, ranges, iters, seed): results must be
   bit-identical whether the subgame came from cache or fresh build. Add test
   `solve_cached_equals_fresh` asserting this.
3. Expose hit/miss counters (printed at process exit or via trace) so the gain is
   measurable.
4. Acceptance:
   - `solve_rnr_400` bench improves ≥ 1.3× on its second iteration within one process
     (criterion repeats — cache hits dominate);
   - `solver_determinism_fixed_iters`, `solver_matches_independent_oracles`,
     `subgame_card_removal_and_class_collapse`, `illegal_action_never` all stay green;
   - memory: cache bounded (LRU cap 256 entries or evict-by-generation) so a long match
     cannot grow it unboundedly.

## B6 — Optional solver warm-start behind a flag (default OFF)

**Files:** `crates/cham-search/src/solve.rs`, `crates/cham-search/src/trigger.rs`,
`crates/cham-cli/src/cmd/play.rs`.

1. Add `--search-warmstart` CLI flag (default off) storing the previous solve's final
   strategy per (board class, SPR band) and using it as the initial strategy for the next
   solve at the same key. Deterministic (same hands → same sequence → same warm starts).
2. Validation gate before anyone enables it: run 1000 river spots vs the independent
   oracle (`solver_matches_independent_oracles` harness) with and without warm-start;
   mean |ΔEV| must be < 0.5 mb and no spot may flip the argmax action. If validation
   fails, leave the flag off and record the numbers in the PR description.
3. Acceptance: flag-off path is bit-identical to today; validation harness exists as a
   test; `budget_wallclock_only_live` stays green.

## B7 — Lazy per-street artifact loading

**Files:** `crates/cham-blueprint/src/policy.rs` (whole-file owned-bytes load, line ~47),
`crates/cham-agent/src/loader.rs`.

1. Split the load: read the artifact header + provenance eagerly, defer row payloads per
   street (the file layout is header | provenance | rows — rows must gain a street tag or
   an offset index; if the format cannot express it WITHOUT a format change, instead do
   the cheaper variant: keep the format, load eagerly, but only parse/quantize-decode the
   street's rows on first use — measure both, pick the one that does not change
   `ARTIFACT_MAGIC`/version).
2. Same bytes, same hashes: `loader_hash_guards` and `artifact_hash_printed` must pass
   unchanged.
3. Acceptance: a golden decision replay (fix a seed, record 100 decisions before the
   change) produces bit-identical decisions after; artifact load time for river-only use
   drops (add `artifact_load` criterion bench, see B10).

## B8 — Pre-load memory guard (16 GB machines)

**Files:** `crates/cham-blueprint/src/policy.rs`, `crates/cham-agent/src/loader.rs`.

1. Before allocating, read the artifact's declared row count × row width (+ header) from
   meta/header, sum across the bundle, and refuse to load if the estimate exceeds a
   budget (new config key `memory_budget_mb`, default 8192) with a clear error naming the
   largest contributor.
2. Acceptance: new test `loader_refuses_over_budget` (small synthetic budget, exit code
   path covered); normal loads unaffected.

## B9 — train-buckets: reuse equity histograms across k-means sweeps

**Files:** `crates/cham-engine/src/` (bucket/k-means code — locate via
`rg -n "kmeans|emd" crates/cham-engine/src`), `crates/cham-cli/src/cmd/train_buckets.rs`.

1. Cache the expensive per-flop/turn equity histograms (the `encode_flop` 5.3 ms path)
   to `artifacts/histo-cache/<content-hash>.bin` keyed by blake3 of (board canonical id,
   range config, histogram params — NOT k).
2. Re-running `train-buckets` with a different `--profile`/k must hit the cache and redo
   only Lloyd assignment/centroid steps.
3. Acceptance: buckets produced with a warm cache are **byte-identical** to a cold run
   (`kmeans_emd_determinism` + a new test comparing final `meta.json` + bucket files);
   second full-profile run ≥ 5× faster.

## B10 — Measurement: latency benches + dev-loop speed

**Files:** `crates/cham-cli/src/cmd/verify.rs`, workspace `Cargo.toml`,
new `crates/cham-agent/benches/decision.rs`.

1. Add criterion benches:
   - `decision_latency`: end-to-end hero decision (tracker update → router → policy →
     mixture), search DISABLED;
   - `decision_latency_search`: same, with the river search forced ON;
   - `artifact_load`: `load_agent` on the tiny bundle.
2. Extend `verify --perf`'s gate table (if PERF-PLAN T6 already made it enforce gates,
   add rows: `decision_latency p99 < 1 ms`, `decision_latency_search p99 < 50 ms`;
   otherwise print the table and note the thresholds).
3. Dev loop: in root `Cargo.toml`, keep `[profile.test] opt-level=3` ONLY where numerics
   matter, via per-package overrides:
   ```toml
   [profile.test.package.cham-core]
   opt-level = 3
   [profile.test.package.cham-blueprint]
   opt-level = 3
   [profile.test.package.cham-engine]
   opt-level = 3
   ```
   and set `[profile.test] opt-level = 1`. Integer fixed-point tests are level-insensitive;
   confirm `fuzz_1m_release`, `eval_golden_50`, `p1_es_mccfr_validity` still pass and full
   test wall-clock drops.
4. Set `sample-time` on the slow benches (engine/eval/mccfr) to ≥ 10 s in their
   `bench_function` groups to silence the criterion truncation warnings.
5. Acceptance: full `cargo nextest run` wall-clock improves; the three new benches exist
   and print; verify gate table includes them.

## B11 — Slumbot client polish

**Files:** `crates/cham-eval/src/slumbot*.rs` (locate via `rg -n "ureq|api/" crates/cham-eval/src`),
`crates/cham-cli/src/cmd/slumbot.rs`.

1. Reuse one `ureq::Agent` (keep-alive) across all requests instead of per-call agents.
2. While awaiting the opponent's response, no computation is possible (protocol is
   request/response) — instead: flush recorder buffers and update tracker bookkeeping
   immediately after OUR action is sent, so the response handler path stays minimal.
3. Acceptance: `slumbot_dialect_verified`, `slumbot_mock_flow`, `slumbot_rate_limit_retry`
   stay green; `--real` still refuses without the consent flags (behavior unchanged).

## Execution order

B1 (nothing is meaningful without it) → B10.3 (fast tests speed the rest) → B2 → B3 →
B4 → B5 → B10.1/10.2 → B7 → B8 → B9 → B6 → B11.
B2/B3/B4 are independent of B5–B9; B6 stays last because it is opt-in and needs its
validation harness.

## Final verification (run once, all must be green)

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo bench --workspace
cargo run -q -p cham-cli -- verify --perf --count-infosets --proofs
cargo run -q -p cham-cli -- train-bp --mode robust && \
cargo run -q -p cham-cli -- collect && \
cargo run -q -p cham-cli -- train-router && \
cargo run -q -p cham-cli -- ladder --fast      # real hero, parallel, VR > 1, sprt-stop rows
cargo run -q -p cham-cli -- dashboard
```

Checklist: ladder wall-clock ≥ 3× faster than the sequential baseline at equal quality ·
rows show VR ×>1.0 · no row is exactly ±0.0 without a mirror-match explanation ·
`solve_rnr_400` second-iteration improvement visible · all 141+ tests green ·
README.md Status section updated with the new numbers and the ladder guardrail behavior.

Commit per task, message format:
`perf-b<B<N>>: <one-line> — <before>→<after> on <metric>`.

## Expected impact map

| Task | Area | Expected gain | Quality guarantee |
|------|------|---------------|-------------------|
| B1   | eval correctness | strength numbers become real | artifact refusal + fallback warning |
| B2   | ladder throughput | ~4–8× wall-clock | identical seeds → identical results |
| B3   | ladder seatings  | 2–5× fewer hands on decided matchups | fixed α/β SPRT |
| B4   | statistical efficiency | 2–5× variance reduction → same CI at fewer seatings | unbiased estimator (vr.rs) |
| B5   | river search | ≥1.3× per solve on repeats | pure-function memo + bit-exact test |
| B6   | river search | extra convergence, opt-in | oracle validation gate, default off |
| B7   | startup/RAM | faster load, lower RAM | bit-identical golden replay |
| B8   | RAM safety | no OOM surprises | explicit refusal + test |
| B9   | preprocessing | ≥5× on re-clustering | byte-identical buckets |
| B10  | dev loop + gates | faster tests; latency becomes measurable | numerics level-insensitive |
| B11  | live client | small, free | protocol unchanged, tests green |
