# SPECS/11 — Milestones & Agent Work Plan — v2 (M-1 → M5, 5–6 weeks)

v2 restructure per review F: **M-1 "prove it" before any scaffold** (tiny-game proofs of every core claim), a **walking skeleton** before scale, honest throughput calibration, trimmed v1 scope (soft buckets, ELO, replay animation, turn search, 5th specialist — all cut), development at 100 bb, and a 5–6 week timeline (v1's 2–4 weeks was not credible).

---

## M-1 — Prove it (days 1–3) · crate `cham-proofs`

Before the 10-crate scaffold, in self-contained micro-implementations (Kuhn, Leduc, a 2-rank LP river, a hidden-type toy), demonstrate:

1. **P-1 ES-MCCFR validity:** exploitability on Kuhn → < 0.065 bb/hand (Nash value 1/18) after N iters; Leduc → below the literature-threshold at matched compute.
2. **P-2 One-sided exploit training** converges to the **exact best-response value** vs a fixed scripted opponent (both computable in closed form on Kuhn/Leduc).
3. **P-3 Router + reach-weighted mixture beats the best single specialist** against a hidden-type opponent, and achieves ≥ 90% of the **exact Bayes-optimal policy's EV** (enumerable in the toy). *This tests the project's central claim for the cost of an afternoon.*
4. **P-4 River solver = LP:** the FMBR/RNR machinery matches enumerative-LP solutions on small river trees to 1e-6.

Plus the **throughput/infoset spike**: run the pilot ES-MCCFR at 200 bb with the v2 abstraction on a draft table build; measure P4 reality (expect 300–1500 iters/s/thread, memory-bound); observe actual table growth for the infoset estimator (drop v1's "analytic growth" — a 50k-deal sample cannot see rare paths).

**Gate G-M-1:** all four proofs green in `chameleon verify --proofs`; spike numbers recorded in `decisions.jsonl` and folded into P4/P6/§3 budgets. **If P-1..P-4 fail, the architecture is wrong — stop and redesign; nothing downstream is built.**

## M0 — Foundation (days 4–6)

**Build:** workspace scaffold (00 §1 incl. `.cargo/config.toml`, profiles), `cham-core` complete (SPECS/01: fast evaluator via gate P1, Copy State, ArrayVec legality, PublicHistory), `cham-rec` (SPECS/12), `cham-cli` with `verify` only, justfile + deny + nextest wiring.

**Gate G-M0:** `just test` green · `chameleon verify --proofs` green · P1 ≥ 100M evals/s · P2 ≥ 10M actions/s · fuzz 1M clean · leak-proof tests green.

## M1 — Walking skeleton (days 7–10)

**Build the whole pipeline THIN, then scale** (review F3): tiny abstraction (flop k=32, turn k=16, river 16 bins × 4 textures, 2 bet sizes), all 10 crates minimally complete: engine tables (quick build), archetypes, blueprint (both modes), router, agent, eval + ledger, CLI subcommands, dashboard stub. One robust blueprint + one TAG specialist trained on the tiny abstraction; one full `collect → train-router → ladder --fast → ab` cycle executed end-to-end.

**Gate G-M1:** the cycle produces ledger entries with CIs; `identical_streams_ab` and `duplicate_profit_formula` green; σ per opponent measured and committed; P3a/P3b/P6 recalibrated; inference artifacts built and mmap-shared (≤ 1.5 GB at full abstraction projected).

## M2 — Scale & specialists (days 11–16)

**Build:** full abstraction tables (`train-buckets`, committed artifacts + blake3), reduced action tree everywhere, robust @100bb (Hogwild, 8–16M iters — budget from the M-1 spike), 4 specialists via **robust warm-start** (SPECS/04 §7), `lbr` + ceilings, probe Tier-1.

**Gate G-M2:** table ≤ 6 GB (`verify --count-infosets` from measured growth) · P4 confirmed at 100bb scale · each specialist: **exploitation efficiency ≥ 0.70** vs its point script on the smoke→screening tier (early G1 read) · robust LBR ≤ 150 mb/hand (G9) · `warmstart_beats_cold` green.

## M3 — Router & the primary verdict (days 17–21)

**Build:** router dataset (`collect`, binary, session-clustered), router training + gates, agent modes incl. `bayes`, EXP-001 and EXP-003.

**Gate G-M3:** G3 (top-1 B-dev ≥ 0.80, ECE ≤ 0.15 on B-test and out-of-family) · **EXP-001 verdict (G2-primary)** — the thesis lives or dies here · mode matrix + deterministic replay green · `just fast` ≤ 30 min.

## M4 — Search & anchors (days 22–26)

**Build:** river solvers (FMBR/RNR/ReachGadget) + independent-oracle suite, EXP-002, Slumbot client + mock + **dialect verification**, dashboard (trimmed 4 sections), `play`/`trace`.

**Gate G-M4:** G5 (oracle suite ≤ 10 mb/hand EV loss) · G4 verdict (search arm > no-search or the arm is shelved and `full` ships with search off) · Slumbot 20k-seating diagnostic recorded (G8) · drift eval (G7) green.

## M5 — Hardening & close (days 27–31)

**Build:** shield + counter-exploit test, full protocol pass G1–G9, final promotion, docs, Annex reconciliation.

**Gate G-M5:** every gate ID has a ledger verdict · `baseline.toml` = promoted `full` (search per G4) · `just fast` green on fresh checkout · honest final report: per-archetype efficiency table, frontier plot, Slumbot diagnostic, known-weaknesses list.

## Risk-driven fallbacks (pre-authorized, in order)

| Symptom | Fallback |
|---|---|
| M-1 proofs fail | stop; redesign the specific mechanism (estimator / mixture / solver); nothing downstream is built |
| Table > 6 GB | river eq-bins 64→48, flop k 300→200, raise cap 2→1; re-measure; record EV-proxy delta |
| P4 < 800 iters/s | reduce to 1 postflop size + jam; shrink seq window to 6; re-verify G1 ceilings |
| Warm-start < 40% efficiency gain | train cold; EXP-006 moot |
| Router ECE fails on out-of-family | widen jitter, retrain; if still failing: argmax + visit-fallback ships, G2 clause 2 becomes the gate |
| Search misses oracle suite | iters 400→800 within budget; else `full` ships search-off (G4 → stretch) |
| Slumbot API unstable | G8 waived with ledger note; everything else unaffected |

## Agent session protocol (unchanged)

Read `SPECS/00` → current crate spec → this file's current milestone; strict task order; implement module + named tests; `cargo nextest run -p <crate>`; commit `cham-<crate>: <module> (test names)`; gates before milestones advance; ambiguity → minimal interpretation that passes the named tests, recorded in `decisions.jsonl`. **New:** throughput/budget numbers from the M-1 spike are authoritative over any number in these docs — update the docs in the same commit rather than working around them.
