# CHAMELEON v3 Brainstorm — Competitive Against Nash, Inside the Fence

> **Status:** [DRAFT] — brainstorm v0.1; nothing committed to implement

> **Status**: brainstorm draft v0.1 for review.
> **Grounding**: all numbers cite `docs/COMPETITIVENESS-AND-SPEED-REPORT` (measurements from `target/criterion/*/new/estimates.json`, `bench-before-gpu.txt`, `docs/reports/bench-20260925.md`, `docs/reports/bench-gpu-trials.md`, `artifacts/ledger/ledger.jsonl`).
> **Relationship to v2**: v2's contract (per `CHAMELEON-v2-ROADMAP.md §7`) was *"the ability to say, with a computed number and a CI, whether your own agent would survive a bot built specifically to beat it."* v3's contract is sharper: **be flat against a Nash bot, and strictly beat everything that deviates from Nash — without leaving the 16 GB M1 fence.**
> **Honesty note**: where this document depends on repo files I could not read in this session, items are tagged **[VERIFY]**. Nothing tagged should be treated as settled until checked against the repo.

---

## 0. The Constitution — non-negotiable constraints v3 inherits

Every idea in this brainstorm was filtered against the following fences. If an idea cannot live inside them, it is not a v3 idea; it is a v4-or-never idea.

1. **Hardware fence**: one Apple M1 Mini, 16 GB unified RAM, `target-cpu=native`. Deployment artifacts are tabular blueprints and lookup tables, mmap-loaded (the `artifact_load` benchmark shows 43.03 µs per load, ~23,200 loads/s — mmap is a proven, fast pattern in this codebase). No inference-time neural networks; the deployment artifact set must remain inspectable, bit-exact, and deterministic across seeds.
2. **Latency fence**: the P7 gate requires hero decision p99 < 1.0 ms. The measured standard decision is **4.38 µs** (95% CI [4.15, 4.47]) — a >200× margin. v3 may *spend* some of that margin on richer router features, but the gate itself does not move. Search decisions live inside the 250 ms live move clock; the measured 400-iteration RNR river solve is 13.15 ms end-to-end (~19× margin), and v3 imposes a self-cap of **p99 ≤ 150 ms** on any expanded solving so that platform jitter never turns margin into a timeout.
3. **Memory fence**: any abstraction growth (more k-means buckets, wider action trees, extra feature tables) must arrive with a **byte budget table** showing the resident set stays under ~12 GB (16 GB physical, minus OS/tooling headroom). The GPU table build already produced 1.44 GB of bit-exact turn EHS tables (worklog G1.2); that is the scale of what fits, and it is the unit of account for what does not.
4. **Correctness fence**: the four M-1 proofs stay green at all times (P-1 Kuhn Nash value −0.0556 vs −1/18; P-2 best-response convergence; P-3 reach-weighted mixture ≥ 90% of Bayes-optimal EV; P-4 FMBR/LP river solver matches closed form to 1e-6). Determinism stays bit-exact across seeds. `unsafe` stays out of core bot logic. The GPU stays **out of the online decision path** — it builds tables, and only bit-exact-validated ones (G1.2 precedent).
5. **Honesty fence**: `require_agent_artifacts()` hard-exits (exit 2) when trained artifacts are missing; no fallback score ever masquerades as strength. Every gate claim requires variance-reduced evaluation (AIVAT or duplicate deals) at pre-registered seating counts. Slumbot remains a **diagnostic anchor, never a promotion gate** (SPECS/08 §5), because σ ≥ 10 bb/seating at 200 bb needs > 20,000 seatings for a useful CI.
6. **Reproducibility fence**: ledger rows carry the artifact identity they were produced against **[VERIFY: artifact-hash tagging exists in ledger schema]**. A ladder number without an artifact hash is a rumor, not a measurement.

---

## 1. North star — what "competitive against nash bot" actually means

A true Nash bot cannot be *beaten* in expectation; by definition it guarantees the game value. So "really competitive against nash bot" must be defined operationally, in two clauses that a promotion gate can measure:

- **N-0 (Nash-flat)**: against `pnash` with 0 overfold, the 2σ confidence interval upper bound on win rate is **≤ −10 mb/seating** at 40,000 seatings. We do not try to extract EV from an equilibrium opponent; we refuse to *fund* one beyond a small, bounded rent.
- **N-1 (deviation harvest)**: against every deviating opponent in the pool — `arch:{station,nit,tag,lag}`, `famB:tag`, and the `pnash:overfold:{0.05,0.15}` family — the 2σ confidence interval **lower bound is > 0**. Wherever the opponent leaks, v3 collects at least as much as a pure Nash bot would, and more where the router works.

The claim this licenses is the one worth putting on the v3 README: *v3 is competitive with a Nash bot because it matches Nash's defense against Nash itself, and strictly dominates Nash's offense against every deviator we can classify.* A pure static-GTO bot satisfies N-0 and fails N-1 by construction — it leaves the deviation value on the table (the report's §2.3-C point: static solvers defend; the router/mixture extracts).

**Where we honestly stand today.** The ledger's 40,000-seating screening ladder produced +1.1 bb/100 vs `pnash:overfold:0.15` (2σ ≈ ±67 bb/100 — far inside noise, i.e., *undetermined*, not positive) and losses everywhere else, from −4.7 bb/100 vs `arch:station` to −59.5 bb/100 vs `arch:lag`. Critically, the report's §2.2 context (from `cmd/guard.rs` and `Broad-perf-plan`) establishes these are **fallback-contaminated numbers**: the run executed without populated specialist artifacts, so the engine played mirror/uniform fallback. The only genuinely informative fact in that ladder is the *shape* of the loss curve (monotonically worse against more aggressive opponents — the signature of an over-folding agent), which is exactly what uniform fallback produces. Conclusion: v3's baseline is not "−59.5 vs LAG"; v3's baseline is **unknown until Step Zero completes**.

**Why the ±67 bb/100 CI drives v3's whole evaluation track.** The `pnash:overfold:0.15` row's error bar (±670.8 mb/seating) is 26× wider than the ±25 mb/seating gate target. Closing that gap by raw seatings alone would take ~(670/25)² ≈ 718× more samples per opponent. That is not viable; it is the arithmetic argument for AIVAT/duplicate-deal variance reduction (Track C) being *sequenced before* any optimization work — you cannot aim at a target you cannot see.

---

## 2. Step Zero — land the artifacts; nothing else matters first

Every algorithmic idea below is worthless while the guard is exiting 2. The first v3 milestone is pure execution, and it is deliberately unglamorous:

- Complete `train-buckets` and `train-bp` for **both** 100 bb and 200 bb depths; populate `artifacts/agent/` with the blueprint bundle **[VERIFY: exact artifact manifest layout]**.
- `require_agent_artifacts()` returns green; a fresh 40,000-seating AIVAT-reduced ladder is run and committed to the ledger with artifact identity.
- That ladder becomes the **v3 baseline line** — the number every subsequent idea must move.

The reason this is a *milestone* and not a footnote: the GPU table factory is already proven (1.4 h for 1.44 GB of bit-exact turn EHS tables vs ~11 h estimated on 4-thread CPU), so the distance to a trained-artifact ladder is days, not weeks. Kill criterion for the whole Track A direction: if the trained-artifact ladder still loses to `arch:nit` (the mildest exploit archetype), then the binding gap is blueprint convergence (A1/A2), not routing — and budget follows the gap.

---

## 3. Idea pool

Ideas are grouped into four tracks. Each entry states the idea, the grounding in measured numbers, the constraint check, and a **kill criterion** (pre-agreed condition under which we drop it). Track A buys N-0, Track B buys N-1, Track C makes both decidable, Track D pays for the others.

### Track A — Blueprint strength (get to Nash-flat)

**A1. Deeper, better-shaped convergence (CFR+ style, same tabular representation).**
The M-1 proofs establish that the MCCFR machinery converges bit-exactly (P-1 hits the Kuhn Nash value −0.0556 vs −1/18 ± 0.005; P-2 proves the best-response engine converges). What the fallback ladder shows is not a broken algorithm — it is missing iterations. v3 upgrades the production blueprint trainer with: regret matching+ (clamping negative regrets), alternating updates, and linear/discounted weighting of early iterations. These are drop-in changes to the existing tabular trainer and inherit the proof harness unchanged.
*Constraint check*: pure CPU training, no representation change, no memory growth. *Metric*: abstraction-local exploitability computed with the existing P-2 best-response machinery, reported **per street** so we know *where* the blueprint is soft. *Kill criterion*: if per-street exploitability plateaus across two successive training weekends, stop buying iterations and move budget to A2/A3 — iteration returns are sub-linear once regret is dominated by abstraction error.

**A2. Same bytes, better abstraction — aim the GPU factory at feature richness.**
The proven superpower of this codebase is bulk evaluation on Metal/wgpu: 2.54e8–3.39e8 evals/s in contended trials (median 9.70× over CPU), and 3.29e9 evals/s steady-state in the full turn EHS build. v2 used that to build *speed*; v3 should spend it on *quality*: rebuild the k-means buckets over richer per-hand features — equity distributions (not just mean EHS), board-texture clusters, and improvement/potential terms — at the **same table byte budget** as today. A better partition of the same 1.44 GB-class space is free strength at deployment time: identical decision latency, identical memory, lower abstraction loss.
*Constraint check*: byte budget table mandatory (Constitution §3); GPU produces tables only, validated bit-exact per G1.2. *Kill criterion*: if the re-bucketed blueprint's abstraction-local exploitability (A1 metric) improves < 10% on sampled subgames, the current feature set was already adequate — drop the feature program, keep the table budget for A4.

**A3. Time-aware real-time solving — extend search from river to turn, paid for with measured headroom.**
The report's timing is the license: 400-iteration RNR solves cost 34.96 ms cold and the *end-to-end* river search decision is 13.15 ms (~19× inside the 250 ms clock); warm cache hits land at 189 µs. That headroom buys the single biggest "vs Nash" lever in the codebase: **Libratus-style deeper solving where the blueprint is weakest**. Concretely: (a) make RNR iteration count a *time budget* rather than a constant — start at 400, scale up while remaining wall-clock allows, hard-capped so p99 stays ≤ 150 ms [estimate, not measurement: cold 400→1,600 iters scales ≈ linearly to ~140 ms; warm-path scaling is far cheaper; **[VERIFY: per-iteration cost stability at deeper counts]**]; (b) add **turn subgames** to the trigger set, prioritized by pot-normalized stakes (solve when the pot justifies 13–150 ms); (c) widen the trigger-stream cache, since the 189 µs hit path shows caching is already the difference between 35 ms and 0.2 ms.
*Constraint check*: latency fence self-cap 150 ms p99, enforced as a CI bench gate so a regression fails the build, not the live table. *Kill criterion*: if turn solving moves the `arch:tag`/`famB:tag` ladder rows by less than the variance floor after a full AIVAT ladder, the turn blueprint was not the leak — redirect to B3.

**A4. Bet-size abstraction audit.**
`ArrayVec<LegalAction, 12>` caps the legal-action set; size abstraction is where tabular agents quietly lose EV, and it is the cheapest axis to improve. Audit the geometric sizing template per street against the action-encoding rates we already measure (`encode_flop` 7.09M keys/s, `encode_river` 5.25M keys/s), and add size variants only where the byte budget table shows room. *Constraint check*: any added size multiplies blueprint state count — must clear the memory fence. *Kill criterion*: abstraction-local exploitability deltas below the variance floor → keep the leaner tree.

### Track B — Exploitation (win the meta-game)

**B1. Parametric opponent grid — generalize `overfold:0.15` into a family.**
Today the pool exposes one deviation axis (`pnash:overfold:0.15`, currently +1.1 bb/100 but statistically undetermined). v3 pre-trains response blueprints against a **grid** over the deviation axes that matter — fold-frequency × aggression × bet-size — e.g. a 3×3×3 = 27-point grid, and the router *interpolates* between neighboring grid points instead of hard-selecting an archetype specialist. P-3 is the theoretical license for exactly this: the reach-weighted mixture captures ≥ 90% of Bayes-optimal EV in the hidden-type model, so interpolation loss is bounded and known. This converts "four discrete specialists" into a continuum of counter-strategies at a known quality discount — and it directly serves the N-1 gate, because every `pnash:overfold:*` variant and every human-ish archetype lands inside the grid's convex hull.
*Constraint check*: 27 blueprints ≈ 27× artifact bytes — must clear the memory fence via mmap streaming (43 µs loads make per-decision lazy paging viable) or grid pruning (drop cells the router never visits). *Kill criterion*: if interpolated play underperforms the best single grid neighbor by more than the P-3 bound suggests, the mixture weighting is wrong — fix the weighting before growing the grid.

**B2. Router maturity: per-street freezing, confidence gating, texture conditioning.**
The current router freezes archetype classification per hand; v3 freezes **per street** (more evidence before commitment), adds a **confidence gate** (below threshold, blend toward the base blueprint — never toward uniform; the guard's lesson is that *fallback quality* is a correctness property, not a convenience), and conditions features on board texture class from A2 so that "LAG on paired boards" and "LAG on monotone boards" are not the same cell. *Constraint check*: all router math stays closed-form Bayesian arithmetic inside the 4.38 µs decision path — feature richness is bounded by the p99 < 1 ms gate, and we hold 200× margin. *Kill criterion*: if per-street freezing degrades the screening ladder vs per-hand freezing, the classifier starves post-flop — revert to per-hand with confidence gating only.

**B3. LAG task force.**
`arch:lag` is the worst measured matchup (−59.5 bb/100 under fallback, and structurally the opponent that punishes over-folding hardest). After Step Zero re-measures the trained baseline, if LAG remains the worst row, train a dedicated **pressure specialist** (wider defense ranges, heavier 3-betting) against the LAG family and let the router pick it up as a grid extreme. Success is measurable and pre-registered: `arch:lag` 2σ CI lower bound > 0 at 40k AIVAT seatings. *Kill criterion*: none needed — this is a bounded, single-opponent engineering task with a hard gate.

**B4. In-hand counter-reweighting (cheap online regret matching across specialists).**
Within a frozen street classification, reweight the specialist mixture by *seat-level realized EV* signals with show-adjustment (avoid punting on unseen showdowns) **[VERIFY: per-seat EV stream exists in the tracker]**. This is the "online" half of the online Bayesian router story: cheap, local, and recoverable — worst case it reduces to the P-3 mixture. *Constraint check*: O(specialists) arithmetic per decision, invisible at 4.38 µs. *Kill criterion*: any negative ladder delta vs static mixture → drop it; the pre-trained grid (B1) does the heavy lifting.

### Track C — Evaluation (make the gates decidable)

**C1. AIVAT / duplicate deals as the default ledger mode.**
The ±670.8 mb/seating error bar on the `pnash:overfold:0.15` row is the whole argument. Variance reduction is not a nicety; it is the difference between "we think we're winning" and a promotion gate. v3 makes AIVAT (or, minimally, duplicate/seeded paired deals) the default evaluation mode, with per-opponent seating calculators that convert measured σ into "seatings needed for a 2σ gate at ±25 mb/seating". Slumbot stays diagnostic (existing serial client with rate limiting and exponential backoff in `crates/cham-eval/src/slumbot.rs`), reported but never gating.

**C2. Pre-registration discipline.**
Before each gate run, a small TOML/JSON file is committed declaring: opponent, artifact hash, seating count, variance-reduction method, and the pass condition. The ledger linter refuses (exit 2, same spirit as the guard) any row whose run lacks a pre-registration **[VERIFY: extend `cmd/guard.rs` or add `cmd/lint-ledger.rs`]**. This costs an afternoon and buys the project out of the most expensive failure mode in empirical bot work: post-hoc gate shopping.

**C3. Exploitability telemetry as a standing benchmark.**
Promote the P-2 best-response engine into a Criterion-tracked exploitability benchmark over sampled abstraction-local subgames, reported per street per depth. This gives Track A a *fast* feedback loop (minutes) that correlates with the slow ladder (days), and it is the earliest alarm for "the new abstraction made the blueprint softer". *Kill criterion* for any Track A/B change is read directly off this benchmark first.

### Track D — Compute & determinism (paying for the others)

**D1. GPU jobs menu (all offline, all bit-exact-validated).** Ranked: (1) A2 abstraction rebuild — proven pipeline; (2) best-response/exploitability sweeps for C3, embarrassingly parallel across subgames; (3) AIVAT baseline term computation for C1; (4) experimental: parallel ES-MCCFR traversals with seed-partitioned, fixed-order reduction to preserve bit-exactness. Jobs 1–3 reuse the existing 3.29e9 evals/s-class pipeline; job 4 is the only one touching training math and must pass the determinism fence before it feeds any artifact.

**D2. The determinism fence.** GPU output enters `artifacts/` only after byte-exact validation against a CPU reference build (the G1.2 precedent: bit-exact 1.44 GB tables). The online decision path stays scalar CPU, forever. Any kernel that cannot be validated bit-exact is rejected regardless of speed — this is a Constitution item, not a preference.

**D3. Time budget realism.** A full turn-scale table build is ~1.4 h; a v3-scale abstraction rebuild (features × streets × depths) is plausibly an order of magnitude more — call it a weekend of compute, not a month **[VERIFY: flop/river board-texture counts for the real estimate]**. Training-time neural distillation is *deliberately out of scope for v3*: the distill-to-tabular path is the v4 door if v3's gates stall, and it stays closed unless the gates say so.

---

## 4. Milestone ladder G3.x — pre-registered, numbered like the G1.2 precedent

Gates are ordered; each one exists to de-risk the next. Seating counts below assume C1 (AIVAT/duplicate) is live; without variance reduction, none of these are decidable at any sane seatings count.

| Gate | Name | Pass condition (pre-registered) | Serves |
|---|---|---|---|
| **G3.0** | Guard-green | Artifacts populated for 100 bb + 200 bb; `require_agent_artifacts()` exit 0; ledger rows carry artifact identity | Constitution |
| **G3.1** | Baseline line | Trained-artifact 40k-seating AIVAT ladder committed; all matchups reported with CIs. Calibration, not pass/fail | Everything |
| **G3.2** | Ladder sweep | Every pool archetype (`station`, `nit`, `tag`, `lag`, `famB:tag`) 2σ CI lower bound > 0 at 40k seatings | N-1 |
| **G3.3** | Nash-flat | vs `pnash` 0-overfold: 2σ CI upper bound ≤ −10 mb/seating at 40k seatings | N-0 |
| **G3.4** | Deviation harvest | vs `pnash:overfold:0.15`: point estimate ≥ +5 bb/100 with 2σ lower bound > 0 (vs today's undetermined +1.1 ± 67) | N-1 |
| **G3.5** | Exploitability proxy | Per-street abstraction-local exploitability (C3 benchmark) under a threshold set from v2 specs **[VERIFY: threshold value]** | Track A |
| **G3.6** | Slumbot diagnostic (stretch) | 20k+ seatings vs Slumbot 200 bb; point estimate reported, **non-gating** per SPECS/08 §5 | Reputation only |

Two design notes on the ladder. First, G3.2 is deliberately *before* G3.3: harvesting deviations is where the architecture (router/mixture/grid) has structural advantage, and it is the faster path to a meaningful win; Nash-flatness is the harder, subtler target and benefits from the C3 telemetry loop being mature. Second, every gate run must include **at least two holdout opponents** — archetype variants or `pnash` grid points not used in any training — so that "we beat the pool" cannot silently mean "we memorized the pool" (the `famB:tag` row exists precisely for this role; add parametric holdouts from the B1 grid **[VERIFY: pool.toml holdout slots]**).

## 5. Risk register

1. **Training-time blowup** (A1/A2 both want compute). Mitigation: iteration caps with checkpoint/resume, GPU offload via D1, and the C3 fast-feedback loop so direction is checked in minutes, not weekends. *Kill*: two consecutive weekends per depth without C3 improvement → freeze abstraction, buy solver depth instead (A3), because solving attacks abstraction error from the live side.
2. **Memory breach** (A2 feature richness, B1 grid size). Mitigation: byte budget table is a review artifact, not an afterthought; mmap streaming for cold artifacts. *Kill*: any table set pushing resident > 12 GB → coarsen flop buckets first (river already has real-time solving as a backstop), keep turn/river fine — that ordering follows from where search covers for us.
3. **Router cascade** (one bad freeze ruins a hand). Mitigation: B2 confidence gating blends toward the *base blueprint* (a real strategy), never uniform (the fallback incident's lesson); per-street freezing bounds blast radius to one street. The guard stays responsible for *missing artifacts*, not for runtime classification — runtime never exits, it degrades gracefully.
4. **GPU determinism leak** (a fast kernel silently corrupts artifacts). Mitigation: D2 byte-exact validation gate; GPU never enters the online path. *Kill*: any non-reproducible kernel is dropped at review, regardless of speedup — no exceptions, because the project's identity is computed numbers you can trust.
5. **Pool overfitting**. Mitigation: holdout opponents in every gate run (§4); B1 grid evaluated on grid *points* never trained on. *Kill*: if holdout rows consistently underperform trained rows by more than the variance floor, the router is memorizing — reduce grid granularity, increase specialization quality.
6. **Variance illusions** (false gates, wasted months). Mitigation: C2 pre-registration + ledger linter; AIVAT mandatory; the ±670.8 mb/seating lesson quoted in every gate doc. *Kill*: any gate claimed without a pre-registration file is void by definition, and the ledger linter makes that mechanical.
7. **Zero-variance ledger rows** (`callbot`/`jamfix` at 0.0 ± 0.0 look unplayed rather than perfectly balanced **[VERIFY]**). Mitigation: ledger linter flags zero-variance rows as suspicious data hygiene failures.

## 6. Recommended sequencing (EV per unit effort)

1. **Step Zero → G3.1** (days): land artifacts, run the trained baseline ladder. Everything downstream is aiming without this.
2. **C1 + C2 + C3** (about a week, partly parallel with 1): variance reduction, pre-registration, exploitability telemetry. Rationale: gates must be decidable *before* optimization, or optimization is guesswork. This is the cheapest, highest-leverage work in the whole document.
3. **A1 + A4** (parallel with 2 where CPU-only): CFR+ shaping and the size-abstraction audit — cheap, proof-compatible, measured by C3 within minutes.
4. **A2 via D1** (a weekend of GPU): the feature-rich re-bucketing. Gate the decision on A1's C3 plateau signal — if iterations are still buying exploitability, A2 waits a cycle.
5. **B1 + B2** (the exploitation program): grid training and router maturity, once the baseline blueprint and the eval loop exist. B3 follows immediately if LAG remains the worst row.
6. **A3** (turn real-time solving) last in the training arc, because solving quality inherits abstraction quality — and it arrives just in time to defend G3.3, where Nash-flatness is decided in the streets where the blueprint is thinnest.

The through-line: **C-track first, A-track second, B-track third, A3 last** — the opposite of the intuitive order (people reach for exploitation tricks first), but the report's own numbers justify it: you cannot tune what you cannot measure (±67 bb/100), and you cannot exploit players from a blueprint that funds them.

## 7. Open questions for the v3 owner

1. **Which Nash?** This document assumes the primary "nash bot" is `pnash` 0-overfold in `config/pool.toml`, with Slumbot as diagnostic. If "nash bot" meant Slumbot, G3.6 promotes to a gate and the seating budget changes accordingly.
2. **Is 16 GB still the wall for v3?** Everything here assumes yes (the "distill-to-tabular" option stays a closed v4 door). If v3 may use training-time neural teachers or larger hardware, A2's feature program and B1's grid change shape substantially.
3. **Does the tracker expose per-seat EV streams** (needed for B4) **[VERIFY]**, and does the ledger schema carry artifact hashes (Constitution §6) **[VERIFY]**?
4. **What threshold does G3.5 inherit from the v2 specs** for abstraction-local exploitability **[VERIFY]**?
5. **Is `pnash:overfold:0.05` in the pool?** If yes, it joins G3.4 as a second, sharper deviation-harvest target, since smaller overfold is closer to the Nash boundary where interpolation loss shows first.


