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
- `docs/PERF-BACKLOG.md` items marked **[TODO]**.

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
| `GPU-PLAN.md` | GPU track runbook | **[DONE]** |
| `GPU-PLAN-AMENDMENTS.md` | amendments 001/002 | **[DONE]** |
| `GPU-G3.0-VERIFY-GPU-DESIGN.md` | design | **[DONE]** |
| `GPU-G5.0-WGPU-PORT-DESIGN.md` | design | **[DONE]** |
| `OVERNIGHT-PLAN-2026-09-25.md` | overnight runbook | **[DONE]** |
| `PERF-BACKLOG.md` | perf items | per-item tokens |
| `V2-DEV-PLAN.md` | next major phase | **[TODO]** |
| `CHAMELEON-v2-ROADMAP.md` | v2 rationale | context |
| `V3-BRAINSTORM.md` | v3 draft | draft |
| `COMPETITIVENESS-AND-SPEED-REPORT.md` | measurement report | snapshot |
| `bench-status-*.md` | measurement snapshots | snapshot |
| `optims-claude.md` | external review notes | input |
| `worklog.md` | chronological log | living |
