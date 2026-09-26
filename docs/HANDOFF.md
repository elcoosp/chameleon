# Session handoff — 2026-09-26

> **Status:** current. Open this file first if you are a new agent on this repo.
> For canonical project status, see `docs/BOARD.md`.

## TL;DR — where we are

**End of a long session.** All background jobs have completed or been killed
cleanly. Repository is clean, all work is committed and pushed to
`origin/gpu/g0`. Tests green: `cargo nextest run --workspace` = 172+ passed.
Clippy green: `cargo clippy --workspace --all-targets -- -D warnings`.

**What this session landed (chronological):**

1. Engine bug fix (`min_raise_to` computed, not cached) — unblocked
   full-abstraction blueprint training which had been panicking on
   `Bet { to: 100 }`.
2. Full-abstraction EHS buckets built (turn 139 MB, flop 12.8 MB).
3. Full-abstraction blueprints trained (robust + 4 experts, 8.2 MB total).
4. First full-abstraction ladder run — real numbers (see below).
5. bincode → postcard migration complete (RUSTSEC-2025-0141).
6. Git history rewritten to purge an 88 MB `histo-cache` blob.
7. V2 Phase 1 items: 1.1 (audit), 1.2 (mutants), 1.4 (unsafe docs),
   1.5 (license notes) landed; 1.3 (memory probe) still open.
8. A/B speedups: training cache, parallel AbRunner, SPRT default,
   shared cache guard, `--reuse/--resume/--cache-dir` on train-bp.

## Handoff priorities (highest first)

### P1 — Get the full-abstraction agent to eliminate fallback

The full-abstraction ladder from this session produced **26.7% fallback**
decisions (vs 0% for the tiny abstraction the previous day). This means
the router is allocating weight poorly at the fuller scale: the mixture
collapses to something the confidence gate rejects, and the numbers
below are therefore "diagnostic only."

**What to do:** this is the V2 Phase 2 (`Cold-start mixture LBR`) or
Phase 7 (`Ensemble-disagreement shield`) job. Neither has started. The
immediate question is: which of the 4 experts is falling below the
confidence gate, and on which opponents?

**Files:**
- `crates/cham-agent/src/pipeline.rs` — the mixture + fallback path
- `crates/cham-agent/src/tracker.rs` — the feature source feeding the router
- `crates/cham-cli/src/cmd/probe.rs` — the diagnostic harness (add a
  per-expert fallback report here)

**Acceptance:** rerun `scripts/run-full-agent.sh` after a fix and see
fallback < 5% (the tiny agent achieves 0%; a full agent should not be
worse than the tiny on this metric).

### P2 — V2 Phase 1.3 (ExploitBayes memory probe)

The only V2 Phase-1 item still open. Small, self-contained.

**Files:** locate the ExploitBayes mode via
`rg -n "ExploitBayes" crates/`; add a test that instantiates it at
tiny-abstraction scale, measures bytes-per-infoset and total RSS, and
asserts `total < 512 MB`. If it exceeds: record the number, mark the
test `#[ignore]` with the measurement in the message, note "EXP-blocked."

**Acceptance:** test green (or documented `#[ignore]`).

### P3 — V2 Phase 2 through 11

`docs/plans/v2-dev-plan.md` is the operational doc. It has a task table
(Part II) and phases (Part III). Phase 2 (Cold-start mixture LBR) is the
most natural next task — it directly diagnoses the P1 fallback problem
by measuring the cold-start policy's exploitability.

**Prereqs satisfied this session:**
- Phase 3.1's "Broad B1" (real hero in ladder) — landed before this session
- Phase 3.x needs shadow registries which don't exist yet

## Known traps discovered this session

### 1. Test churn in `crates/*/artifacts/runs/` is real but not committed

PID-stamped test run dirs (`agent-test-16569-0/` etc.) are created on every
`cargo nextest run`. They are git-ignored (`crates/*/artifacts/runs/*-test-*/`).
Do NOT try to commit them.

However, some provenance files under `crates/cham-blueprint/artifacts/runs/*-test/provenance.json`
ARE tracked (they are golden fixtures) and change on every test run.
If you see them dirty after a `nextest` run: `git checkout -- crates/cham-blueprint/artifacts/`.

### 2. macOS APFS is case-insensitive

`docs/V3-BRAINSTORM.md` and `docs/v3-brainstorm.md` are the **same file**.
A `rm` of one deletes both. Watch for this on any doc renames.

### 3. `git filter-repo` removes the origin remote

By design. After any history rewrite, restore with:
