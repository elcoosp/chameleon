# SPECS/10 — Experiment Protocol — v1 (normative)

The rules that make results trustworthy. v1 fixes the circularity and the statistics (review B1, C): out-of-family evaluation, one preregistered primary endpoint with Holm correction, SPRT early stopping, session-clustered CIs, exploitation-efficiency gates derived from computed ceilings, and honest expectations (Slumbot is diagnostic; the goal is **best-in-class exploitative HUNL agent with rigorous evaluation on M1**, not Nash-adjacency).

---

## 1. Vocabulary

- **Config** = `(AgentMode, artifact hashes, abstraction_hash, pool spec, seeds)`; comparable runs differ in exactly the intended knob.
- **Verdict** = ledger entry with a CI-based decision. **Diagnostic** = no CI or no promotion path.
- **Promotion** = `chameleon ab --promote` moving `baseline.toml`. No other path exists.
- **In-family** = jittered nit/TAG/LAG/station from family-A scripts. **Out-of-family** = perturbed-Nash, family-B scripts, noisy wrapper (SPECS/03 §5).

## 2. Seed & family governance (anti-leak v1)

- **Seed partition** (FNV-1a mod 10): **A** (0–5) router training + specialist jitter draws; **B-dev** (6–7) tuning, temperature sweeps, SPRT screening; **B-test** (8) headline numbers — touched at most twice per experiment, never tuned on; **C-registered** (9) reserved for out-of-family session seeds.
- **Family governance** (the v1 hole): C is defined by *opponent family*, not seed novelty. "Unseen jitter draws" are not adversarial — the jitter distribution was the training distribution. Out-of-family opponents never appear in any training or tuning dataset, enforced at the dataset loader (`family` field) and the pool config.
- No cross-set leakage anywhere; loader refusals are build failures.

## 3. Sample sizes & power (computed from measured σ, per opponent)

Duplicate-pair σ per opponent measured in the M1 pilot (v1 borrowed CallBot's σ — wrong); committed to `config/pool.toml`. Reference budget (σ_pair ≈ 3.5 bb/deal, VR factor ~2 from §AIVAT measured, session-clustered):

| Tier | Scope | Seatings | Purpose |
|---|---|---|---|
| smoke | 4 arch × 2.5k deals ×2 | 20k | not-catastrophically-broken |
| screening (`ladder --fast`, SPRT on) | 4 arch + 2 baselines × 10k deals ×2 | 160k | kill bad ideas cheaply |
| promotion (`ab`) | 2 arms × 4 arch × 25k deals ×2 | 800k | verdicts |
| headline (G1/G2) | 100k seatings/opp × 4, 3 clusters | 1.2M+ | the paper numbers |
| Slumbot anchor | 20k seatings | 20k | **diagnostic** (±140 mb at σ≥10 bb) |

SPRT defaults: H0 Δ=0 vs H1 Δ=+25 mb/seating, α=0.05, β=0.10 — screening arms stop early on boundaries; stopped arms are ledger-labeled.

## 4. Multiple comparisons (v1)

**One preregistered primary endpoint: G2-primary** (full vs robust-only, overall pool delta, promotion A/B). Every other gate is a **secondary**; the gate family is Holm-corrected at α=0.05 within each experiment's pre-registered list (`experiments/EXP-*.toml` carries the family). v1 ran 9 gates × 4 archetypes × many experiments with no correction — a false-promotion machine.

## 5. Tier flow

```
change → just test → just verify (Tier 0, incl. cham-proofs)
       → probe (Tier 1): FAIL ⇒ stop
       → ladder --fast (Tier 2, SPRT): kill or advance
       → ab (Tier 3, promotion rule + Holm): --promote moves baseline
       → nightly: ladder --full + slumbot --seatings 20000 (Tier 4, diagnostic anchors)
```

## 6. Acceptance gates v1

| ID | Claim | Gate | Tier |
|---|---|---|---|
| **G1** | Specialists exploit their archetype | **exploitation efficiency ≥ 0.70** = winrate / BR-ceiling per archetype (ceiling from `lbr` long runs vs each point script — computed, not guessed), session-clustered CI excludes 0 | headline |
| **G2-primary** | The thesis: mixture ≥ robust | overall pool delta full vs robust-only, paired, CI rule, **the** promotion gate | ab |
| G2-secondary | per-archetype deltas ≥ 0 (Holm-corrected family) | ab |
| **G3** | Router is real and calibrated | top-1 ≥ 0.80 B-dev; **ECE ≤ 0.15 on B-test and on out-of-family sessions**; posterior mass on true family ≥ 0.5 for perturbed-Nash variants | router train + ladder |
| **G4** | River search earns its keep | best search arm (EXP-002) ≥ no-search, paired CI > 0, **Iterations budget**; conservative ReachGadget arm must not lose (CI ⊅ −10 mb) | ab |
| **G5** | Solver soundness | independent-oracle suite (Kuhn/Leduc/LP/postflop-solver dev-time): mean EV loss ≤ 10 mb/hand | offline |
| G6 | Bayes arm compared | EXP-005 bayes vs mixture vs argmax reported with Holm correction; no promotion requirement (it's the falsification test of the mixture idea) | ab |
| **G7** | Hysteresis cost bounded | drift sessions (nit→LAG etc. at hand 250): full ≥ +40 bb/100 | ladder, family-C seeds |
| **G8** | External anchor | Slumbot mb/seating with CI, 20k seatings — **diagnostic, no pass/fail** (v1's "≥ −100 mb/hand" was fantasy at this compute; SOTA is neural on GPU clusters) | slumbot |
| **G9** | Robust arm is sound | LBR vs robust blueprint ≤ 150 mb/hand on the abstraction (from `cham-blueprint::lbr`) | probe |

**Cut from v1:** soft-bucket gate (feature cut), Glicko-ELO gates (ELO cut), "C top-1 ≥ 0.60" (vacuous).

## 7. Special evals

- **Drift sessions (G7):** SwitcherBot nit→LAG / TAG→station / LAG→nit at hand 250, family-C seeds, 20k seatings each; rolling net-bb/100 chart around the switch (textual in `trace`, charted in dashboard).
- **Shield test:** forced trend_z injection (unit) + one 20k-seating session vs a counter-exploiting SwitcherBot; shield must not lose > 25 mb/seating vs the argmax arm.

## 8. Experiment registry & launch order (updated)

Each hypothesis: `experiments/EXP-<id>-<slug>.toml` with frozen `expect` (hash recorded at run start; editing after a verdict is a ledger violation). Launch order:

1. **EXP-001 `mixture-vs-robust`** (G2) — the thesis verdict.
2. **EXP-002 `search-arms`** — FMBR vs RNR(0.9) vs ReachGadget vs off (G4), after G5.
3. **EXP-003 `router-ablations`** — feature blocks dropped (G3 sensitivity), temperature T ∈ {0.5, 0.7, 1.0}.
4. **EXP-005 `bayes-vs-mixture`** (G6) — the Bayesian-game alternative.
5. **EXP-006 `depth-ladder`** — the old Idea #2 as a pre-registered experiment vs robust warm-start (SPECS/04 §7); dies if it can't win.
6. EXP-007+ (stretch): soft buckets with EMD buckets; 5th "whale" archetype; family-B-trained router transfer.

## 9. Reporting rules

Unchanged: no number without CI + n; screening labeled screening; failed experiments are results and live in the ledger. Added: the frontier plot (exploitability vs winrate) is the standard reporting artifact; legacy pkr comparison stays via `legacy_baseline.json` (Annex A4).

## 10. Failure modes this protocol catches

v1's table stands, plus: "C seeds made it look adversarial" → family governance §2; "we promoted on the 11th uncorrected comparison" → §4; "SPRT would have saved 4 GPU-hours" → §3; "we tuned on the headline set" → B-dev/B-test split.
