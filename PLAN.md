# PROJECT CHAMELEON — Master Plan — v2

**Codename:** CHAMELEON
**Tagline:** *Play the opponent, not the game.*
**One-liner:** A routed mixture of specialist blueprints — each cheap to train, each shaped like an opponent archetype — wrapped in a flight-recorder eval loop that turns every commit into feedback.

**Date:** 2026-09-25 (v2 after external spec review) · **Hardware:** Apple M1, 16 GB, no CUDA · **Target:** HUNL, developed at 100 bb (200 bb only for the Slumbot anchor) · **Timeline:** 5–6 weeks · **Stack:** 100% Rust.

**What v2 is:** a correctness rewrite after a hard external review. The governance layer (seed partitions, paired A/B, append-only ledger, frozen predictions, closed deps) survived intact; the poker core underneath it did not — ten issues were fatal or thesis-threatening, and they are fixed at spec level, which is where you want to fix them. Full point-by-point mapping: [`REVIEW-RESOLUTIONS.md`](REVIEW-RESOLUTIONS.md).

---

## 1. The direction (unchanged in essence, hardened in mechanism)

The pivot stands: **abandon Nash pursuit, pursue opponent-specific exploitation.** On an M1, minimizing exploitability against the whole strategy space is 2–3 orders of magnitude beyond reach; exploiting 4–8 common archetypes converges in a fraction of the compute.

**The mix:** Idea #1 (classifier + N blueprints) is the backbone; Idea #3 (river re-solve, now RNR/FMBR/reach-gadget — not anchored CFR+) is the inference layer; Idea #4 (soft buckets) is **cut** to stretch; Idea #2 (depth curriculum) is **demoted to a pre-registered experiment** (EXP-006) after the review showed it is probably negative — the default specialist recipe is a key-exact warm-start from the robust blueprint at the same depth.

### 1.1 The novel mechanisms (v2 status)

| # | Mechanism | v2 change |
|---|---|---|
| 1 | Jittered archetype manifolds (anti-memorization) | kept; scripts now use **exact equity + fresh per-decision draws**, making their policies analytic probability oracles (required by the trainer) |
| 2 | Posterior routing with hysteresis | fixed math: sharpening `w ∝ p^(1/T)`, weights **frozen per hand**, and the **reach-weighted behavioral mixture** `σ_mix ∝ Σ w_k π_k(i) σ_k(a|i)` — the v1 per-decision flattened softmax was self-defeating and the unweighted average was the wrong behavioral strategy |
| 3 | Regret magnitude = confidence | **replaced by visit counters** — v1's regret-ratio saturated exactly when least converged; visits drive router fallback and solver range priors |
| 4 | Drift-triggered shield | kept; per-hand hysteresis + trend-z |
| 5 | Anti-leak protocol | strengthened: **out-of-family evaluation** (perturbed-Nash, family-B scripts, noisy wrapper) — unseen seeds were never adversarial; B split into B-dev (tune) / B-test (report) |
| 6 | **NEW — M-1 proofs** | every core claim proven on tiny games *before* the scaffold: ES-MCCFR validity, one-sided convergence to exact BR, mixture ≥ specialist vs Bayes-optimal, solver = LP (review F1) |

### 1.2 Honest expectations (v2 rescale)

State-of-the-art HUNL is neural (ReBeL, Supremus, Student of Games) on GPU clusters. A tabular blueprint on an M1 will **not** approach Slumbot; v1's "≥ −100 mb/hand vs Slumbot" was fantasy. The v2 goal:

> **Best-in-class exploitative HUNL agent with rigorous evaluation on M1** — specialists at ≥ 70% of their computed exploitation ceilings, the mixture beating the robust baseline in a preregistered paired trial, solver soundness proven against independent oracles, and Slumbot reported as a diagnostic anchor.

## 2. System architecture (v2)

```
cham-rec (leaf)      cham-proofs (leaf: Kuhn/Leduc/Bayes/LP proofs, M-1)
   │
cham-core ──► cham-engine ──► cham-blueprint ──► cham-agent ──► cham-eval ──► cham-cli
   │              │                ▲   │  ▲          ▲
   └─► cham-opponents ┘            │   └──┼── cham-router (weights, per-hand)
                                   │      └── cham-search (river: FMBR/RNR/ReachGadget)
                                   └── quantized inference artifacts (mmap, shared, ≤1.5 GB)
```

11 crates. Key v2 architecture decisions:

- **Keys are pure functions** of (hole, board, geometry): Waugh-style suit-isomorphism tables for flop/turn (offline-built, mmap'd), exact-equity quantile bins × texture for river, SPR bands everywhere, **legal mask in the key**, zero Monte Carlo at encode time.
- **Correct ES-MCCFR**: chance/opponent sampled, hero actions enumerated, no reach multipliers, no baselines; regret-based pruning; delayed linear averaging both modes; Hogwild atomics for throughput, a single-threaded deterministic mode for the byte-identical contract.
- **Solver family** replaces anchored CFR+: FMBR (max exploitation), RNR(p) (principled interpolation), reach-gadget (safe arm) — validated against *independent* oracles (Kuhn/Leduc/LP/postflop-solver as dev-time AGPL oracle, never linked).
- **Inference artifacts**: strategy-only, u8-quantized, mmap-shared (v1 would have needed ~30 GB loading five training tables).
- **Statistics**: session-clustered CIs, SPRT early stopping, Holm correction around one preregistered primary endpoint, AIVAT-style variance reduction vs known scripted policies.

## 3. Cheap feedback machine (intact, sharper)

Tier 0 `verify` (incl. proofs) → Tier 1 `probe` (LBR proxy, coverage, router calibration) → Tier 2 `ladder --fast` (SPRT-guarded screening) → Tier 3 `ab` (paired, Holm-guarded promotion) → Tier 4 `slumbot` (diagnostic anchor) → Tier H `play`/`trace`. The commit ladder is `just fast` ≤ 30 min end-to-end.

## 4. Artifacts & configs

Unchanged layout, with v2 hashing: `abstraction_hash` = blake3 over TOML **plus bucket artifacts** (v1 hashed only the TOML — retraining buckets silently invalidated blueprints). `config/baseline.toml` remains the only promotion pointer. Inference artifacts live beside training tables with their own `artifact_hash`.

## 5. Risk register (v2)

| # | Risk | Mitigation |
|---|---|---|
| R1 | Router leakage / circular eval | family governance + out-of-family eval + B-dev/B-test split; ECE gates |
| R2 | Mixture more exploitable than parts | per-hand frozen weights; reach-weighted form; shield; EXP-005 Bayes arm as the falsification test |
| R3 | Script overfitting | jittered manifolds + family-B adversarial family |
| R4 | Memory blowup | reduced action tree (≤2 sizes + jam, raise cap 2), 100 bb dev depth, measured-growth infoset estimator, knob-down path |
| R5 | Throughput reality below hopes | M-1 spike before budgets are trusted; P4 provisional; budget table recomputed from measurements |
| R6 | Slumbot variance/API drift | diagnostic only; 20k seatings; dialect-verify-first gate |
| R7 | Legacy pkr surprises | Annex §6 checklist; workspace self-contained |
| R8 | Subtle estimator/math bugs | M-1 proofs; cargo-mutants on traversal; negative tests; independent oracles |

## 6. Grounding Annex — legacy `pkr` reconciliation (unchanged; needs `dump-pkr.txt`)

- [ ] A1 evaluator vs P1 gate (≥100M/s — if legacy is faster, port behind `cham_core::eval`)
- [ ] A2 iteration throughput — **v2 note: the M-1 spike supersedes the brainstorm's ~5.5k iters/s assumption; the review estimates 300–1500 iters/s/thread memory-bound at 200 bb. Measure, then trust.**
- [ ] A3 bucket centroids importable via `cham-engine` tables (v2 format: iso-orbit tables; conversion script required)
- [ ] A4 legacy A/B numbers exported to `artifacts/legacy_baseline.json` before parking branches
- [ ] A5 legacy rules engine vs our fuzz suite (port if cleaner)
- [ ] A6 units (bb=100 chips, SB=50) confirmed or adapted at the opponents layer
- [ ] A7 legacy ablation tooling jurisdiction unchanged

## 7. Driving an implementation agent

Same loop as v1 (read 00 → crate spec → current milestone; module+tests; gates; decisions.jsonl), plus two v2 rules: (a) **M-1 gates everything** — no scaffold before the proofs pass; (b) measured numbers (P4/P6/σ) override doc numbers — update docs in the same commit rather than working around them.

## 8. Milestone summary (full detail: SPECS/11)

| Milestone | Days | Content | Gate (headline) |
|---|---|---|---|
| **M-1 Prove it** | 1–3 | `cham-proofs`: Kuhn/Leduc ES-MCCFR + BR convergence + Bayes-mixture + LP-solver; 200 bb throughput spike | 4 proofs green; spike numbers recorded — **everything stops if these fail** |
| M0 Foundation | 4–6 | scaffold, `cham-core`, `cham-rec`, verify | P1 ≥ 100M/s, P2 ≥ 10M/s, leak-proof |
| M1 Walking skeleton | 7–10 | all 10 crates THIN on a tiny abstraction; one full cycle end-to-end | ledger entry with CI; σ per opponent measured |
| M2 Scale & specialists | 11–16 | full tables, robust + 4 specialists (robust warm-start), LBR/ceilings | efficiency ≥ 0.70 per specialist; robust LBR ≤ 150 mb |
| M3 Router & primary verdict | 17–21 | collect/train-router, modes, EXP-001/003 | G3 + **G2-primary verdict** |
| M4 Search & anchors | 22–26 | solvers + oracle suite, EXP-002, Slumbot, dashboard | G5, G4, G7, G8-diagnostic |
| M5 Hardening | 27–31 | shield, full G1–G9 pass, promotion | final ledger; honest report |
