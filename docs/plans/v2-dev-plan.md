# V2-DEV-PLAN — Agent Runbook (supersedes QUALITY-PLAN.md as the operational doc)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


> **Status:** [TODO] — next major phase — not started; prerequisites complete (see BOARD.md)

You are a coding agent implementing the merged v2 quality roadmap (source documents in
this repo: `CHAMELEON-v2-ROADMAP.md` = rationale, `QUALITY-PLAN.md` = reconciliation).
This file is your **only** operational reference. Execute phases in order. Never skip a
gate. Never weaken a test, threshold, or alpha/beta to pass a gate.

Companion perf plans (`PERF-PLAN.md`, `Broad-perf-plan.md`) keep their own task lists;
this plan **depends on** Broad-plan B1 (real hero agent in ladder) — if not done, do it first.

---

## PART I — THE DEV LOOP (non-negotiable, run after EVERY task)

```text
1. PRE-FLIGHT (once per session)
   git status                      # clean tree, on a work branch per phase
   cargo build --workspace

2. QUALITY GATE (always)
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings      # 0 warnings
   cargo nextest run --workspace                              # 141+ passed, 0 failed

3. QUALITY EXTRA (only if the task table says PROOFS or DETERMINISM)
   PROOFS:      cargo run -q -p cham-cli -- verify --proofs   # P-1..P-4 GREEN
   DETERMINISM: run the task's named replay/determinism test explicitly

4. PERF GATE (always)
   cargo bench --workspace > bench-after-<task-id>.txt
   Compare each benchmark against the previous passing run:
     FAIL if any benchmark regresses > 5% (criterion "Performance has regressed"
     with p < 0.05), UNLESS the task explicitly budgets the regression
     (only Phase 5 budgets trainer slowdown; nothing else ever does).

5. INVARIANT GATE (always)
   cargo run -q -p cham-cli -- verify --perf --count-infosets --proofs   # GREEN

6. EVAL GATE (only tasks tagged [POLICY-AFFECTING])
   A policy-affecting change ships ONLY through the Phase 3 promotion ratchet:
   retrain artifacts → screening ladder vs pool + last-3 shadows → ab paired-SPRT
   → ledger entry with CI beating the incumbent. No CI win, no default change.

7. COMMIT + LOG
   git commit -m "<type>(<phase.task>): <one-line>"
   types: correctness | measure | engineering | research | govern
   Append one section to worklog.md (what, gates run, numbers observed).

FAILURE RULE: a gate fails → fix and re-run. Two consecutive failures on the same
gate → STOP, append "BLOCKED: <reason>" to worklog.md, ask the human. Do not
attempt a third workaround.
```

**Hard rules for ALL code you write (from SPECS/00):**
- No `unsafe`, no nightly features, no new crates beyond the existing whitelist
  (`arrayvec, half, bytemuck, blake3, serde, thiserror, anyhow, rand, proptest,
  criterion, insta, toml, rustc-hash, ureq, walkdir`).
- No RNG-stream changes; new randomness derives from `cham_core::rng` seeded paths.
- No wall-clock in eval paths (`budget.rs` logic is the only sanctioned clock, live-mode only).
- No infoset-key composition changes; if the encoder changes, `abstraction_hash` bumps
  and artifacts regenerate — key tests get UPDATED, never loosened or deleted.
- Artifact formats: additive schema changes only; bump `ARTIFACT_VERSION` when doing so.

---

## PART II — TASK TABLE

| Phase.Task | Name | Tag | Extra gates |
|------------|------|-----|-------------|
| 1.1 | Lanctot Alg-3 strat_sum audit | [CORRECTNESS] | PROOFS |
| 1.2 | mutants gate on traversal.rs | [CORRECTNESS] | — |
| 1.3 | ExploitBayes memory probe | [CORRECTNESS] | — |
| 1.4 | memmap2/unsafe doc scoping | [CORRECTNESS] | — |
| 1.5 | AGPL license notes | [CORRECTNESS] | — |
| 2.1 | Cold-start mixture LBR in probe | [MEASUREMENT] | DETERMINISM |
| 3.1 | Frozen-snapshot opponent wrapper | [ENGINEERING] | DETERMINISM |
| 3.2 | Shadow ladder registration | [ENGINEERING] | — |
| 3.3 | Promotion ratchet in ab.rs | [GOVERNANCE] | — |
| 4.1 | Bucket realized-EV audit | [MEASUREMENT] | — |
| 5.1 | Histogram cache (Broad B9) | [ENGINEERING] | PERF budget: none |
| 5.2 | Full-profile buckets + retrain | [ENGINEERING][POLICY-AFFECTING] | EVAL |
| 5.3 | Denser histograms | [ENGINEERING][POLICY-AFFECTING] | EVAL |
| 5.4 | Turn texture flags | [ENGINEERING][POLICY-AFFECTING] | EVAL + DETERMINISM |
| 5.5 | Finer-grid LBR param | [MEASUREMENT] | — |
| 5.6 | Preflop granularity → EXP-015 only | [GOVERNANCE] | — |
| 6.1 | Self-Exploit Audit (cold+adaptive) | [MEASUREMENT] | DETERMINISM |
| 7.1 | Cross-abstraction ensemble shield | [ENGINEERING][POLICY-AFFECTING] | EVAL + DETERMINISM |
| 8.1 | Meta-strategy solve (zoo Nash) | [MEASUREMENT] | — |
| 9.1 | Slumbot dashboard target | [MEASUREMENT] | — |
| 10.1 | EXP-009 CCS-MCCFR | [RESEARCH] | PROOFS |
| 10.2 | EXP-010 CS-RNR | [RESEARCH] | PROOFS |
| 11.1 | Register EXP-012..019 | [GOVERNANCE] | — |

---

## PART III — PHASES

### Phase 1 — Hygiene pack (do before trusting any v2 number)

**1.1 Lanctot Alg-3 audit** — Files: new `crates/cham-blueprint/tests/lanctot_alg3_audit.rs`,
`crates/cham-blueprint/src/traversal.rs`, `SPECS/04-*` (if variant mismatch).
1. Read `traversal.rs` regret + strat_sum updates. The claim to verify: regret
   `r[a] += v[a] − v̄` with NO reach factor (correct external-sampling MCCFR), strat_sum
   accumulated with NO `reach_hero` term (Lanctot Alg. 3 variant).
2. Implement an independent, direct Alg-3 simulator in the test file (small Kuhn-scale
   tree, exhaustive over chance deals, own local state — do NOT import traversal code).
   Assert per-infoset strategy sums equal within fixed-point epsilon after N=1000
   iterations on a fixed seed.
3. If they DISAGREE: the audit is right, the code or the spec is wrong. Fix the code to
   match the intended variant, update SPECS/04 wording, add a row to
   REVIEW-RESOLUTIONS.md. Re-run PROOFS (P-1 Kuhn value must still hold).
Acceptance: test exists, passes (or fix committed); PROOFS green.

**1.2 Mutants gate** — Files: `justfile`.
`mutants-gate:` recipe = `cargo mutants -p cham-blueprint --file traversal.rs`.
Run it once; surviving mutants list goes in the commit message. Acceptance: target exists,
runs, result recorded.

**1.3 ExploitBayes memory probe** — Files: locate ExploitBayes via
`rg -n "ExploitBayes" crates/`, new test beside it.
Instantiate at tiny-abstraction scale; print bytes/infoset + total; assert
`total < 512 MB`. If it exceeds: record the number, tag the test `#[ignore]` with the
measured value in the message, and note "EXP-blocked" — do NOT redesign it now.
Acceptance: test green (or documented `#[ignore]` with number).

**1.4 Unsafe-scope doc** — Files: `crates/cham-blueprint/src/policy.rs` module docs,
`README.md`.
Rewrite the "zero unsafe" claim precisely: `#![forbid(unsafe_code)]` covers this
workspace's crates; dependencies may contain unsafe at the syscall boundary (memmap2
example); mitigation = hash-verify artifact, read-only mapping, never mutate. Also fix
policy.rs's D-008 comment (it says mmap ruled out — keep that decision, but the *reason
documented* must be the scoped statement above). Acceptance: docs read correctly; no code
change.

**1.5 AGPL notes** — Files: new `LICENSE-NOTES.md`.
Read postflop-solver's LICENSE (web or vendored copy). Record: license id, whether
committed derived numeric fixtures are permitted, required attribution. If the answer is
"not permitted", open `BLOCKED` note instead of deleting fixtures — human decides.

### Phase 2 — Cold-start mixture LBR (roadmap §1.2)

**2.1** — Files: `crates/cham-cli/src/cmd/probe.rs`, `crates/cham-blueprint/src/lbr.rs`
(`lbr_vs` exists), `crates/cham-eval/src/ledger.rs` (entry schema).
1. Build the cold-start policy: the `full` agent's mixture with tracker at `hands_seen=0`
   and session-start weights (same code path probe uses for coverage — read probe.rs
   first; extract the policy-evaluation closure, do not duplicate logic).
2. Run `lbr_vs` against it; print `cold_lbr: <N> mb/hand` next to the existing proxy LBR.
3. Add `cold_lbr_mb: Option<f64>` to the ledger entry (additive, old entries parse).
4. Determinism test: fixed fixture agent → pinned cold_lbr value.
Acceptance: probe prints both numbers; ledger roundtrip test updated; DETERMINISM green.

### Phase 3 — Shadow ladder + promotion ratchet (roadmap §3.1; DEPENDS: Broad B1)

**3.1 Frozen-snapshot wrapper** — Files: new `crates/cham-eval/src/frozen.rs`,
`crates/cham-opponents/src/factory.rs`.
1. Implement `FrozenAgent`: loads an artifact bundle via `cham_agent::loader::load_agent`,
   exposes `action_probs` as the mixture with weights frozen at a documented point
   (cold-start for screening; the adaptive variant is Phase 6's job). Implements
   `cham_core::obs::Agent`.
2. New `OpponentSpec::BotSnapshot { hash8 }` variant parsing `bot:shadow-<hash8>`,
   resolving to `artifacts/shadows/<hash8>/`. Factory returns `FrozenAgent`.
3. Test: snapshot roundtrip + deterministic `action_probs` for fixed infoset.
Acceptance: tests green; DETERMINISM.

**3.2 Shadow registration** — Files: `crates/cham-cli/src/cmd/ab.rs`, new
`artifacts/shadows/` layout, `crates/cham-cli/src/cmd/ladder.rs`.
1. On every promotion, `ab.rs` copies the promoted bundle to `artifacts/shadows/<hash8>/`
   (hash8 = first 8 hex of blake3 over bundle bytes) and appends to
   `artifacts/shadows/registry.toml` (path + date + ledger ref).
2. `ladder --shadows N` (default 3): prepend the registry's last N shadows to the pool.
   Shadow rows print with the `shadow:` prefix.
Acceptance: ladder lists shadow rows; registry roundtrip test; works when 0 shadows
exist (flag degrades to pool-only).

**3.3 Promotion ratchet** — Files: `crates/cham-cli/src/cmd/ab.rs`.
A candidate is promotable iff BOTH: (a) screening-ladder pooled CI lower bound exceeds
the incumbent's, AND (b) paired-SPRT vs EACH of the last 3 shadows does not decline
(alpha=beta=0.05, thresholds from `config/pool.toml`). Refuse promotion otherwise, exit
nonzero, ledger entry `verdict=declined` with reasons. Test `promotion_requires_shadow_win`
with fixture ledgers. Acceptance: refusal path tested; happy path tested.

### Phase 4 — Bucket realized-EV audit (roadmap §3.4)

**4.1** — Files: new `crates/cham-engine/tests/bucket_ev_audit.rs`, bucket lookup API in
`crates/cham-engine/src/` (locate via `rg -n "pub fn bucket" crates/cham-engine/src`).
1. From a fixed 50k-seat fixture replay (or synthetic generator, seeded): group river
   hands by bucket; compute within-bucket vs between-bucket variance of realized
   showdown EV; metric R = VarB/(VarB+VarW).
2. Assert R > 0.5 on tiny buckets (calibrate once from the fixture, pin the constant,
   document how it was chosen).
3. Long variant `#[ignore]` for full profiles; print R in `verify --count-infosets`.
Acceptance: test green; verify prints R.

### Phase 5 — Abstraction upgrade (engineering + policy-affecting)

**5.1 Histogram cache** (= Broad-perf-plan B9; do first) — cache per-board equity
histograms to `artifacts/histo-cache/<blake3>.bin` keyed by (canonical board, range
config, histogram params — NOT k). Second `train-buckets` with different k must hit
cache; buckets byte-identical (`kmeans_emd_determinism` + new equality test); ≥5× faster
re-run. No perf budget changes.

**5.2 Full-profile run** — `train-buckets --profile full` (k=300/200) → retrain ALL
specialists + robust + router (PERF budget: trainer wall-clock MAY regress here; record
before/after). Promote ONLY via Phase 3 ratchet. Ledger records tiny→full delta.

**5.3 Denser histograms** — raise MC sample count for the full profile in
`config/abstraction.toml`; exact enumeration where the river allows. Same EVAL gate.

**5.4 Turn texture flags** — additive bits in the turn observation for flush/pair
interaction. `abstraction_hash` bumps; artifacts regenerate; key tests UPDATED with the
new composition documented in SPECS/02. Never silent. DETERMINISM + EVAL gates.

**5.5 Finer-grid LBR** — `lbr_vs` gains a grid constructor param (double bet-size
granularity, uncapped raises). Default grid unchanged. Run once per specialist at M4;
report both grids. No policy change.

**5.6** Preflop granularity: REGISTER as EXP-015 (Phase 11) — do NOT implement.

### Phase 6 — M6 Self-Exploit Audit (roadmap §1.1)

**6.1** — Files: new `crates/cham-eval/src/audit.rs`, new `audit` CLI subcommand in
`crates/cham-cli/src/cmd/mod.rs` + `main.rs`, reuse Phase 3's `FrozenAgent`.
1. Cold-start variant: point the existing `Exploit`-mode trainer at the frozen cold-start
   `full` agent (`TrainMode::Exploit { opponent: FrozenSnapshot, jitter_seed: 0 }`).
   Validate convergence on Kuhn via the proof harness first (30k iters), then production.
2. Adaptive variant: the wrapped opponent runs the REAL tracker+router across hands
   (weights still frozen per-hand); the exploiter may manipulate the classifier.
3. Output: `G-SELF` = exploit winrate vs `full`, mb/hand with session-clustered CI,
   ledger entry, dashboard row. Diagnostic only — NEVER gates promotion.
4. Determinism: same seed → same G-SELF.
Acceptance: `chameleon audit --variant cold-start|adaptive` works end-to-end; README
G-SELF section; DETERMINISM green.

### Phase 7 — Ensemble-disagreement shield (roadmap §3.2)

**7.1** — Files: `crates/cham-blueprint/src/` (second-abstraction training entry),
`crates/cham-agent/src/pipeline.rs` (shield), `config/agents/full.toml`.
1. Train the same recipe on a second tiny abstraction (k-means seed differs, k±10%).
   Ship as `expert_alt` in the agent bundle.
2. Pipeline: when primary confidence < visit-counter threshold, also query alt;
   disagreement = total-variation distance between σ vectors; `shield_beta` grows with
   it; above a documented bound → conservative fallback (existing path).
3. New test extends `pipeline_mode_matrix`; deterministic replay pinned.
4. EVAL gate: full+ensemble vs full on screening ladder; promote only via ratchet.
Acceptance: all four gates green.

### Phase 8 — Meta-strategy solve (roadmap §3.3)

**8.1** — Files: `crates/cham-eval/src/stats.rs` (new `empirical_nash`),
`crates/cham-cli/src/cmd/dashboard.rs`.
1. Build the 8×8 pairwise payoff matrix from ledger A/B history (symmetrized
   `(EV_ij − EV_ji)/2`; missing pairs excluded from the support).
2. Fictitious play, 500 iterations (gap bound 2/√t documented); no LP dependency.
3. Dashboard section: Nash mixture over modes; flag if deployed `full` is outside support.
4. Test: known 2×2 mixed equilibrium reproduced.
Acceptance: test green; dashboard renders from fixture ledger.

### Phase 9 — Slumbot aspirational target (roadmap §1.4)

**9.1** — Files: `crates/cham-eval/src/dashboard.rs`, `crates/cham-cli/src/cmd/slumbot.rs`.
Dashboard section "Slumbot bar": latest real-anchor vs target "CI excludes negative at
20k seatings" (pass/pending/never-run). Non-blocking forever. Runbook paragraph in
README (login → 20k → ledger → dashboard). `--real` consent flags unchanged.
Acceptance: renders from fixtures; no gate logic.

### Phase 10 — Literature EXPs (roadmap §2)

**10.1 EXP-009 CCS-MCCFR** — register + implement ~100-LOC traversal variant behind
`TrainMode`; M-1 harness paired runs (Kuhn/Leduc), Holm-corrected. Expect ≈0 production
gain (roadmap's own analysis). Record verdict either way. PROOFS gate.
**10.2 EXP-010 CS-RNR** — register; extend `solve_rnr` with confidence-scheduled
restricted response + self-audited certificate. MEASURE certificate cost at
river-subgame scale vs the 250 ms live budget (paper numbers are Leduc-scale — do not
assume). Eval-mode (`Iterations`) first; live only with budget margin. PROOFS gate.
Record verdicts in `experiments/` + ledger.

### Phase 11 — EXP registrations (governance; no implementation)

**11.1** Create `experiments/EXP-012..019-*.toml` from this exact template:
```toml
id = "EXP-0NN"
name = "<snake_name>"
class = "research"                  # never ships as a default
hypothesis = "<one sentence, falsifiable>"
primary_metric = "mb_per_seating_vs_pool"
gate = "ledger CI beats incumbent baseline via promotion ratchet"
trigger = "<condition that schedules the run>"
cost = "<engineer-days estimate>"
status = "registered"
```
| EXP | name | trigger |
|-----|------|---------|
| 012 | turn_subgame_solving | G-SELF green AND cold-start river LBR attributes ≥30 mb/hand to turn-policy errors |
| 013 | ev_targeted_router_labels | full-profile buckets promoted AND router top-1 plateaued |
| 014 | beta_binomial_tracker_counters | probe shows tracker calibration gap (cov / acc_b_dev off target) |
| 015 | preflop_granularity_upgrade | bucket audit blames preflop coarseness for ≥20% of within-bucket EV variance |
| 016 | third_postflop_bet_size | finer-grid LBR (5.5) shows the size grid is binding |
| 017 | adversary_mix_weighting | ladder shows ≥2 archetypes with CI excluding 0 on the losing side |
| 018 | perturb_robust_river_solves | EXP-010 has a verdict AND adaptive G-SELF shows range-model overfit |
| 019 | controlled_adaptation_drift | all prior EXPs resolved; pre-registered A/B with shield bounds |
Also update EXP-011's definition: sweep = update-rule (RM+, alternating) × discount
schedule (current θ₀=10bb/δ=0.99/γ=0.9 vs DCFR α=1.5/β=0/γ=2), M-1 first, then screening
ladder. Acceptance: files exist, valid TOML, `status="registered"`.

---

## PART IV — FINAL VERIFICATION (after Phase 11, all must be green)

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo run -q -p cham-cli -- verify --perf --count-infosets --proofs
cargo bench --workspace                      # no unexplained regressions vs baseline
just fast                                    # real hero + parallel ladder + shadows
cargo run -q -p cham-cli -- audit --variant cold-start
cargo run -q -p cham-cli -- dashboard        # G-SELF row, Slumbot bar, Nash mixture, bucket R
ls experiments/EXP-01[2-9]-*.toml | wc -l    # = 8
```
Then update README.md Status: v2 sections shipped, G-SELF headline number, promotion
ratchet now mandatory, EXP registry 006–019 present. Commit per task:
`<type>(<phase.task>): <one-line>`. Every ledger-facing change must show its CI.
