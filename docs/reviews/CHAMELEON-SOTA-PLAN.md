# CHAMELEON → State-of-the-Art HUNL on an M1 — Execution Plan

> **Audience:** an autonomous coding agent (and the human who supervises it).
> **Scope:** the whole workspace in `dump.txt` (604 files, 12 crates, ~130 dated plan docs).
> **Goal:** make the bot *actually* competitive against strong (equilibrium-class and adaptive) heads-up no-limit Hold'em opponents, while staying inside a 16 GB Apple-M1 budget.
> **Honesty note:** I read the dump (README, SPECS, the latest ~30 result docs, and the trainer / traversal / table / encoder / ladder / search / agent code). I could **not** compile or run anything. Every Rust snippet below is written against the APIs I read and is **UNVERIFIED** until it builds. Every task therefore ships with a test and a gate; **the gate, not the snippet, is the source of truth.**

Evidence tags used throughout:

| Tag | Meaning |
|---|---|
| **[V]** | verified by reading the code/doc text in `dump.txt` |
| **[C]** | inferred from code reading, not measured |
| **[D]** | a number taken from one of your docs/logs (cited by file name) |
| **[E]** | my estimate (arithmetic shown) |
| **[H]** | hypothesis — the plan contains the experiment that settles it |

---

## 0. How the executing agent must work (READ FIRST)

1. **One task at a time.** Each task has an ID (`T1.4`), files, steps, a test, and a **GATE**. Do not start task N+1 until task N's gate is green. Commit after each task with the ID in the message.
2. **Never trust a metric you did not just print.** The history of this repo is a sequence of metrics later overturned (clairvoyant LBR 6–10× too high; "negative exploitability" that was an under-converged learner; search "works" against a manipulator that loses either way). Every task that claims an improvement must print its number **with a standard error** and the **zero-sum bound check** (§W0).
3. **Keep the constitution** (`docs/SPECS/00-conventions.md`): `unsafe` forbidden in workspace crates; dependency whitelist is closed (`rayon`, `arrayvec`, `rustc-hash`, `bytemuck`, `blake3`, `serde*`, `thiserror`, `memmap2` are all already whitelisted and are all this plan needs); no `Vec` allocation in per-decision hot paths *of the live agent* (the offline trainer/solver pools its buffers); no `Instant::now()` outside `cham-search::budget`; no `thread_rng`; evaluation uses iteration budgets, wall-clock only in live play.
4. **Never read behaviour from environment variables inside a key path or a training path.** Two env flags (`CHAM_SLOT_BUCKET`, `CHAM_COMPRESS_HISTORY`) currently change infoset keys through a process-global `OnceLock` (`ladder.rs::slot_bucket_enabled`, `encoder.rs::history_compression_enabled`) [V]. That is how a "26.6% fallback" contamination happened (`INSIGHT-RUN-2026-10-06`) [D]. From abstraction `version = 3` on, those behaviours are **config fields**, hashed into `abstraction_hash`.
5. **Determinism is a feature, keep it.** New parallel code must be bit-reproducible under a fixed seed (mini-batch + fixed-order merge, §W1). No Hogwild in new code.
6. **If a snippet does not compile, fix it to the intent, do not delete the test.** If an API assumption in an "API CHECK" box is false, stop, write the finding into `docs/plans/<task-id>-FINDING.md`, and adapt minimally.
7. **Every result gets a ledger entry** (`artifacts/ledger/ledger.jsonl`) bound to the blake3 bundle hash — the infrastructure exists, use it.
8. **Time-boxing:** each task lists an effort in *agent-days* (an agent-day ≈ one focused session of code+test+measure). If you exceed 2× the estimate, stop and write down why.

---

## 1. Diagnosis — where the bot really stands

### 1.1 What is solid (preserve it)

* Engine with fuzz tests, deterministic RNG discipline, hash-bound artifacts (`policy.bin` v2, blake3), paired A/B with SPRT, hash-bound ledger, shadow gauntlet, GPU eval7/EHS kernels bit-exact against the CPU path, a safe-resolving *concept* (gadget unit test: opponent gap 27.06 → 0.013, `GADGET-COMPLETE-2026-10-06`) [D].
* A culture of audit (130 plan docs). The weakness is not rigor of process; it is that **the instruments were wrong several times and the architecture's ceiling is low** (below).

### 1.2 The measured facts that matter

| # | Fact | Source |
|---|---|---|
| F-1 | Strength is only ever measured against **9 scripted bots** (mean ≈ 8.3k mb/seating with search OFF). Nothing adaptive/equilibrium-like has beaten or been beaten by the bot in a way that can be trusted. **No Slumbot result exists anywhere in the dump** (grep: only mock/spec mentions). | `INSIGHT-RUN-2026-10-06`, `chameleon-competitiveness-report.md`, grep [V][D] |
| F-2 | **Converged in-abstraction exploitability (sum of both seats' best-response values) is ~8–10 bb**: tiny-full 8.19, rich-lite 10.27 (DCFR) / 10.11 (CFR+) / 10.56 (12M iters); SE of the sum ≈ 0.9 bb. | `DEFINITIVE-RESULTS-2026-10-06`, `HONEST-COMPARE-2026-10-04` [D] |
| F-3 | That number **does not fall with more iterations** (5M → 12M: 10.27 → 10.56) and does not fall with a richer tree or a different regret schedule. | same [D] |
| F-4 | For scale: a policy that folds every hand has a best-response sum of exactly 1.5 bb (the blinds). The shipped blueprint is ~6–7× *worse than folding* by this metric. A converging equilibrium solver cannot plateau there. | arithmetic [E] + F-2 |
| F-5 | Every earlier "≈ 0 exploitability" claim was an **under-converged learner** (negative sums violate BR₀+BR₁ ≥ 0). | `FULLCOV-AND-THE-BOUND-2026-10-04`, `CONVERGED-EXPLOITABILITY-2026-10-04` [D] |
| F-6 | The abstraction is tiny: preflop 169 classes, **32 flop / 16 turn / 16 river** buckets, **one** flop and **one** turn bet size (0.5 pot), river {0.5, 1.25} + jam, raise cap 1. Richer trees measured *worse* at matched iterations. | `config/abstraction-tiny.toml`, `HONEST-COMPARE` [V][D] |
| F-7 | Training is external-sampling MCCFR, **one deal per iteration**, ~0.33 ms/iter/thread; 5M iterations ≈ 62 visits per infoset on 80k infosets (rich-lite: 26 visits on 190k). That is sample-starved by 2–3 orders of magnitude relative to what blueprint solvers use. | `F6A-INFOSET-EXPANSION`, `RICH-LADDER-INFOSET-EXPLOSION` [D] |
| F-8 | The live river search is **harmful on the scripted ladder (−2,104)** and its "win" is measured against a nit-then-deviate manipulator that loses ~3.4k mb/seating to the agent *either way*. | `INSIGHT-RUN`, `SEARCH-WORKS-2026-10-06`, `SEARCH-ADAPTIVE-*` [D] |
| F-9 | The honest router gate fails (0.761 top-1, 0.45 TAG recall); the shipped default is **pure argmax** over one-sided-BR experts. | `README` (Tracked gaps) [D] |

### 1.3 What the code reading adds (new, not in your docs)

| # | Finding | Evidence |
|---|---|---|
| C-1 | **The river "search" solves a different game from the one being played.** `try_solve` returns `None` unless `obs.to_call == 0` and street is river; hero is collapsed to **one class** (its exact equity), villain to 3 strength classes derived from the tracker; the subgame tree is hard-wired "hero acts first, villain responds, one raise of 2.2×" — it ignores position (BB acts first postflop). A single-class hero cannot be balanced, so this is an *exploit heuristic*, not equilibrium re-solving. | `search_bridge.rs::try_solve`, `subgame.rs::hero_node/villain_node` [V] |
| C-2 | **The production trainer has never been validated against a game with known exploitability.** `cham-proofs` deliberately re-implements Kuhn/Leduc *separately* (SPECS/00 §1), so a defect in `traversal.rs`/`table.rs`/averaging could survive every test. The BR metric is a *sampled learner* with its own noise floor and a "passive default" for thin infosets. Plateau F-3 is therefore ambiguous: trainer defect, abstraction floor, or metric defect. | SPECS/00 §1; `lbr.rs` header [V] |
| C-3 | **`Encoder::key_for` mixes the bucket into the *middle* of an FNV byte stream** (`street|player|spr|bucket|belief|mask|seq…`). The "public part" of a key cannot be hashed once and combined with 1,326 buckets, which blocks every vectorised algorithm (§W1) and costs a full re-hash per hand per node. | `encoder.rs::key_for` [V] |
| C-4 | The key path reads **process-global env flags** (see rule 4). | `ladder.rs`, `encoder.rs` [V] |
| C-5 | `ActionSeq` records full 4-street history (compression is opt-in), and the ladder reads `raises_per_street_cap` from config — together the source of the 40× infoset blow-up observed for richer trees. Imperfect-recall action abstraction is the standard cure and is already half-implemented (`CHAM_COMPRESS_HISTORY`). | `encoder.rs`, `RICH-LADDER-INFOSET-EXPLOSION` [V][D] |
| C-6 | `Traversal` returns `NaN` for a missing row in the parallel phase and every ancestor skips its update (selection bias; rate now telemetered as `cold_rows`). | `traversal.rs` [V] |
| C-7 | `train_robust_parallel` uses *warmup slices* (single-threaded insert pass) then parallel Hogwild phases; the averaging weight is a hand-rolled `(t − T/4)·γ^(T−t)` (default γ now 1.0). Non-standard and easy to get subtly wrong. | `trainer.rs` [V][D] |
| C-8 | GPU is used only for table building; the CPU path uses 4 threads and a scalar per-hand `evaluate7`. The M1 has 4 P-cores + 4 E-cores, NEON, and 2.6 TFLOPs of GPU — none of it applied to the actual solving workload. | `README`, `docs/gpu/*` [V] |

### 1.4 Reading of the situation

> Your system is a **well-instrumented exploit-oriented tabular bot with a ~10 bb-exploitable, 32/16/16-bucket blueprint and a toy real-time component.** Against scripted opponents it wins big. Against a Nash-class opponent it should be expected to lose, and *no instrument you own can currently prove or disprove that.*

Tuning knobs (DCFR α/γ, more sizes, more iterations, router features, search flags) have been explored for 130 documents and are exhausted: F-3 says the plateau is **structural**. The plan therefore changes the *structure*:

1. Build the instrument that cannot lie (exact card-perfect best response) — **and run it on the shipped bundle first.**
2. Replace sampling noise with a **vector-form solver** (public-chance-sampling DCFR) that converges 1–2 orders of magnitude faster per wall-second and doubles as the ground-truth checker.
3. Spend the saved compute on a **real abstraction** (more buckets, real preflop tree, key v2).
4. Replace the toy search with a **combo-level, position-correct, safe real-time solver built from the same kernels**.
5. Only then layer exploitation, sized by *measured* exploitability budgets.

---

## 2. Target architecture

```
                 ┌────────────────────────── OFFLINE (days) ───────────────────────────┐
  abstraction v3 │  cham-vcfr  (NEW)                                                    │
  (key v2,       │   ├─ tree     public betting tree from engine+ladder                 │
   buckets,      │   ├─ kernels  O(n) fold/showdown with card removal (f32, NEON-friendly)
   preflop tree) │   ├─ pcs      public-chance-sampling DCFR, mini-batch, deterministic │
        │        │   ├─ vbr      EXACT card-perfect best response (the ground-truth ruler)
        ▼        │   └─ export   dense rows ──► policy.bin (same artifact format)       │
  robust blueprint└────────────────────────────────────────────────────────────────────┘
        │
        ▼                         ┌────────────── LIVE (≤ 2–3 s / decision) ──────────────┐
  ChameleonAgent ──► blueprint σ ─►│ cham-vcfr::solver  (same kernels, identity buckets)   │
   tracker/router (λ-capped)       │  ranges = blueprint reach per combo (1326)            │
        │                          │  safe gadget (Burch–Brown–Sandholm), position-correct │
        │                          │  river ≈ 0.3 s · turn ≈ 2 s · flop = blueprint (+opt) │
        ▼                          └───────────────────────────────────────────────────────┘
   sample (never argmax) ──► action            exploitation = RNR(p) inside the solver,
                                                p ← min(confidence, measured budget)
```

Design principle: **one numerical core** (`cham-vcfr`) used by the trainer, the exact evaluator and the live solver. A bug fixed once is fixed everywhere, and the evaluator and trainer are cross-checked by sharing *only* the kernels, not the algorithms.

---

## 3. Roadmap, gates, kill criteria

| Phase | Work | Duration (agent-days) | **Gate to continue** | **Kill / pivot rule** |
|---|---|---:|---|---|
| **A** | W0: anchors started in background, safe defaults, encoder refactor | 3–4 | Slumbot probe running; sparring partner trained; `search` default OFF; `cargo test --workspace` green | — |
| **B** | W1 (T1.1–T1.7): kernels, tree, **VBR**; run VBR on shipped bundle (**Decision D1**) | 6–8 | Kernel parity tests bit-close to brute force; VBR(uniform) ≫ VBR(shipped) ≫ 0; VBR of a hand-built Nash toy ≈ 0 | If kernels disagree with brute force by > 1e-4 relative: stop, fix before anything else |
| **C** | W1 (T1.8–T1.10) PCS trainer on the **existing tiny abstraction** (**Decision D2**) then W2 (key v2, abstraction v3, buckets) | 8–12 | **D2:** PCS at ≤ 4 h wall beats shipped bundle's VBR by > 3 SE *and* is monotone-decreasing in iterations | If PCS also plateaus at the same level ⇒ the floor is the *abstraction*, skip to W2 immediately (more buckets/real tree) before more trainer work |
| **D** | W3: combo-level river → turn solver + gadget + live integration | 8–12 | Duplicate match vs blueprint-only ≥ 0 (CI), VBR(blueprint+resolve) ≤ VBR(blueprint), latency budget met | If solver loses to blueprint-only ⇒ ranges (T3.2) are wrong; fix ranges before touching solver |
| **E** | W4: deployment (sampling, λ-capped exploitation, translation), W5 perf, 200 bb | 5–8 | Beats sparring partner; Slumbot CI trending to ≥ 0; ladder regression ≤ 15% vs old agent on scripted bots | — |
| **F** | W7 stretch: flop depth-limited solving / learned leaf values | open | Only after Phase D gates pass with margin | — |

**Decision D1 (end of Phase B).** Run `vbr` on `artifacts/agent-honest-19dim/robust` and `tiny-full`. Three outcomes:
* VBR ≈ the old tabular number (8–10 bb) ⇒ the blueprint really is that bad ⇒ Phase C is mandatory.
* VBR ≫ old number ⇒ abstraction leak the old BR couldn't see ⇒ Phase C + W2 mandatory, more urgent.
* VBR ≪ old number ⇒ the old metric was the problem ⇒ re-rank every past decision with VBR before continuing (cheap: artifacts exist).

**Decision D2 (end of T1.9).** Same abstraction, same 4-hour budget: ES-MCCFR (shipped) vs PCS-DCFR. Winner = lower VBR with SE. This settles C-2/C-7 by experiment instead of by argument.

---
