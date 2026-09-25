# REVIEW-RESOLUTIONS — map of external review → spec changes (v2)

Every numbered point from the spec review, and where it landed. "kept" = verified already-correct.

## A. Fatal

| # | Review point | Resolution | Where |
|---|---|---|---|
| A1 | River key has no board | River bucket = exact equity-vs-uniform quantile bin (64, global thresholds) × texture class (8). Board-aware by construction | 02 §3, §5a; test `river_bucket_board_aware` |
| A2a | MC noise inside the key | Encode is a pure function of (hole, board): Waugh-style suit-isomorphism tables (flop ≈1.3M, turn ≈55M×u16 ≈110 MB, mmap), built OFFLINE with seeded MC, frozen artifacts. Test `bucket_pure_function` + structural `no_mc_in_encode` | 02 §3, §6 |
| A2b | Features should be potential-aware; EMD ≈ L1 on CDFs | 16-bin river-equity CDF histograms; k-means on CDF vectors = near-free EMD. EHS/EHS² dropped | 02 §3 |
| A2c | P3 impossible with MC inside encode | Split gates: P3a ≥ 1M/s (table path), P3b ≥ 100k/s (river exact-enumeration path, arithmetic documented) | 00 §6, 02 §6 |
| A3a | Exploit estimator invalid (double-weighted p², no importance weight, unsound baseline) | Rewritten as correct ES-MCCFR: chance/opponent sampled, hero actions ENUMERATED, `R(a) += v(a) − Σσv`, no reach multipliers/baselines. Hand-checkable regression test `exploit_enumeration_estimator` | 04 §4 |
| A3b | In-spec self-argument ("NO —") | Deleted; normative pseudocode only | 04 §4 |
| A3c | Add RBP + linear discounting | Regret-based pruning (θ_t = 10 bb · 0.99^t) with `rbp_matches_full`; Linear-CFR discounting in robust averaging | 04 §4 |
| A3d | Seat unspecified | Hero seat drawn uniformly per iteration (exploit); alternation in robust | 04 §4; test `seat_randomized` |
| A4 | action_probs not computable (persistent hand uniforms + MC ehs) | Exact equity everywhere; fresh independent draws per decision; analytic action_probs + consistency tests | 03 §3–4; tests `action_probs_*` |
| A5 | on_hand_end leaks hidden cards | `PublicHistory` (no seed, showdown-holes only) is the ONLY agent-visible history; HandHistory is internal; leak tests extended | 01 §5–6, 07 §2 |
| A6 | Router features 31/32 circular + DAG violation; 25–30 opponent-blind | Deleted; N=20 tracker+opportunity features, frozen at hand start; structural no-blueprint-inputs test | 05 §2; 07 §4 |
| A7a | softmax(p/T) flattens (certain posterior → 0.58) | `w ∝ p^(1/T)`; the 0.58 case is the regression test `sharpening_math` | 05 §5 |
| A7b | α=0.3/decision isn't hysteresis; mid-hand switching | Weights computed once per hand, frozen for the hand; hysteresis hand-to-hand | 05 §5; test `weights_frozen_per_hand` |
| A7c | Mixture ignores own-reach (Kuhn) | Behavioral mixture `σ_mix(a|i) ∝ Σ w_k π_k(i) σ_k(a|i)` (≤8 lookups); toy test vs closed form | 05 §5, 07 §4 |
| A7d | Blurred mixes are BR-to-nothing | EXP-005: ExploitBayes policy — one policy, per-session sampled hidden type, quantized belief bin in the key | 04 §5, 10 §8 |
| A8a | 5 training tables ≈ 30 GB at play time | Strategy-only u8-quantized inference artifacts, mmap read-only (memmap2+bytemuck, safe), shared; ≤1.5 GB for all five | 04 §6, 00 §6 |
| A8b | Same key, different W | Legal mask inside the key; W = popcount(mask); invariant I8 | 02 §5c, 04 §2 |
| A8c | Turn/river alias pots; seq window drops preflop | SPR bands (log-spaced, 16) in ALL streets' keys | 02 §5b |
| A8d | Depth bands contradict key_depth_alignment | Depth bands removed; SPR bands preserve cross-depth alignment exactly (fractionally-identical sequences ⇒ same SPR ⇒ same key); test updated both ways | 02 §5b, test `key_depth_alignment` |
| A8e | f32 strat_sum saturates at 2²⁴ | Row-wise renormalization at snapshot (scale strat_sum + avg_weight); `renorm_preserves_strategy` | 04 §2 |
| A8f | Robust stores 2 regret arrays but key has position | One regret row per infoset in all modes | 04 §2 |
| A8g | Shared-table + no-unsafe + bit-identical can't coexist | Two named modes: Hogwild (AtomicU32 relaxed CAS-add, interleaving-dependent, documented) and Deterministic (single-thread, bit-identical; all reproducibility tests here) | 00 §3.5, 04 §2 |
| A9 | Range [u64;3] = 192 bits < 1326 | `Range([u64; 21])` + roundtrip tests | 01 §3 |
| A10 | Slumbot dialect invented | Pinned to published client (`/api/login`, `/api/new_hand`, `/api/act`, `k/c/f/b<amt>`, `winnings`); verify-first gate `slumbot_dialect_verified` against a recorded 50-hand real session before long runs | 08 §5 |

## B. Thesis-level

| # | Point | Resolution | Where |
|---|---|---|---|
| B1 | Evaluation circular; "adversarial" seeds aren't adversarial | Out-of-family opponents: PerturbedNash (over-fold/over-call/over-raise tilts), FamilyB scripts (independent implementation), NoisyAgent wrapper; C = out-of-family by construction; B → B-dev/B-test | 03 §5, 10 §2 |
| B2 | River re-solve un-exploits; λ-anchor unsound | Solver family: FMBR, RNR(p), reach-gadget; anchor deleted; G4 pre-registered expecting possible failure; conservative arm must not lose | 06 §4, 10 §6 |
| B3 | "Matrix game" wrong term | "Extensive-form river subgame"; metric = LBR gap | 06 §3 |
| B4 | Self-referential oracle | Independent oracles: Kuhn/Leduc equilibria, enumerative-LP spots, postflop-solver as dev-time-only AGPL oracle (deny-listed, never linked) | 06 §5 |
| B5 | Regret-ratio "confidence" saturates wrongly | Visit counters (u32/row); c = v/(v+64) | 04 §2, §6 |
| B6 | Current-iterate extraction jittery | Delayed linear averaging BOTH modes (D = iters/4; ×0.9^(T−t) robust) | 04 §4 |
| B7 | Depth curriculum probably negative | Cut from default; key-exact robust warm-start same-depth; ladder demoted to EXP-006 with a must-beat-warmstart gate | 04 §7 |
| B8 | Expectations unrealistic | Goal reframed: best-in-class exploitative agent, rigorous eval; Slumbot = diagnostic (G8 no pass/fail) | PLAN §1.2, 10 §6 |

## C. Statistics & budget

| # | Point | Resolution | Where |
|---|---|---|---|
| C1 | Session-clustered CIs | Cluster bootstrap over sessions for absolute numbers; paired deal-level for A/B diffs; coverage test | 08 §3 |
| C2 | Sequential testing | SPRT (0 vs +25 mb, α .05/β .10) on screening; ledger-labeled early stops | 08 §3, §6 |
| C3 | Multiple comparisons | One preregistered primary endpoint (G2-primary) + Holm-corrected secondaries per experiment family | 10 §4 |
| C4 | Variance reduction | All-in-EV always; AIVAT-style known-opponent baseline (≥1.5× else auto-disable); duplicate always | 08 §4 |
| C5 | Search must be deterministic in eval | Iterations budget in all eval paths; WallClock only live play | 06 §2, 00 §3.6 |
| C6 | Tune on B, report on B | B-dev / B-test split | 10 §2 |
| C7 | Define "hands"; σ per opponent | Deal/seating definitions normative; per-opponent σ measured at M1 pilot, committed | 00 §4, 08 §2–3 |
| C8 | Paired-profit formula garbled | `profit(d) = (net_seatA + net_seatB)/2` (sum — seat advantages cancel by adding); regression test | 08 §2 |
| C9 | G1 numbers guessed | Exploitation efficiency = winrate / BR-ceiling (computed via `lbr`); G1 = ≥0.70; LBR-vs-robust frontier replaces Glicko | 08 §8, 10 §6 |
| C10 | Router dataset format | Binary .rbin (84 B/row), one row per hand, 2M cap, session-clustered splits | 05 §4 |
| C11 | Budget table inconsistent | Honest tier table derived from measured P6/σ; Slumbot ±140 mb @20k, ~10 h wall | 08 §9, 10 §3 |

## D. Rust & dev loop

| # | Point | Resolution | Where |
|---|---|---|---|
| D1 | P1/P2 100× too low | P1 ≥ 100M evals/s, P2 ≥ 10M actions/s; criterion benches with thresholds replace `#[ignore]` tests | 00 §6, 01 §3/§8 |
| D2 | Vec in hot paths | `State: Copy` (≤128 B), ArrayVec legality (cap 12), borrowed `Observables<'a>`; structural tests + gates | 01 §5–6, 00 §11 |
| D3 | Debug builds glacial | `[profile.dev/test] opt-level = 3`, `-C target-cpu=apple-m1` | 00 §1 |
| D4 | M1 memory: one training at a time | Normative in 00 §6 | 00 §6 |
| D5 | Calibrate at 200bb; 5.5k iters/s optimistic | M-1 spike; P4 provisional ≥1.5k/s single-thread; budgets re-derived from measurements | 11 M-1, 00 §6 |
| D6 | Nearest-slot translation | Pseudo-harmonic top-2 weights for off-tree sizes (search ranges + AIVAT baselines; keys stay nearest/deterministic) | 02 §4 |
| D7 | abstraction_hash misses bucket artifacts | blake3 over TOML + artifacts; FNV only as hot-path mixing | 00 §7, 02 §2 |
| D8 | AllIn/Bet{max} dual encoding | AllIn variant removed; canonical Bet/Raise-to-cap + `is_all_in` flag | 00 §4, 01 §5 |

## E. Crates & deps

memmap2, bytemuck, arrayvec, blake3, criterion, proptest, insta added to the closed whitelist; `holdem-hand-evaluator` conditional on P1; dev tools nextest/llvm-cov/mutants/deny wired in the justfile; candle = v2 stretch only. | 00 §2, 09 §4

## F. Restructure

| # | Point | Resolution | Where |
|---|---|---|---|
| F1 | M-1 "prove it" | `cham-proofs` crate: P-1 ES-MCCFR validity (Kuhn/Leduc), P-2 one-sided → exact BR, P-3 mixture vs Bayes-optimal (≥90%), P-4 solver = LP; gates ALL downstream work | 11 M-1, 00 §1 |
| F2 | Pilot throughput/infoset spike; drop analytic growth | M-1 spike; estimator uses measured table growth | 11 M-1, 02 §7 |
| F3 | Walking skeleton first | M1 = all 10 crates thin on tiny abstraction, one full cycle end-to-end | 11 M1 |
| F4 | Reduce action tree; dev at 100bb | ≤2 postflop sizes + jam, raise cap 2, dev @100bb, 200bb = Slumbot only | 02 §2, 00 §4 |
| F5 | v1 cuts | Soft buckets → stretch EXP-007; Glicko cut → frontier; dashboard → 4 sections; replay animation cut (trace kept); turn search → stretch; 5th specialist → stretch | 02 §2, 08 §8, 09 §2, 10 §8 |
| F6 | Timeline 5–6 weeks | Adopted (M-1…M5, 31 working days) | 11 |
| F7a | In-spec self-corrections | All removed (donk note, size_idx note, start_stack note, Fold/Check note) | 01 §5, 03 §3 |
| F7b | cham-rec specless; wrong cross-ref | New SPECS/12-cham-rec.md owns the registry; 00 §9 points there | 12 |
| F7c | 12 vs 11 subcommands | Exactly 11, asserted by test | 09 §2 |
| F7d | No dataset command; probe ownership | `collect` added; probe owned by cham-eval (compute: blueprint lbr + router metrics) | 09 §2, 08 |
| F7e | core↔rec arrow contradiction | cham-rec = leaf; cham-core depends on nothing | 00 §1 |
| F7f | Bare `chameleon` in justfile | `$(CHAM) = cargo run -q -p cham-cli --` everywhere | 09 §4 |
| F7g | Loader 200bb vs play --depth 100 | Depth-flexible loader; blueprint depth must equal play depth | 07 §6 |

**Kept as-is (reviewer-endorsed):** ledger, seed governance, paired design, frozen predictions, dependency discipline, closed whitelist mechanism, DoD/test-name contractualism.
