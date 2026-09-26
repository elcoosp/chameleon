# CHAMELEON v2 — Roadmap

> **Status:** [CONTEXT] — rationale for V2-DEV-PLAN; reference only

**Status:** core is sound and should ship as-is. v2 is a layer on top, not a rewrite: it makes "competitive" measurable, imports two literature techniques that survived fact-checking, cuts one that didn't fit the project's own determinism contract, and adds several original, cheap, evidence-gated mechanisms. Every item below enters through the existing M-1 / EXP-registry / ledger machinery — v2 adds **zero** new trust mechanisms, only new things to measure with the ones you already have.

---

## 0. The one rule that governs this whole document

Nothing in v2 becomes a default until it has a ledger entry with a CI that beats the v1 baseline, exactly like every other change in this project. Where a technique is imported from a paper, the paper's headline number is a *hypothesis*, not a budget line. Treat every row below as an `experiments/EXP-*.toml` candidate, not a spec change, until it's proven on your own hardware.

---

## 1. The core reframe: making "beat other bots" a real, measurable claim

v1 correctly killed v1's fantasy Slumbot target and made it diagnostic-only. That was right — but it left "competitive" with no teeth. There is no second runnable bot to benchmark against on this hardware; ACPC-era bots were never released, and Slumbot is the only stable external opponent that exists. So v2 redefines "beat other bots" as five concrete, cheap-to-add measurements instead of one impossible one.

### 1.1 — M6: Self-Exploit Audit (the headline new idea)

The honest way to answer "would a bot built to beat me, beat me" is to **build that bot** using tooling you already have.

`ChameleonAgent`'s behavioral mixture `σ_mix(a|i)` is fully computable given frozen per-hand weights — it's the same shape as an archetype script's `action_probs`. That means a **frozen snapshot of your own promoted `full` agent can be wrapped as an analytic opponent policy**, exactly like `ArchetypeAgent`. Point `cham-blueprint`'s existing `Exploit` mode at it:

```
TrainMode::Exploit { opponent: OpponentSpec::Frozen(full_agent_snapshot), jitter_seed: 0 }
```

This trains a genuine best-response specialist against your own deployed policy, using code you already wrote for the specialist pipeline. It lives in `cham-eval` (or a thin new `cham-audit` binary) rather than `cham-opponents`, because it needs `cham-agent`'s router/tracker machinery — `cham-eval` already sits above that in the DAG, so no new edge is required.

Two variants, both cheap:

- **Cold-start self-exploit**: weights frozen at the session-start prior (no tracker history). This is functionally the same number as `lbr_vs(cold_start_full_policy)` (§1.2 below), just obtained via training instead of exact enumeration — use the LBR version as the cheap diagnostic and this as the expensive confirmation once per release.
- **Adaptive self-exploit (the actually new part)**: the wrapped opponent runs the *real* tracker + router across a session, so the trained exploiter can try to **manipulate the classifier** — e.g. play like a nit for 40 hands to get routed toward the nit-specialist mixture, then deviate. Nothing in v1 checks for this. This is the single biggest unaddressed risk in a router-based architecture, and it's answerable with tooling you already have.

**Gate `G-SELF`** (diagnostic, tracked in the final report, not a promotion blocker): self-exploit winrate vs `full`, with CI. This is your honest exploitability headline number — the number you'd want to know before anyone else finds it.

### 1.2 — Cold-start mixture LBR, tracked alongside G2

R2 ("mixture more exploitable than parts") is currently validated **only** in the M-1 toy game. Add one cheap production-scale check: `lbr_vs()` against the `full` agent's cold-start policy (hands_seen = 0, a static function of infoset alone — no session simulation needed, reuses `cham-blueprint::lbr` exactly as built). Report it every time EXP-001 runs, next to G2's delta. This is the number that would tell you the thesis is quietly making you *more* exploitable in exactly the way R2 warns about, and it costs you nothing you don't already have.

### 1.3 — Abstraction-free sanity pass on G1

`lbr_vs` computes the best response inside your own training abstraction. If the abstraction is coarse, the ceiling is depressed and "efficiency ≥ 0.70" can look great for the wrong reason — both numerator and denominator live in the same lossy abstraction. Before trusting G1: run one LBR pass per specialist with a materially finer action grid (double the bet-size granularity, uncapped raises) than your training ladder. If efficiency collapses under a finer grid, the number was flattering you, not measuring you.

### 1.4 — Slumbot: promote from purely diagnostic to a stated aspirational target

Keep G8 non-blocking (v1's "-100 mb/hand" fantasy stays dead). But add an explicit, honestly-scoped target on the dashboard: **CI excludes negative at 20k seatings** — a real bar most hobbyist bots don't clear, achievable on this hardware, tracked but never gating promotion.

---

## 2. Literature imports — verified, then triaged

I checked the four load-bearing claims in the brainstorm doc against the actual papers before recommending anything.

| Technique | Verified? | Verdict for CHAMELEON |
|---|---|---|
| **CCS-MCCFR** (correlated chance sampling) | Real — arXiv:2607.27035, Jul 2026 | Adopt as `EXP-009`. ~100 LOC, essentially free to implement. **But the paper's own ablations show every paired interval crosses zero on Liar's Dice, reduced Flop Hold'em, and Libratus turn/river endgames** — the games structurally closest to yours. The 20–34% headline is real but concentrated in tiny, high-revisit games (Kuhn/Leduc). Validate in the M-1 harness, expect near-zero production gain, don't budget on it. |
| **CS-RNR** (confidence-scheduled restricted response) | Real — arXiv:2607.28520, Jul 2026 | Adopt as `EXP-010` after G4/G5 are green. Directly extends your existing `solve_rnr.rs` with a self-audited safety certificate. Their measured certificate cost (3–22 ms) is on Leduc-scale games — **re-measure at your river-subgame scale against the 250 ms live budget** before assuming it's cheap enough for `play`/Slumbot; it's almost certainly fine for eval-mode (`Iterations`). |
| **Embedding CFR** | Real — arXiv:2511.12083, AAAI 2026 | Keep as `EXP-008`, post-M5, exactly as v1 already scoped it. Independent reviewers of the paper flag: *"validation limited to a simplified poker variant; extension to full-scale games absent."* Don't let the "first algorithm to..." framing raise its priority. |
| Dirichlet-posterior router confidence | Underlying math is sound; the "Nov 2025 lecture" citation is unverifiable | **Skip.** Your existing visit-counter `c = v/(v+64)` is already shape-equivalent to a Dirichlet posterior's variance under a monotonic transform for this use case — added complexity for no demonstrated behavior gap. Only revisit if `cargo-mutants` triage on the router finds a real edge case the visit counter misses. |
| LLM-as-router labels | N/A | **Cut, not deferred.** It introduces a non-seed-reproducible, unversioned, unauditable input into a pipeline whose entire value proposition is determinism and provenance hashing (00 §3). Directly violates your own contract. If you want richer labels later, get them from finer deterministic clustering on tracker features, not a model call. |
| Hyperparameter Schedules (RBP/discount tuning) | Not independently verified | Don't hardcode borrowed numbers. Treat as `EXP-011`: sweep your existing `θ₀=10bb, δ=0.99, γ=0.9` against any imported schedule, gated the same as everything else. Ship with your own values as default in the meantime — they're already in the v1 spec and untested imports shouldn't jump the queue. |

---

## 3. Original additions (new to v2, none require new dependencies)

### 3.1 Shadow ladder — a champion/challenger regression gauntlet

Right now regressions are only caught against scripted opponents. Add a lightweight standing gauntlet: every promoted `baseline.toml` is retained as a frozen `bot:shadow-<hash>` opponent spec, and every new promotion candidate plays a screening-tier match against the **last 3 promoted baselines**, not just the pool. This catches "beats the pool, loses to what we shipped two weeks ago" — a real failure mode in iterative RL/CFR development that pool-only testing misses. Cost: near-zero (reuses `MatchRunner`, adds opponent specs, no new infra).

### 3.2 Cross-abstraction ensemble disagreement as a shield signal

Train the *same* specialist recipe on a second abstraction with a different bucket seed/count (cheap on the reduced-tree, 100bb dev config — this is a k-means reseed and a retrain, not new code). At inference, the disagreement between the two abstractions' strategies at a given infoset is a second, independent uncertainty signal — orthogonal to your visit-counter confidence, because it catches *abstraction-specific* overfitting rather than *undertrained* infosets. Feed it into the shield as an additional trigger (`shield_beta` grows with disagreement, not just `trend_z`). This is inspired by standard ensemble-disagreement uncertainty estimation, not a specific paper — it's a general, well-understood technique, cheap here because your abstractions are already small.

### 3.3 Meta-strategy solve over your own agent zoo

You already have ~8 agent modes (`full`, `no-search`, `argmax`, `robust-only`, `bayes`, `fmbr`, `rnr`, `reach`) and pairwise winrates between many of them from the A/B history. Instead of picking the deployed mode by a single G2 comparison, build the small pairwise payoff matrix from ledger data and solve for the **empirical-game Nash mixture** over your own modes (a tiny linear program — 8×8, hand-rollable in `stats.rs`, no new deps; this is the same idea behind empirical game-theoretic analysis / α-Rank, simplified to a size where a basic LP or even fictitious play converges in milliseconds). This tells you, principled and computed rather than guessed, whether `full` is actually dominant or whether some mode combination would be harder to exploit — and it's a natural thing to compute once you already have the pairwise ledger data from M3–M4.

### 3.4 Bucket-quality audit test

Nothing currently verifies buckets group hands by *realized* EV, only by *equity-histogram similarity*. Add a cheap post-hoc test in `cham-engine`: from real match data, compare within-bucket vs. between-bucket variance of realized showdown EV for a sample of same-bucket hands. This is exactly the "provably works" discipline the rest of the spec already applies everywhere else, currently missing from the abstraction layer itself.

---

## 4. Hygiene follow-ups (small, concrete, do before trusting the numbers)

1. **Verify `strat_sum` accumulation against Lanctot's Algorithm 3 exactly.** The regret update in `traversal.rs` (`regret[a] += v[a] − v_bar`, no reach factor) is correct external-sampling MCCFR. The `strat_sum` update carries no `reach_hero` term anywhere in the spec — confirm this is the intended variant before M2, and don't let a passing M-1 Kuhn proof stand in for the actual `cham-blueprint` implementation being bug-free; keep `cargo-mutants` on `traversal.rs` as a hard M2 gate, not a nice-to-have.
2. **`ExploitBayes`'s 13-bin belief key ≈ 13× table size.** Validate its memory footprint on the M1 walking-skeleton abstraction *before* M3 engineering time goes into it.
3. **memmap2 "safe API" note.** `#![forbid(unsafe_code)]` covers your own crates only; memmap2 uses unsafe internally at the syscall boundary. Your design (hash the artifact, then mmap read-only, never mutate) avoids the practical risk — just document this precisely so "zero unsafe" isn't read as a broken promise in a later audit.
4. **AGPL due diligence on `postflop-solver`-derived fixtures** deserves an actual five-minute read of the license terms around committing derived numeric outputs, not just a `decisions.jsonl` note.

---

## 5. Updated milestone table

| Milestone | Adds |
|---|---|
| M-1 → M5 | unchanged from v1 |
| **M3** | + cold-start mixture LBR reported alongside every EXP-001 run (§1.2) |
| **M4** | + abstraction-free LBR sanity pass before trusting G1 (§1.3); + shadow-ladder opponents registered (§3.1) |
| **M5** | + Slumbot aspirational target tracked on dashboard (§1.4); + bucket-quality audit (§3.4) |
| **M6 — Self-Exploit Audit** (new, 3–4 days, after M5) | Wrap frozen `full` agent as an analytic opponent; train cold-start and adaptive self-exploiters via existing `Exploit` mode; report `G-SELF` in the final report; run cross-abstraction ensemble retrain (§3.2) if time remains; compute the meta-strategy solve over the mode zoo (§3.3) |

**EXP registry additions:** `EXP-008` (Embedding CFR, unchanged from v1, post-M5), `EXP-009` (CCS-MCCFR), `EXP-010` (CS-RNR), `EXP-011` (schedule sweep). All Holm-corrected secondaries under the same primary-endpoint discipline as everything else.

---

## 6. What stays cut

Soft buckets (EXP-007 stretch, unchanged), Glicko ELO, dashboard scope creep, replay animation, turn search, 5th archetype, depth curriculum as a default (EXP-006, unchanged), **and now LLM-as-router labels, permanently** — not because the idea is bad in isolation, but because it breaks the determinism contract that is this project's actual competitive advantage over a slicker-sounding but less rigorous alternative.

---

## 7. Honest closing expectation

On an M1 Mac mini with a tabular blueprint, you will not beat neural, GPU-trained SOTA (ReBeL/Supremus-class systems) — that was already correctly ruled out in v1. What v2 adds is the ability to say, with a computed number and a CI, whether your own agent would survive a bot built specifically to beat it (§1.1), whether your central thesis is safe from its own known failure mode (§1.2–1.3), and whether you clear a real, externally-verifiable bar (§1.4) — which is a stronger, more honest claim than "we didn't lose too badly to Slumbot," and it's achievable with code you're already writing.
