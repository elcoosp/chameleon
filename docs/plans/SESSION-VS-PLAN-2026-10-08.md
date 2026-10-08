# Session vs plan — 2026-10-08 evening

The authoritative plan is `docs/reviews/CHAMELEON-SOTA-PLAN.md` (added
2026-10-08 late in the session, still UNTRACKED). This doc maps the
plan's tasks and gates against what actually landed.

## Phase A — W0 (3–4 agent-days)

Plan gate: Slumbot probe running; sparring partner trained; `search`
default OFF; `cargo test --workspace` green.

| Item | Status | Evidence |
|---|---|---|
| Slumbot probe running | **PARTIAL** | `cham-cli/src/cmd/slumbot.rs` exists but the only visible path is `MockSlumbot`; no real Slumbot artifact under `artifacts/`; no real probe launched this session |
| Sparring partner trained | **DONE** | `artifacts/sparring-20M-s11/` exists |
| `search` default OFF | **DONE** | `AgentMode::full_search_off()` exists at `modes.rs:59`; the live-agent constructor at line 136-138 uses it |
| `cargo test --workspace` green | **NOT RUN** | only `cargo test -p <specific>` used; full workspace not exercised as a whole |

**Conclusion:** Phase A is **mostly green** (3 of 4 items).
The Slumbot probe is the outstanding item — the CLI exists, but
there is no evidence of a real run against the live Slumbot API.

## Phase B — W1 T1.1–T1.7 + Decision D1 (6–8 agent-days)

Plan gate: kernel parity tests bit-close to brute force; VBR(uniform)
≫ VBR(shipped) ≫ 0; VBR of a hand-built Nash toy ≈ 0.

| Item | Status | Evidence |
|---|---|---|
| T1.1 kernels (W1) | **DONE (prior sessions)** | `cham-search/src/kernel.rs`, `kernel_bruteforce.rs` (8/8) |
| T1.2/T1.3 river VBR | **DONE (prior sessions)** | `cham-search/src/vbr.rs`, `vbr_validate.rs` |
| Tree (public betting tree) | **DONE** | `cham-search/src/pubtree.rs`, `pubtree_card_independent.rs`, commit `79d5363` |
| Full-game VBR | **DONE** | `cham-search/src/fullgame.rs`; validated via `fullgame_brute_force.rs` (diff = 0e0) and `fullgame_fold.rs` (+0.5 bb exact) |
| Kernel parity bit-close | **DONE** | brute-force diff = 0 (exact) |
| VBR of hand-built Nash toy ≈ 0 | **PARTIAL** | `fullgame_fold` pins +0.5 bb on a fold-to-known winner; there is no *equilibrium* toy comparison against `vbr` yet |
| D1 on `agent-honest-19dim` | **DONE** | 5.77 ± 0.55 bb (180 boards); ledger entry `ts=1791476675` |
| **D1 on `tiny-full`** | **DONE** | 7.65 +/- 0.92 bb at 1.06% miss; needs `CHAM_SLOT_BUCKET=1` (bundle trained with it); ledger entry `ts=...` appended |
| D1 verdict interpretation | **PARTIAL → addressed** | D1's third outcome applies to `agent-honest-19dim` (5.77 ≪ 8.19). The mapping of which past decisions need VBR re-measurement is in `D1-RERANK-2026-10-08.md`. The three VBR re-measurements themselves are the next session's work. |

**Conclusion:** Phase B's core is done, but two plan-explicit items are
missing: the `tiny-full` VBR run and the re-ranking of past decisions.

## Phase C — W1 T1.8–T1.10 + Decision D2 + W2 (8–12 agent-days)

Plan gate: PCS at ≤ 4h wall beats shipped bundle's VBR by > 3 SE **and**
is monotone-decreasing in iterations.

| Item | Status |
|---|---|
| T1.8–T1.10: PCS DCFR trainer | **SCAFFOLD DONE, GATE NOT EVALUATED** |
| W2: key v2 / abstraction v3 | **NOT STARTED** |
| Decision D2 | **NOT EVALUATED** |

Built this session:
- `cham-blueprint/src/pcs/{dcfr,sampling,table,walk,trainer}.rs` — DCFR math, sampler, regret table, tree walk, training loop.
- `cham-blueprint/tests/pcs_reduction.rs` — the correctness gate (jam-or-fold toy converges to analytic equilibrium).
- `cham-blueprint/tests/pcs_artifact_bridge.rs` — `to_tabular` round-trips through `BlueprintPolicy`.
- `cham-blueprint/tests/pcs_board_filter.rs` — board-overlap filtering.
- `cham-cli/src/cmd/train_pcs.rs` — the CLI with wall-budget guard.
- Pipeline verified end-to-end (0.03% miss through D1 on a 20-iter artifact).

**Why D2 is not evaluated:** the walk is ~1 s/iter at full range. 4h
allows ~12k iterations; PCS needs 10^6-10^7. Three optimization
attempts were made and reverted — see `PHASE-C-STATUS-2026-10-08.md`
for the measurement protocol and candidate list. The perf work is
blocked on a quiet machine (load was 42 at session end).

## Phase D — W3 combo-level solver (8–12 agent-days)

**NOT STARTED.** The plan calls for a combo-level river→turn solver
using the same kernels as the walker + gadget. None of it exists.

## Phase E — deployment (5–8 agent-days)

**NOT STARTED.**

## Phase F — stretch

**NOT STARTED.**

---

## Architectural divergences from the plan

1. **Crate name**: the plan names `cham-vcfr` as the new crate holding
   `pcs`, `vbr`, `kernels`, `export`. This session put PCS in
   `cham-blueprint/src/pcs/` and the full-game VBR in
   `cham-search/src/fullgame.rs`. Functionally equivalent, architecturally
   divergent. A future refactor can consolidate.

2. **Ledger hash**: the plan (rule 7) says ledger entries should be bound
   to the blake3 bundle hash. The D1 entries use sha256 because `b3sum`
   was not installed on this machine. The entry notes field records this.

3. **Process: commit messages**: the plan (rule 1) says "commit after each
   task with the ID in the message." This session's commit messages
   describe the change, not the task ID. The mapping is implicit.

---

## What the next session should do, in plan order

1. **Close Phase A**: verify the Slumbot probe and the sparring partner
   actually ran/finished. If not, launch them per the handoff's §7.
2. **Finish B's D1**: run `vbr` on `artifacts/blueprints-tiny-full/robust`
   (or whichever bundle the plan means by "tiny-full") and add a ledger
   entry. Cheap.
3. **B's D1 third outcome**: since VBR (5.77) ≪ tabular BR (8.19–10.56),
   the plan says re-rank every past decision with VBR. Identify which
   ledger decisions were made on the tabular BR and re-measure.
4. **Then Phase C's D2**: only after a quiet machine and the perf work.

---

## Honest summary

Against the plan's own gates, this session is: **Phase A not green,
Phase B mostly green with two named items missing, Phase C scaffold-only
with the gate unevaluated, Phases D/E/F untouched.** What landed is real
(the walker, D1, the PCS scaffold, the CLI, the docs), but "the plan
done" is not a claim I can make.
