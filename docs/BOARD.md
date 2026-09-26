# Project Board — canonical status view

Open this first. One-line-per-item status, updated at end of each session.
Detail lives in the linked docs; this page is the index.

Legend: **[DONE]** shipped · **[WIP]** actively in flight · **[TODO]** next · **[BLOCKED]** waiting · **[SKIP]** deliberately not doing · **[DEAD]** investigated and dropped

---

## IN PROGRESS

- **[WIP]** Full-abstraction bucket build (`artifacts/buckets-full/turn.bin`, turn phase).
- **[WIP]** Full-abstraction agent queue (`scripts/run-full-agent.sh`) — fires when buckets land.

---

## TODO — next candidates

- Full-abstraction ladder — the real "competitive?" answer. Fires automatically.
- Re-run A/B chains at correct iters (10k / 100k / 1M). Driver ready in `scripts/overnight-2026-09-25.sh`.
- Slumbot 20k-seating baseline — external anchor (`SPECS/08 §3`, `SPECS/10 §2`).
- Wire turn EHS into the runtime (v3 prerequisite; G2.x SKIP stands for v2).
- `docs/backlog/perf.md` items marked **[TODO]**.

---

## DONE (this cycle)

### GPU track (G0.0–G4.0)
- **[DONE]** Metal eval7 kernel — bit-exact on 1M hands; `verify --gpu` P7 green.
- **[DONE]** wgpu 30 cross-platform backend (Amendment 002 target).
- **[DONE]** Turn EHS table — 270,725 boards, blake3 `a1e260fb…`.
- **[DONE]** Flop EHS table — 22,100 boards, blake3 `60b23b58…`.
- **[DONE]** `verify --gpu` — P7 + P8 + P9 + bucket structural checks.
- **[DONE]** G4.0 doc closure (README, SPECS/00, worklog).
- **[SKIP]** G1.4 river EHS (no consumer, no v3 anchor).
- **[SKIP]** G2.1 / G2.2 (no pure-acceleration consumer; AIVAT stage unimplemented).

### V2 Phase 1 — Hygiene pack (partially landed)
- **[DONE]** 1.1 Lanctot Alg-3 strat_sum audit — no reach factor leaked.
- **[DONE]** 1.4 unsafe-scope doc — policy.rs, SPECS/00 §3.5 + §11 corrected.
- **[DONE]** 1.2 mutants gate — 66 tested, 40 missed. Triage in
  `docs/reports/mutants-traversal-20260926.md`. Top gap: SnapBatchSink
  write path has zero direct tests.
- **[DONE]** 1.5 license notes — no postflop-solver content in tree; standing rule recorded.
- **[DONE]** 1.2.1 mutants gap close — SnapBatchSink direct unit tests in
  `crates/cham-blueprint/tests/snapbatch.rs`; parity test now structural-only
  (CFR+ flooring is not associative across a batched flush — see the test's
  doc comment for the counterexample).
- **[DONE]** 1.3 ExploitBayes memory probe — `crates/cham-blueprint/tests/memory.rs`;
  tiny-scale measured 18 B/infoset, 58 KB snapshot, RSS delta 4.4 MB; assert < 512 MB holds.

### V4 fallback-measurement closure (v4-experiment-brainstorm Tier 0)
- **[DONE]** EXP-012 R3 decision-path fallback telemetry (`pipeline.rs` routing arms; `robust-only` 13.1%→6.0%, `full` unchanged path).
- **[DONE]** EXP-013 R2 skip-not-substitute + renormalize (`AgentMode.fallback_mode`, `CHAM_FALLBACK_MODE=substitute` legacy path; `full` 17.9%→2.4%, Cause B gone, miss-detection identical; unit tests green).
- **[DONE]** EXP-014 R1 curriculum rotation declared (`config/training/rotation-widened.toml`); full retrain + widened-vs-baseline A/B still open (multi-day).
- **[DONE]** EXP-015 router-manipulation sweep hooks (`self-exploit --switch-at/--router-temp/--router-n0`, `build_chameleon_with_router`); 60-cell grid still to run.
- **[DONE]** EXP-016 shadow snapshot/gauntlet CLI (`shadow snapshot/gauntlet`, prune-at-5); gate wiring on `--promote` still open.
- **[DONE]** EXP-017 bucket audit (`cham-engine::audit`, `audit-buckets`); EMD rebuild + exploitability-vs-audit comparison still open.
- **[DONE]** EXP-018 meta-solve (`meta-solve`, `stats::build_payoff_matrix`); ledger currently has 0 covered mode-pair cells — honest empty report, re-run once clean A/B rows land.
- **[DONE]** EXP-019 preflop cold-vs-warm bench (`cham-eval/benches/preflop_equity.rs`); measurement + GPU-table keep/kill decision still open.

### Eval / training pipeline
- **[DONE]** `matcheng` `on_public_action` wiring — 65% → 0% fallback.
- **[DONE]** `train-bp` hash parity (reads same TOML the loader does).
- **[DONE]** `train-bp` `--config` / `--buckets` / `--thread-mode` / `--regret-discount` flags.
- **[DONE]** Parallel `enumerate_orbits` + kmeans++ init (unblocked full-abstraction).
- **[DONE]** Tiny agent end-to-end ladder with real numbers (was ±0.0 stub).

### Search / cache
- **[DONE]** B-2 river subgame cache persistence (`play` hydrates/saves; VERSION v2).
- **[DONE]** B-5 DCFR positive-regret discount (opt-in `--regret-discount`).

### Dependency hygiene
- **[DONE]** bincode → postcard (RUSTSEC-2025-0141), all artifacts migrated.
- **[DONE]** History purge: 88 MB `histo-cache` removed from all commits (402 commits rewritten).

### Tooling
- **[DONE]** `scripts/status.sh` — one-shot dashboard.
- **[DONE]** `verify --gpu` extended with `check_bucket_artifacts`.

---

## SKIP / DEAD

- **[SKIP]** G1.4 river EHS — no consumer.
- **[SKIP]** G2.1 turn-machinery consumer — no runtime path computes EHS.
- **[SKIP]** G2.2 AIVAT enumeration — stage not implemented.
- **[DEAD]** B-1 multiset-rank eval — measured 3× slower than baseline.
- **[DEAD]** B-3 Hogwild hot-node accumulators — trainer loop is serial, no contention.
- **[DEAD]** B-4 Snapbatch deterministic drain — same reason as B-3.
- **[TODO]** B-6 EMD clustering — v3 scope.
- **[TODO]** B-7 multi-leaf continuation — v3 scope.
- **[TODO]** B-8 Bayesian router — research, needs an A/B harness.
- **[TODO]** B-9 SwissTable RegretTable — speculative, needs bench to justify.

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
| `plans/v2-dev-plan.md` | next major phase | **[TODO]** |
| `plans/v2-roadmap.md` | v2 rationale | context |
| `plans/v3-brainstorm.md` | v3 draft | draft |
| `backlog/perf.md` | perf items | per-item tokens |
| `reports/competitiveness.md` | measurement report | snapshot |
| `reports/bench-20260925.md` | bench snapshot | snapshot |
| `reports/bench-gpu-trials.md` | bench snapshot | snapshot |
| `review/optims-claude.md` | external review notes | input |
| `../worklog.md` | chronological log | living |
