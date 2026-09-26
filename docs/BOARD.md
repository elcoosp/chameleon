# Project Board — canonical status view

Open this first. One-line-per-item status, updated at end of each session.
Detail lives in the linked docs; this page is the index.

Legend: **[DONE]** shipped · **[WIP]** actively in flight · **[TODO]** next · **[BLOCKED]** waiting · **[SKIP]** deliberately not doing · **[DEAD]** investigated and dropped

---

## IN PROGRESS

- **[WIP]** EMD bucket rebuild validation (`train-buckets --profile exact`,
  `artifacts/buckets-exact/`) — CPU validation scale (2000 flop orbits + exhaustive
  next-street histograms), feeds the unchanged `kmeans_l1` (EMD = L1 on CDFs).
  Running at end of session; baseline for the future full-orbit GPU bulk-fill.

---

## TODO — next candidates

- **EMD bucket rebuild** — after the `exact` profile validates, the full-orbit
  GPU bulk-fill (§4.1 / v3 A2, job 1). Gate: abstraction-local exploitability
  improves ≥10 % vs tiny (`benches/exploitability.rs`), else stop.
- **EXP-011** α/γ DCFR sweep (`experiments/EXP-011-dcfr-alpha-gamma.toml`) —
  bench-only on `benches/exploitability.rs`, cheap to kill.
- **EXP-018** meta-solve re-run once clean A/B rows land (currently 0 covered cells).
- **Slumbot 20k-seating baseline** — external anchor (SPECS/08 §3, SPECS/10 §2).
- **BOARD/main housekeeping** — `renovate/configure` branch, PR #1 (renovate).
- **`docs/backlog/perf.md` items** marked **[TODO]**.

---

## DONE (recent first)

### v5-deepdive-audit fixes (v6 runbook, 2026-09-26 session)
- **[DONE]** Item 1 — `docs/HANDOFF.md` P1 synced (fallback fixed this cycle).
- **[DONE]** Items 2+3 — `pipeline.rs`: reach update reuses `expert_sigma`/
  `robust_sigma` (probe before/after byte-identical), duplicate `slots()`
  dropped. `cargo test -p cham-agent` green (15 passed), clippy clean.
- **[DONE]** Item 4 — `cache.rs`: map guard scoped + dropped before `touch()`;
  crate-scoped `clippy::significant_drop_in_scrutinee` added. `cargo test
  -p cham-search` green (9+12 passed), clippy clean. Contention bench gap
  noted (no MT scenario in `trigger_cache`).
- **[DONE]** Item 5 — EXP-016 shadow gate on `--promote` (see entry below).
- **[WIP]** Item 6 — EMD full-orbit GPU bulk-fill: flop running in background
  (`artifacts/gpu-tables/flop-full`, ~3.5 h projected, `--resume`); turn
  (55M orbits) queued after. CPU validation scale (`buckets-exact`,
  `agent-exact`) already landed previously.
- **[WIP]** Item 7 — EXP-014 hi-iters (2M, 4×) full retraining in background
  (`scripts/exp-014-hi-iters.sh` → `artifacts/agent-widened-full-hi-iters`);
  ledger + verdict on completion.
- **[DONE]** Item 8 — EXP-015 60-cell grid (see entry below).
- **[SKIP]** Item 9 — O(1) LRU: Item 4's fix was sufficient. Measured
  `cache_hit_rate=0.00` at 200 deals/arm/opp (0 subgames cached) — `touch()`'s
  O(n) hit path never executes in current workloads; nothing to optimize.

### V4 fallback-measurement closure (v4-experiment-brainstorm Tier 0) — measured
- **[DONE]** EXP-012 R3 decision-path fallback telemetry (`pipeline.rs` routing
  arms report each mode's own tier miss). `robust-only` 13.1 % → 6.0 %; full unchanged.
- **[DONE]** EXP-013 R2 skip-not-substitute + renormalize
  (`AgentMode.fallback_mode`, `CHAM_FALLBACK_MODE=substitute` legacy path).
  **Dominant fix** — full-abstraction fallback 26.7 % → **3.3 %**; Cause B gone;
  miss-detection counts identical; two new unit tests green.
- **[DONE]** EXP-014 R1 widened curriculum (`config/training/rotation-widened.toml`,
  `scripts/exp-014-run.sh`). Measured (probe --diag-fallback, DIAG_DEALS=40):
  - tiny 2.4 % → **1.9 %** (`jamfix` 9→0, `pnash` 4→0; net positive)
  - full 3.3 % → **3.7 %** (same witnesses eliminated but arch-shape coverage
    regressed: `arch:station` 0→7); net negative at full scale.
  - Headline: both full baselines are now 3.3 % not 26.7 % — EXP-013 is the
    dominant fix; EXP-014's specific contribution is eliminating Cause A.
- **[DONE]** EXP-015 router-manipulation sweep hooks (`self-exploit --switch-at/
  --router-temp/--router-n0`, `build_chameleon_with_router`); 60-cell grid RUN
  (v6 runbook item 8, `docs/reports/exp-015-router-manipulation-grid.md`):
  manipulator earns +1171…+1476 everywhere, default near-optimal, no
  promotion, no ladder follow-up.
- **[DONE]** EXP-016 shadow gauntlet gate on `--promote` (v6 runbook item 5).
  `run_gauntlet` faces the candidate against the last-3 shadows in real
  duplicate matches (`MatchRunner::run`, rows via the opponents-crate shadow
  registry under each shadow's own encoder); snapshots now persist KEYED rows
  (v2) + bundle-root merge (robust + 4 experts, first-wins). Dry runs: first
  promotion skips cleanly ("no prior shadows"), auto-snapshots (15,986 rows);
  second promotion runs the gate and BLOCKED (worst -337.7 mb/seating vs -10.0
  tolerance — the live full bundle loses to its own frozen snapshot, consistent
  with the known router mis-allocation; gate is honest, not spurious).
- **[DONE]** EXP-017 bucket-quality audit + producer. `audit-buckets --generate`
  now drives a short match and writes the JSON. Baseline ratio on tiny
  (artifacts/agent, 60 deals/opponent, 294 flop+turn pairs):
  **between=0.70e7 / within=2.94e7 → ratio 0.24**. Ratio < 1 confirms the tiny
  buckets don't separate realized-EV — the EMD rebuild target.
- **[DONE]** EXP-018 meta-solve (`meta-solve`, `stats::build_payoff_matrix`);
  ledger currently has 0 covered mode-pair cells — honest empty report, re-run
  once clean A/B rows land.
- **[DONE]** EXP-019 preflop-equity cold/warm bench
  (`cham-eval/benches/preflop_equity.rs`, registered in Cargo.toml this session).
  **cold = 254.57 ms [234.97, 274.32]; warm = 16.052 ns [15.813, 16.398]**
  (~1.6e7×). Verdict: memoization alone is sufficient; offline GPU table NOT
  warranted (recurring (hero,villain) pairs hit the same key; cold-dominated
  workloads implausible at 1.7M possible pairs).

### V3 execution roadmap — implemented + partially measured
Full roadmap (docs/plans/v3-execution-roadmap.md) landed as atomic commits.
Sections: 1.1 (parallel/session-isolated `AbRunner::run_shared`), 1.2 (2048-entry
LRU cache + `warm-cache`), 2.1 (`vr_factor` first-class + preflop all-in EV),
2.2 (`LedgerEntry::artifact_hash` + `lint-ledger` + PREREG files), 2.3
(`benches/exploitability.rs`), 3.1 (DCFR α/γ split, `--avg-gamma`), 3.2 (double-hash
probing; hashbrown escalation documented), 3.3 (evaluate7 multiset-rank table),
4.1 (`BuildParams::exact`), 5.1 (leaf variants), 5.2 (Dirichlet-multinomial router
fusion with `posterior_variance()`), 6 (FrozenAgent + `self-exploit` + frozen
training mode). §3.4 (Hogwild/Snapbatch parallel-iterations) remains
not-applicable until a parallel-iterations training mode exists.

### P1 fallback diagnosis (`docs/reports/p1-fallback-diagnosis-20260926.md`)
Per-expert miss attribution added to `DecisionTrace` + `probe --diag-fallback
--bundle`. Two distinct failure classes identified (Cause A: training-reachability
gap on `jamfix`/`pnash`; Cause B: fallback-order over-report on `callbot`/`arch:station`).
That diagnostic is what made the v4 fallback numbers separable.

### V2 Phase 1 — Hygiene pack (all landed)
- **[DONE]** 1.1 Lanctot Alg-3 strat_sum audit — no reach factor leaked.
- **[DONE]** 1.2 mutants gate — 66 tested, 40 missed; triage in
  `docs/reports/mutants-traversal-20260926.md`.
- **[DONE]** 1.2.1 mutants gap close — SnapBatchSink direct unit tests
  (`crates/cham-blueprint/tests/snapbatch.rs`); parity test now structural-only
  (CFR+ flooring is not associative across a batched flush).
- **[DONE]** 1.3 ExploitBayes memory probe — `crates/cham-blueprint/tests/memory.rs`;
  18 B/infoset, 58 KB snapshot, RSS delta 4.4 MB; assert < 512 MB holds.
- **[DONE]** 1.4 unsafe-scope doc — policy.rs + SPECS/00 §3.5 + §11.
- **[DONE]** 1.5 license notes — no postflop-solver content in tree; standing rule recorded.

### GPU track (G0.0–G4.0)
- **[DONE]** Metal eval7 kernel — bit-exact on 1M hands; `verify --gpu` P7 green.
- **[DONE]** wgpu 30 cross-platform backend (Amendment 002 target).
- **[DONE]** Turn EHS table — 270,725 boards, blake3 `a1e260fb…`.
- **[DONE]** Flop EHS table — 22,100 boards, blake3 `60b23b58…`.
- **[DONE]** `verify --gpu` — P7 + P8 + P9 + bucket structural checks.
- **[DONE]** G4.0 doc closure (README, SPECS/00, worklog).
- **[SKIP]** G1.4 river EHS (no consumer, no v3 anchor).
- **[SKIP]** G2.1 / G2.2 (no pure-acceleration consumer; AIVAT stage unimplemented).

### Eval / training pipeline
- **[DONE]** `matcheng` `on_public_action` wiring — 65 % → 0 % fallback.
- **[DONE]** `train-bp` hash parity, `--config/--buckets/--thread-mode/--regret-discount/--avg-gamma` flags.
- **[DONE]** `train-bp --resume` now wired through to `train_with_threads`
  (warm-start semantics; true resume-at-N is a separate change).
- **[DONE]** Parallel `enumerate_orbits` + kmeans++ init (unblocked full-abstraction).
- **[DONE]** Full-abstraction bucket build (`artifacts/buckets-full/`, turn 139 MB).
- **[DONE]** Full-abstraction agent + first ladder (26.7 % fallback diagnosed + fixed).

### Search / cache
- **[DONE]** B-2 river subgame cache persistence (`play`/`ladder`/`ab`).
- **[DONE]** B-5 DCFR positive-regret discount.

### Dependency hygiene
- **[DONE]** bincode → postcard (RUSTSEC-2025-0141), all artifacts migrated.
- **[DONE]** History purge: 88 MB `histo-cache` removed from all commits.

### Tooling
- **[DONE]** `scripts/status.sh`, `scripts/exp-014-run.sh`, `scripts/run-full-agent.sh`.
- **[DONE]** `verify --gpu` extended with `check_bucket_artifacts`.
- **[DONE]** `probe --diag-fallback --bundle`.

---

## SKIP / DEAD

- **[SKIP]** G1.4 river EHS — no consumer.
- **[SKIP]** G2.1 turn-machinery consumer — no runtime path computes turn EHS.
- **[SKIP]** G2.2 AIVAT enumeration — stage not implemented.
- **[DEAD]** B-1 multiset-rank eval as a *recompute* — measured 3× slower; the
  LOOKUP-table variant (v3 §3.3) shipped and is used.
- **[DEAD]** B-3 Hogwild hot-node accumulators — trainer loop is serial, no contention.
- **[DEAD]** B-4 Snapbatch deterministic drain — same reason.
- **[TODO]** B-6 EMD clustering — addressed by v3 §4.1 `BuildParams::exact`; the
  GPU bulk-fill is the remaining step.
- **[TODO]** B-7 multi-leaf continuation — landed as v3 §5.1 leaf variants.
- **[TODO]** B-8 Bayesian router — landed as v3 §5.2 Dirichlet fusion.
- **[TODO]** B-9 SwissTable RegretTable — hashbrown not whitelisted; double-hash
  probing shipped instead (v3 §3.2). Documented escalation behind the mccfr kill gate.

---

## Blockers

None.

---

## Doc index

| Doc | Purpose | Status header |
|---|---|---|
| `SPECS/*.md` | reference (frozen) | — |
| `gpu/plan.md` | GPU track runbook | **[DONE]** |
| `gpu/amendments.md` | amendments 001/002 | **[DONE]** |
| `gpu/g3-verify-design.md` | verify --gpu design | **[DONE]** |
| `gpu/g5-wgpu-design.md` | wgpu port design | **[DONE]** |
| `plans/overnight-2026-09-25.md` | overnight runbook | **[DONE]** |
| `plans/v2-dev-plan.md` | v2 dev runbook | superseded |
| `plans/v2-roadmap.md` | v2 rationale | historical |
| `plans/v3-brainstorm.md` | v3 draft | inputs absorbed |
| `plans/v3-execution-roadmap.md` | v3 execution roadmap | **[DONE]** (12 sections; §3.4 not-applicable) |
| `plans/v4-experiment-brainstorm.md` | v4 experiment registry | partial (EXP-012..019; 012/013/014/017/019 measured) |
| `plans/aivat-baseline-spec.md` | AIVAT full-stage spec | spec-only |
| `plans/bet-size-audit.md` | bet-size audit plan | spec-only |
| `plans/gpu-jobs-v3.md` | GPU job queue | per-job |
| `backlog/perf.md` | perf items | per-item tokens |
| `reports/competitiveness.md` | measurement report | snapshot |
| `reports/bench-20260925.md` | bench snapshot | snapshot |
| `reports/bench-gpu-trials.md` | bench snapshot | snapshot |
| `reports/mutants-traversal-20260926.md` | mutants triage | snapshot |
| `reports/p1-fallback-diagnosis-20260926.md` | P1 fallback attribution | snapshot |
| `review/optims-claude.md` | external review notes | input |
| `../worklog.md` | chronological log | living |
