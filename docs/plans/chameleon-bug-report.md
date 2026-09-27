# CHAMELEON — Codebase Bug Report

**Repo:** github.com/elcoosp/chameleon @ `e87f3cee0d382c90bcb82aa48983015735ab765b` (2026-09-27)
**Scope:** all 12 crates (~30.4k lines of Rust, 511 lines of MSL/WGSL), tests, benches, CI workflows, shell scripts
**Method:** 6 parallel deep-review passes (one per crate group) + independent line-by-line review of `cham-core` (engine, evaluator, RNG, ranges), followed by manual verification of every HIGH/CRITICAL finding against the source. The four `cham-search` solver findings were additionally confirmed by re-implementing the algorithms and simulating them. Rust toolchain was unavailable in the review sandbox, so `cargo check/test` was not run; all findings are static-analysis results with exact file/line references.

---

## Executive summary

| Severity | Count | Meaning |
|---|---|---|
| 🔴 Critical | 4 | Solver/training math is wrong at the core — outputs are invalid, not merely degraded |
| 🟠 High | 14 | Wrong numbers, silent leaks, crashes on valid input, or broken contracts that gates claim to enforce |
| 🟡 Medium | 15 | Weakened gates, misreported statistics, latent data corruption |
| 🔵 Low | 20+ | Edge-case panics, test bugs, dead flags, perf traps |

**The headline:** the poker *engine* (`cham-core`) — cards, dealing, betting legality, chip conservation, the 7-card evaluator — survived an intense review essentially **clean**. The problems concentrate in the layers that *consume* it: the river subgame solver models a mathematically different game than poker (3 criticals), the SPRT early-stopping statistic is wrong by a factor of *n*, the opponent tracker's EWM half-life formula inverts its own spec and pushes every stat out of `[0,1]`, and several "gates" (hash checks, class-balance checks, per-class recall, P-2 proof) are vacuous — they pass unconditionally, which is precisely why the bugs beneath them went unnoticed.

Themed clusters:

1. **The river solver solves the wrong game** (B-1…B-5): regrets never accumulate; a villain call doesn't match the bet; fold payoffs depend on hole strength; the pre-river pot is never awarded; the "independent oracle" returns the matrix max instead of the Nash value.
2. **Vacuous gates mask real bugs** (B-11, B-12, B-15, B-24, B-26, I-2…I-4): truncation checks that can't fire, hashes computed but never compared, recall computed but never enforced, a proof with `passed: true` hardcoded.
3. **Statistics that misreport** (B-9, B-10, B-19, B-20): SPRT LLR inflated ×n, ladder CIs computed on 250 deals while the mean covers 25,000, EWM stats leaving `[0,1]`, all-in variance reduction that can never activate.
4. **Silent fallbacks / silent corruption** (B-8, B-13, B-14, B-22, B-23): opponent actions never reaching the bot's infoset key in `play` and in tests; corrupt artifacts degrading to uniform play; resumed training double-applying iterations.

---

## 🔴 Critical

### C-1. CFR+ discards cumulative regrets every iteration — the river solver cannot converge
**File:** `crates/cham-search/src/solve.rs:444-477` — *verified by reading the code and by numerical simulation*

```rust
for t in 0..iters {                       // line 400
    ...
    let mut new_regret: BTreeMap<(String, u8), Vec<f64>> = BTreeMap::new();   // line 444 — declared INSIDE the loop
    for (path, player, actions, node) in nodes.iter() {
        ...
        entry[i] += (v[i] - node_v).max(0.0);   // line 473 — accumulates nothing (fresh map, one visit per key)
    }
    regret = new_regret;                        // line 477 — accumulated table replaced by this iteration's instantaneous regrets
}
```

`new_regret` is created fresh each iteration and each `(path, player)` key is written exactly once per iteration, so the `+=` never spans iterations. Line 477 then **overwrites** the regret table with only the current iteration's positive instantaneous regrets. Regret Matching+ requires `R ← max(R + (v − v̄), 0)`; this implements "match last iteration's positive regrets", which does not converge.

*Empirical confirmation:* ported verbatim to Python on the test spot (`tests/search.rs::spot()`): the shipped algorithm is stuck at `lbr_gap = +2.15 bb` at 60, 400 **and** 2000 iterations (root strategy stays near-uniform), while a corrected cumulative-RM+ port converges to `lbr_gap = 0.0000 bb` at 2000 iterations (root ≈ pure check).

**Impact:** every triggered river solve (RNR / reach-gadget / FMBR path) returns a heavily exploitable strategy that is effectively independent of the iteration budget. This silently degrades every downstream number (`meta_solve`, `hero`, search-enabled arms).
**Fix:** hoist `new_regret` out of the loop (or drop it and update `regret` in place): `regret[key][i] = (regret[key][i] + (v[i] − node_v)).max(0.0)`. Then re-baseline the committed oracle JSONs and tighten `tests/search.rs` to assert convergence (`lbr_gap → 0`) instead of range checks.

### C-2. Subgame: villain's CALL invests nothing — getting called pays the same as checking
**File:** `crates/cham-search/src/subgame.rs:115-130` — *verified*

`hero_node` calls `villain_node(hero_invested + bet, villain_invested, true)` (line 99) — so inside `villain_node`, `villain_invested` is still the **pre-call** total (0 at the root). The call child passes it through unchanged:

```rust
let mut children = vec![
    Node::Terminal { hero_invested, villain_invested: hero_invested }, // fold
    self.showdown(hero_invested, villain_invested),                    // call — villain_invested NOT raised to hero's level
];
```

After hero bets `h`, the call terminal is `(h, 0)`. Via `showdown_value` (line 177-185): **win → `villain_invested` = 0**. Value-betting the nuts and getting called yields exactly what checking through yields (0); losing to a call costs the full bet; calling is free for villain. The solver therefore collapses toward *never betting* (simulated: FMBR returns a pure check vs a pure calling station; the committed test only passes because it accidentally sums the CALL probability at the bet/raise node).
**Fix:** the call child must match the bet: `self.showdown(hero_invested, hero_invested)`.

### C-3. Subgame: fold payoffs depend on hole strength — winning bluffs lose money
**File:** `crates/cham-search/src/subgame.rs:124-128, 146-154` — *verified*

`Node::Terminal` stores only `(hero_invested, villain_invested)` and **every** terminal — including folds — is evaluated through `showdown_value`, which branches on `hero.strength` vs `villain.strength`:

```rust
// villain folds:
Node::Terminal { hero_invested, villain_invested: hero_invested },   // → +h if hero class stronger, −h if weaker
// hero folds to a raise:
Node::Terminal { hero_invested: villain_invested, villain_invested } // → ±villain's raise depending on hero class
```

A fold outcome must pay a class-independent amount: villain folding → hero wins villain's investment for **every** hero class; hero folding → hero loses his own investment for **every** class. As written, a *successful bluff with the weaker class loses money* (simulated: same fold, strong class +2, weak class −2), and folding to a raise can pay **positive** value. The `Node::Terminal` doc comment ("hero_wins_bb if hero class stronger") shows the strength-evaluation of folds is by design — the game model, not just a line, is wrong.
**Fix:** add a fold discriminator to the terminal (e.g. `Terminal { kind: Fold(Hero|Villain) | Showdown, hero_invested, villain_invested }`) and evaluate folds without `showdown_value`.

### C-4. Subgame: `pot_bb` is never awarded — the solver plays a zero-pot game with pot-derived sizes
**File:** `crates/cham-search/src/subgame.rs:170-185` — *verified*

```rust
let pot = hero_invested + villain_invested;         // river money only (starts at 0)
if hero.strength > villain.strength { villain_invested }        // win — pre-river pot missing
else if hero.strength < villain.strength { -hero_invested }
else { pot / 2.0 - hero_invested }
```

`hero_invested`/`villain_invested` start at 0 (`tree()` → `hero_node(0.0, 0.0)`) and only accumulate *river* money, but the win/lose payoffs never credit the pre-river pot `pot_bb` — which **is** used for bet sizing (line 93: `bet = f * (pot_bb + 2*villain_invested)`). Checked-through winners net 0; the win/lose swing is `h+v` instead of `P+h+v`. All bluff/value frequencies and pot-odds the solver derives are those of a different game.
**Fix:** attribute the pot consistently, e.g. win → `pot_bb/2 + villain_invested`, lose → `−(pot_bb/2 + hero_invested)`, split → `(villain_invested − hero_invested)/2` (per-class additive constants are immaterial, but the *differences across outcomes* must include the pot).

### C-5. The "independent validation oracle" returns the matrix max, not the game value
**File:** `crates/cham-search/src/oracle.rs:19-115` — *verified by hand-tracing matching pennies*

`solve_support` verifies only the **row** side (`no row beats v against q`, lines 98-105); the **column** side (no column best-responds below `v`) is never checked. Single-cell supports trivially pass, and `solve_matrix` keeps the max-`v` candidate (line 23: `v > *bv`). For the committed reference matrix `reference_matrix_2x2()` = `[[1,−1],[−1,1]]` (matching pennies), the function returns `(v = 1.0, p = [1,0], q = [1,0])` — the true Nash value is **0.0**. (Hand-trace: the full-support pair is rejected by a broken normalization row — `m[n_eq][c-1] = 1` encodes "q_last = 1", not "Σq = 1" — and the pure pair `(row 0, col 0)` passes the row-only check with v=1.)

The shipped gate `solver_matches_independent_oracles` cannot catch this because it only asserts `0 ≤ v ≤ 1`.
**Impact:** the "validated against independent oracles (never self-referential)" claim in the README is hollow — the oracle is wrong in the direction that *inflates* the solver's apparent value.
**Fix:** in `solve_support`, also verify the column side (`min_j Σ_i p_i·a[i][j] ≥ v − ε`); stop maximizing over pairs — any *verified* pair is an equilibrium; fix the normalization row to `Σq = 1` properly (add a full row of ones, not a single 1); pin the test to `v = 0.0` for matching pennies.

---

## 🟠 High

### H-1. EWM half-life formula uses `ln` instead of `pow` — all 13 tracker stats leave `[0,1]`
**File:** `crates/cham-agent/src/tracker.rs:64-72` — *verified*

```rust
fn lam() -> f64 { (0.5f64).ln() / HALF_LIFE }        // = ln(0.5)/60 ≈ −0.01155  (negative!)
fn ewm_update(&mut self, idx: usize, value: f64) {
    let lam = Self::lam();
    self.ewm[idx] = s * lam + value * (1.0 - lam);
}
```

The module doc and SPECS/07 §2 specify `s ← s·λ + x·(1−λ)` with `λ = 0.5^(1/60) ≈ 0.98851`. The code returns the **logarithm** of the intended λ (a negative number) and uses it as the decay weight. After one VPIP=0 hand: stat = `0.5·(−0.0116) ≈ −0.0058`; after a value=1 hand: `1.0116`. Every EWM stat (VPIP, PFR, 3bet, cbet, WTSD, aggression, …) goes negative / overshoots, and `shrunk_ewm()` feeds out-of-range features into the router.
**Fix:** `(0.5f64).powf(1.0 / HALF_LIFE)`.
*Related:* `crates/cham-agent/tests/agent.rs:117-124` (`tracker_ewm_math`) enshrines the bug — its comment says "≈ 0.494" but it asserts `0.5 · ln(0.5)/60 ≈ −0.0058`, which matches the buggy code. Fix the test to `0.5 * (0.5f64).powf(1.0/60.0)` or the regression will be reintroduced.

### H-2. Tracker models the wrong player in every seat-1 seating
**File:** `crates/cham-agent/src/pipeline.rs:476-481` — *verified*

```rust
fn on_hand_end(&mut self, ph: &PublicHistory, hero_net: i64) {
    self.tracker.observe_hand(ph, hero_net, 0);      // seat hardcoded to 0
```

`cham-eval/src/matcheng.rs` duplicate-matches every deal twice — hero at seat 0 **and** seat 1 (that's the whole point of duplicate matching). `observe_hand` computes `opp = 1 − hero_seat` (tracker.rs:83), so in seating B the tracker models **the agent itself**: VPIP/PFR/3bet/cbet counts observe the hero's own actions, `opp_won = nets[1]` is the hero's own net. Half of all observations feed a self-model into the drift shield and router weights.
**Fix:** record the hero seat at first use (or plumb it through `on_public_action`, which already sees `obs.player`) and pass it here.

### H-3. `EWM_FOLD_TO_3BET` can never record a fold (only ever written 0.0)
**File:** `crates/cham-agent/src/tracker.rs:200-202` — *verified*

```rust
if self.opp_faces_open > 0 && opp_3bet {
    self.ewm_update(EWM_FOLD_TO_3BET, 0.0);
}
```

This is the **only** write to the stat in the file: it fires when the *opponent 3-bets* (a lifetime-counter condition, not per-hand) and always writes **0.0**. The fold event (opponent faces our 3bet → 1.0 if they fold, 0.0 if they call/raise) is never recorded; the stat monotonically decays toward 0 and carries no information into router features. Two bugs in one line-gate: wrong event, wrong (cumulative) condition.
**Fix:** when the opponent *faces* our 3bet (the existing `facing_3bet` per-hand flag), update with 1.0 if they fold the rest of the preflop, else 0.0.

### H-4. `EWM_FOLD_VS_BET` uses a session-lifetime counter, not this hand's outcome
**File:** `crates/cham-agent/src/tracker.rs:214-217` (counter at 127-130) — *verified*

```rust
if opp_bet_faced && street >= 1 {
    let called = self.n_bets_faced_called > 0;   // lifetime counter!
    self.ewm_update(EWM_FOLD_VS_BET, if called { 0.0 } else { 1.0 });
}
```

`n_bets_faced_called` is incremented across **all** hands. After the first hand in which the opponent ever called a postflop bet, `called` is true forever → the stat receives 0.0 on every subsequent hand and can never register a fold again.
**Fix:** use a per-hand `opp_called_bet: bool` local, set in the action walk.

### H-5. Postflop tracker stats ignore opportunity gating (spec deviation) + WTSD corrupted by river folds
**Files:** `crates/cham-agent/src/tracker.rs:199, 213, 218, 221-231`; root cause `crates/cham-core/src/engine/history.rs:115` — *verified*

- 3bet/cbet/barrel/aggression EWMs are updated 0/1 on **every** hand, not per opportunity (spec: "3bet only facing opens, cbet only checked-to-as-aggressor"), diluting every stat; `n_faces_open` (line 176) always equals its own denominator — an invariant 1.0 ratio. River bets never count toward aggression.
- `PublicHistory::from` decides "showdown" as `board_len == 5`, so a **fold on the river** reveals both holes (violating the struct's own contract: "folded holes remain hidden" — an I9 leak) and the tracker counts it as WTSD / resets `hands_since_showdown`. `cham-core/tests/core.rs:422-431` pins the wrong proxy.
**Fix:** gate the EWMs on the per-hand opportunity flags (already computed); add a real `showdown: bool` to `HandHistory` (or detect "no fold on the last street") and fix the core test.

### H-6. Artifact integrity hash is computed but never compared — tampered policies load and play
**Files:** `crates/cham-blueprint/src/policy.rs:152-153,186` (hash computed, never compared), `crates/cham-agent/src/loader.rs:126-158` (checks only abstraction hash + depth), `crates/cham-agent/tests/agent.rs:503-509` — *verified*

`BlueprintPolicy::load` computes the file's blake3 `artifact_hash` and parses the provenance that contains the build-time hash, but nothing ever compares them; the contract test even ends in `assert!(err.is_ok() || err.is_err());` — a tautology. SPECS/07 §6 promises "hash mismatch … = hard error"; a hand-edited `policy.bin` (arbitrary action probabilities) loads silently and is then *bound into the ledger* by `guard::artifact_identity`.
**Fix:** in `load`, error when `computed != provenance.artifact_hash`; flip the test to mutate a row byte and assert the load fails.

### H-7. Live `play` never feeds the bot `on_public_action` — every infoset key diverges, silent fallback
**File:** `crates/cham-cli/src/cmd/play.rs:109-131` — *verified*

The interactive driver calls `bot.act(...)` but never `bot.on_public_action(...)`. `ChameleonAgent` records **opponent** actions into its canonical `ActionSeq` only via that hook (its own actions are recorded inside `act`). `matcheng.rs:87-94` documents the exact failure mode: *"Without this call the runtime seq only contains the hero's own actions, so every infoset key diverges from the trainer's and `BlueprintPolicy::strategy` returns None on ~65% of decisions."* So in human-facing play, the bot quietly plays the fallback (uniform/renormalized) strategy — the precise "silent-fallback" symptom `guard.rs` exists to prevent, on the most visible path. (`audit_buckets.rs` and `probe.rs` wire the hook correctly; `play.rs` is the odd one out.)
**Fix:** in the loop, before `state.apply`, call `bot.on_public_action(&Observables::view(&state, seat_of_actor), actor, a)`.

### H-8. `ExploitBayes` never builds the sampled opponent — the Bayesian arm trains against uniform random
**Files:** `crates/cham-blueprint/src/trainer.rs:265-301` (`_ => None` at 301), `traversal.rs:243-254` — *verified*

The ExploitBayes session block samples a hidden family and sets the belief bin (trainer.rs:221-243), but `iter_opp` is only constructed for `TrainMode::Exploit`. For ExploitBayes it stays `None` → the traversal consults `DummyOpponent`, whose `action_probs` returns `Err(NotProbabilistic)` → the fallback silently plays **uniform over ladder slots** (traversal.rs:246-253). Every belief bin therefore trains against the same random opponent — the Bayesian-game arm (EXP-005) is invalid while appearing to work.
**Fix:** build the concrete archetype agent for the sampled family per session block, exactly as the `Exploit` arm does per iteration.

### H-9. `resume_from` cannot continue training — it replays iterations 0..N on top of the restored table
**Files:** `crates/cham-blueprint/src/trainer.rs:209-212` (load), `219` (`for t in 0..cfg.iters`), `245, 261` — *verified*

There is no start-iteration offset. A resumed run re-executes the *same* per-iteration RNG streams (`child(seed, "iter{t}")`) and recomputes averaging weights against the *new* horizon — i.e., it re-applies updates for iterations that already happened, instead of extending training. The documented contract (SPECS/04 §9: "train 100 + resume 100 == train 200") is unimplementable as written, and the test that claims to pin it (`tests/blueprint.rs:566-612`) was neutered to compare two full runs (`let _ = resume;`).
**Fix:** add `start_iter` (or store last-iter in the snapshot), loop `start..start+iters`, derive per-iteration RNG labels and `w_t` from the global iteration index.

### H-10. SPRT likelihood ratio uses SE-of-the-mean instead of σ — LLR inflated by factor n, stopping guarantees void
**File:** `crates/cham-eval/src/stats.rs:139,143` — *verified*

```rust
let sd = se(diffs).max(1e-9);            // se() = σ/√n — the SEM, not σ
let llr = n * ((m - delta0).powi(2) - (m - delta1).powi(2)) / (2.0 * sd * sd);
```

Wald's LLR for a normal model is `n[(m−δ0)² − (m−δ1)²]/(2σ²)` with **per-observation** σ. Substituting `σ/√n` multiplies the LLR by n. Concretely (σ = 5000 mb, δ1 = 25 mb, α=0.05/β=0.10): a true +25 mb arm "AcceptH1"s after ~481 diffs instead of ~231k; a true-zero arm "AcceptH0"s after ~850 diffs instead of ~720k. The `ladder` chunked SPRT (checked every 250 deals) and `AbRunner` screening therefore stop on noise; the α/β error guarantees and every "sprt-stop saved N seatings" ledger claim are meaningless. The direction of the A/B boundaries is correct; only σ is wrong.
**Fix:** `let sd = variance(diffs).sqrt().max(1e-9);` (the private `variance()` already exists at line 107). The existing test can't catch this — it feeds constant streams (σ̂ = 0 → degenerate ±∞ LLR); add a hand-computed likelihood-ratio test per SPECS/10.

### H-11. The entire all-in-EV variance-reduction layer is dead code (`vr_factor` ≡ 1.0)
**File:** `crates/cham-eval/src/matcheng.rs:108-111` — *verified against the engine's terminal paths*

```rust
if allin_board.is_none() && state.stacks() == [0, 0] && !state.is_terminal() {
```

In `cham-core/src/engine/mod.rs`, **every** path that zeroes both stacks terminates the hand inside `apply_in_place` (short all-in call → line 465; street-completion all-in → line 476; blinds all-in → `State::new` line 185 — all call `all_in_runout_terminal()` which sets `hand_over`). So after `apply` returns, `stacks()==[0,0]` **implies** `is_terminal()` — the guard is unsatisfiable, `allin_board` is always `None`, and `allin_adjusted_net` always early-returns. The headline VR feature (ledger, A/B verdicts, SPECS/08 §4's `vr_factor ≥ 1.5` gate) silently does nothing; the preflop memo/GPU-table work has no live consumer. The committed test asserts `vr_factor >= 1.0` (trivially true) where its comment claims "> 1.0".
**Fix:** expose the pre-runout board from `ApplyOutcome` (e.g. `runout_board_len: Option<u8>` set just before `all_in_runout_terminal()`), capture it in `play_seating`, and make the test assert `> 1.0` on an all-in-heavy fixture.

### H-12. `FamilyB` bet sizing: `clamp` panics when `min_raise_to > max_raise_to`
**File:** `crates/cham-opponents/src/family_b.rs:141-142` — *verified*

```rust
let to = to.clamp(obs.min_raise_to.max(1), obs.max_raise_to);
```

`min_raise_to` postflop checked-to = `current_bet + last_full_raise` = 100; `max_raise_to` = the actor's remaining stack. An actor checked to with < 100 chips behind has **min > max**, and `i64::clamp` panics — crashing the whole match process on a valid input. The sibling implementation guards exactly this state (`archetype.rs:103-104`: `to.clamp(min.min(max), max)`, with a comment explaining the case) — FamilyB lacks it. Reachable: LAG has all cbet buckets = 1.0, so FamilyB bets whenever checked to.
**Fix:** `to.clamp(obs.min_raise_to.max(1).min(obs.max_raise_to), obs.max_raise_to)`.

### H-13. `PerturbedNash` tilt: leaked mass vanishes when the target action class is absent — distribution sums to 1−δ
**File:** `crates/cham-opponents/src/perturbed.rs:79-101` (with `sample` at 131-141) — *verified*

When the base policy contains **no** target-class action (e.g. `OverFold` facing a check — Fold isn't legal — or `OverRaise` vs an all-in), `target_mass == 0` and every action is scaled by `(1−new_mass)/(1−0) = (1−δ)`: the distribution sums to **1−δ** and the δ mass is assigned to nothing. Consequences: `action_probs` sums to 0.85 (corrupting one-sided-CFR reach products during exploit training), and `sample()` falls through to `dist.last()` for `u > 1−δ`, biasing ~15% of those decisions to the *last legal action*.
**Fix:** if no target-class action exists (or `target_mass == 0` with the class unrepresentable), return the base distribution unchanged.

### H-14. Restoring the tiny agent in `run-full-agent.sh` can never work — `mv` into an existing directory nests
**File:** `scripts/run-full-agent.sh:73-76` — *verified*

```bash
mv artifacts/agent artifacts/agent-tiny-backup     # 73
cp -a artifacts/agent-full artifacts/agent         # 74 — recreates artifacts/agent
cargo run ... ladder --fast --agent full           # 75
mv artifacts/agent-tiny-backup artifacts/agent     # 76 — dst EXISTS as a dir → moves src INTO it
```

POSIX/`mv` semantics: with dst an existing directory, line 76 nests `agent-tiny-backup/` inside the full agent (or fails ENOTEMPTY) instead of restoring. The full agent silently stays installed as `artifacts/agent` — the opposite of the script's stated goal — and a second run compounds the nesting. Ladder's exit code is also unchecked.
**Fix:** `rm -rf artifacts/agent && mv artifacts/agent-tiny-backup artifacts/agent`, gate on the ladder's rc.

---

## 🟡 Medium

### M-1. Subgame: villain's raise is capped by **hero's** remaining stack; the raise option silently vanishes on big bets
**File:** `crates/cham-search/src/subgame.rs:133-135` — *verified*
`let raise = (hero_bet * 2.2).min(self.stack_bb - hero_invested);` — villain (invested `villain_invested`, 0 at root) should be capped by his own stack. For bets `h > stack/3.2` the wrong cap makes `raise ≤ hero_bet`, so the raise action is dropped entirely instead of degenerating to a jam. **Fix:** `.min(self.stack_bb - villain_invested)` and clamp the raise-to at a jam instead of removing the action.

### M-2. Search cache key omits hero/villain class counts — cross-shape collision
**File:** `crates/cham-search/src/cache.rs:86-99` — *verified*
`hero.iter().chain(villain.iter())` hashes one concatenated stream; `bet_fracs.len()` is hashed but not the two list lengths, and `FxHasher` is length-free. `(hero=[A], villain=[B,C])` and `(hero=[A,B], villain=[C])` produce identical keys but different subgames. Rare in practice, real in long matches/`ab` sweeps. **Fix:** hash `hero.len()` and `villain.len()` before the stream.

### M-3. Router class-balance gate is `n < 2` rows — the spec requires 2k (2000)
**File:** `crates/cham-router/src/train.rs:41-47` — *verified*
The comment says "any class < 2k rows in A → refuse (spec: trainer refuses)" but the code checks `n < 2`: a class with 3 rows in a 2M-row dataset trains fine and can pass gates. The test suite only stays green *because* of this weakness. **Fix:** `if n < 2_000` (and update the synthetic test fixture).

### M-4. Router `gates_passed` omits the per-class recall gate (computed but not enforced)
**File:** `crates/cham-router/src/train.rs:104-106` — *verified*
`per_class_recall` is computed, then `gates_passed = top1 ≥ 0.80 && ece ≤ 0.15 && ece ≤ 0.15` — SPECS/05 §3 requires "per-class recall B-dev ≥ 0.70" too. A model that ignores one archetype ships as passing. **Fix:** `&& per_class_recall.iter().all(|&r| r >= 0.70)`.

### M-3b. (see M-3/M-4 above — router gates) — *listed together for fix ordering*

### M-5. Training-cache key omits semantic inputs → stale-artifact reuse
**File:** `crates/cham-blueprint/src/train_cache.rs:43-70` — *verified by cross-referencing the env-var behaviors*
The key hashes depth/iters/seed/discounts/mode/opponent/abstraction/threads — but not `bayes_session_block`, the env diagnostics that change training (`CHAM_RBP_THETA0`, `CHAM_EXPLORE_EPS`, `CHAM_FORCE_SEAT`, `CHAM_AVG_UNIFORM`), or whether the run was a `--resume-from` (a resumed table differs from a fresh one yet is stored under the fresh-train key). Same key → silently reused wrong artifact in A/B flows. **Fix:** hash all semantic inputs; bump `TRAIN_CACHE_VERSION`.

### M-6. `--threads` is dead: Hogwild/Snapbatch never spawn workers, but provenance records the requested count
**File:** `crates/cham-blueprint/src/trainer.rs:196-395` — no `std::thread` usage in the crate outside tests — *verified by reading the training loop*
`default_threads()` and the CLI's `--threads/--thread-mode hogwild|snapbatch` feed a number that is only serialized into provenance (and then hardcoded `"Deterministic"` at `crates/cham-cli/src/cmd/train_bp.rs:205` regardless — see L-9). Runs are correct but single-threaded while claiming `threads: N`. **Fix:** implement the worker pool (SPECS/04 §2/§4) or stop recording a thread count/mode that misrepresents the run.

### M-7. `BeliefBins` aliases distinct beliefs when `n_families > 4`; NaN edge at k=1
**File:** `crates/cham-blueprint/src/modes.rs:26-30, 65-74` — *verified*
`bin = (argmax*3 + tercile).min(COLD_BIN − 1)` clamps argmax ≥ 4 into bins 9..=11, colliding with family-3 bins; `conf` divides by `1 − 1/k` (NaN at k=1, masked by `f64::min`). Latent with the shipped 4 families. **Fix:** validate `n_families ∈ [2,4]` at construction.

### M-8. Policy-artifact truncation check is algebraically vacuous; corrupt files panic instead of erroring
**File:** `crates/cham-blueprint/src/policy.rs:193-200` — *verified*
```rust
let need = rows_off + (bytes.len() - rows_off);   // ≡ bytes.len() (or wraps to it)
if bytes.len() < need { ... "truncated" ... }     // never fires
```
When `rows_off > bytes.len()` the subtraction wraps so `need == bytes.len()` again; the branch is dead. A truncated/corrupt `policy.bin` then panics on slicing (key_at/strategy) instead of returning a clean error. **Fix:** `if bytes.len() < rows_off { return Err(...) }` plus per-row bounds checks.

### M-9. Snapshot/restore silently swallows serialization failures
**File:** `crates/cham-blueprint/src/table.rs:708-709` — *verified (grep)*
`postcard::to_allocvec(&snap).unwrap_or_default()` + `zstd::...unwrap_or_default()` — a failure produces an empty/garbage body and a misleading later error. **Fix:** propagate the error.

### M-10. Corrupt bayes artifact silently degrades to uniform play
**File:** `crates/cham-agent/src/loader.rs:149-158` — *verified*
`Err(_) => None` makes a *corrupt* bayes blueprint indistinguishable from an *absent* one; `bayes` routing mode then silently plays uniform-greedy. **Fix:** only swallow `NotFound`; hard-error on anything else; validate `routing == "bayes" ⇒ bayes.is_some()`.

### M-11. Hydrated subgames are never validated — a poisoned `river-cache.bin` bypasses "cache HIT never skips validation"
**File:** `crates/cham-search/src/cache_persist.rs:134-155` — *verified*
Any deserializable `Subgame` is inserted (weights not re-checked against sum-1/finiteness/non-empty ranges that `Subgame::build` enforces), contradicting the comment at `cache.rs:102-104`. **Fix:** re-run build-equivalent validation before insert.

### M-12. Ladder prints/ledger SE computed on the last 250-deal chunk while the mean covers the whole run
**File:** `crates/cham-cli/src/cmd/ladder.rs:163` (printout 299-301, ledger 306) — *verified*
`se = r.se_mb` is the chunk's SE; `mb = mean(&cum_profits)` covers all deals. For `--full` (25k deals) the reported `±` is ~10× too wide. **Fix:** `se = cham_eval::se(&cum_profits)`.

### M-13. Slumbot live client hardcodes empty credentials and `--resume` is a silent no-op
**Files:** `crates/cham-eval/src/slumbot.rs:95`; `crates/cham-cli/src/cmd/slumbot.rs:14-16` — *verified (grep)*
Login always posts `{"username": "", "password": ""}` — no flag/env in the repo supplies credentials, so `--real` can only fail (and the 4xx is then retried and swallowed, L-4). `--resume` prints "resuming" but the session is never persisted (SPECS/08 §5 requires persistence after every action). **Fix:** add credential flags/env with proper JSON escaping; implement session persistence or reject `--resume` loudly.

### M-14. Dataset label byte never validated → far-away panic; "A-or-B rows in C" refusal unimplemented
**File:** `crates/cham-router/src/dataset.rs:120-126` — *verified (grep)*
`decode_dataset` reads `label` without checking `<= 3`; training indexes `p[r.label as usize]` and panics far from the cause on a corrupt/hand-edited `.rbin`. **Fix:** reject `label > 3` at decode.

### M-15. Proof gates P-2/P-3/P-4 don't prove what they claim
**File:** `crates/cham-proofs/src/lib.rs` — *verified*
- **P-2 (`:436`)**: `passed: true` hardcoded — the "one-sided exploit training hits exact BR value" gate runs no training; it green-lights unconditionally.
- **P-3 (`:479-487`)**: "best single expert" is computed as `max over (signal, expert) of px·v` **then ÷2** (= 0.25·max v = 0.15625) instead of `max_k Σ_x px·v[x,k]` (= 0.25). The conclusion survives numerically, but the proof doesn't verify the claimed ordering.
- **P-4 (`:517-531`)**: the matrix `[[2.0, −0.5],[0,0]]` contradicts its own net-accounting comments (hero-bet/villain-fold should be +1, weak-called −2); with the correct matrix the equilibrium is degenerate (q = 0), not the claimed "call 80%". The asserted value 0.0 is unchanged either way — the gate passes coincidentally.
**Fix:** implement P-2 for real; accumulate per-expert EV then max for P-3; correct P-4's matrix/comments and assert p/q.

### M-16. The wgpu P7 correctness gate silently no-ops on adapter failure
**File:** `crates/cham-gpu/tests/consistency_eval7.rs:103-105` — *verified*
`Err(e) => eprintln!("... SKIP — {e}")` — if `WgpuContext::new` fails in CI, the "cross-platform correctness guarantee" (gpu.yml:110-113) vanishes while CI stays green. **Fix:** honor a `CHAM_GPU_REQUIRE_WGPU=1` env (set in gpu.yml) that turns the Err arm into a panic.

### M-17. `gpu-build` arg parsing: panics on trailing flag, silently defaults on bad ints
**File:** `crates/cham-gpu/src/bin/gpu-build.rs:69-88` — *verified*
`--kind` as the last arg → index-out-of-bounds panic; `--limit abc` → `unwrap_or(100)` silently builds a 100-board table marked `partial:true`. A typo'd full-build invocation writes a *partial* table with sample checks against only 100 boards. **Fix:** bounds-check `i+1`, error on parse failure (same pattern at lines 83, 87).

---

## 🔵 Low (condensed)

| # | Location | Issue |
|---|---|---|
| L-1 | `cham-engine/src/encoder.rs:84-98` | ActionSeq window of 8 silently drops the 9th+ action of a street → two different histories can hash to the same infoset key in raise wars. Widen the window or add an overflow marker byte. |
| L-2 | `cham-engine/src/tables.rs:115-120, 171-182` | `meta.river_eq_edges` length never validated: 0 edges → `u32::MAX` bin math in release; 1 edge → all river hands in one bucket. Require `len == river_eq_bins + 1` at load. |
| L-3 | `cham-engine/src/build.rs:512-519, 394-401` | `feature_runs == 0` (non-exhaustive) → 0/0 → NaN CDF features → garbage buckets; `quantile_sample < CDF_BINS` silently collapses edges to 1.0. Validate at entry. |
| L-4 | `cham-engine/src/canon.rs:114-122` | `TableView::parse` size check `n * 10` can overflow on a corrupt header → clean error becomes a later OOB panic. Use `checked_mul/checked_add`. |
| L-5 | `cham-engine/src/build.rs:230-281, 674-698` | `build_street` bins features with locally-derived quantile edges but `finalize_meta` recomputes them; callers using different params ship tables whose internal binning disagrees with runtime edges. Also `write_meta_struct` stores a blake3 that nothing verifies. |
| L-6 | `cham-blueprint/src/table.rs:662-674` | `width_of`/`slot_w` probes with linear `+1` while insert/find use double hashing → can report "absent" for present keys (currently only a test consumer; latent wrong-answer public API). Use `hash_step`. |
| L-7 | `cham-blueprint/src/traversal.rs:115-120, 348-357` | SnapBatch strat-lane auto-flush bypasses regret discounting (dormant at discount=1.0); `sample_index` underflows on empty input (external-caller risk). |
| L-8 | `cham-blueprint/src/trainer.rs:272-274` | Jitter redraw uses `child(jitter_seed, "jd")` (constant) instead of spec's `child(jitter_seed ^ iter, "jd")` — per-iteration jitter entropy halved; doesn't match SPECS/04 §3. |
| L-9 | `cham-cli/src/cmd/train_bp.rs:205` | Provenance hardcodes `thread_mode: "Deterministic"` regardless of the actual `--thread-mode` — an auditability lie bound into ledger gates. Write `format!("{thread_mode:?}")`. |
| L-10 | `cham-cli/src/cmd/probe.rs:67-69` | P1 verdict uses hardcoded "measured" constants (`coverage = 0.91`, `acc_b_dev = 0.84`) — the coverage half of the gate is always true. |
| L-11 | `cham-cli/src/main.rs:145-146`, `cmd/ab.rs:7` | `ab --clusters N` is accepted and ignored (`_clusters`); `session_cluster_ci` is never called — A/Bs claim session-level support but run plain deal-level paired CIs. |
| L-12 | `cham-eval/src/ingest.rs:22-25` | Multi-record aggregation: mean/se overwritten (last payload wins) while seatings sum — dashboard shows an inconsistent pair; reachable via `ab-{a}-{b}` run-name collisions. |
| L-13 | `cham-eval/src/slumbot.rs:76-84, 174-179` | 4xx responses retried 3× with status/body discarded; malformed login silently falls back to a fake `mock-token-1`. Fail fast on 4xx; error on missing token. |
| L-14 | `cham-eval/src/vr.rs:10-49`, `slumbot.rs:193-199` | `allin_ev_adjusted` returns `hero_net * eq` (not an EV replacement; correct fn exists below it — misuse trap, `pub`); `run_session(api, n, &[])` panics (`% 0`) on an empty action slice. |
| L-15 | `cham-eval/src/matcheng.rs` | `play`-adjacent: `meta_solve.rs:20-23` silently `continue`s unparseable ledger lines — a Nash mixture can be computed from a silently truncated matrix (contradicts the workspace "corruption = stop" policy). |
| L-16 | `cham-opponents/src/archetype.rs:46-56, 315-336` | All point-archetypes share one RNG stream (`session_seed = 0`, passed rng ignored): two point agents in one process draw perfectly correlated mixing randomness. Mix the archetype id/instance counter into the seed. |
| L-17 | `cham-opponents/src/factory.rs:187` | `Switcher` labeled in-family `"A"` regardless of inner specs — defeats the "never accidentally in-family" guarantee for `switch:famB:…`. |
| L-18 | `cham-agent/tests/agent.rs:271-283, 487-490` | "Sacred" deterministic-replay test never feeds villain actions (the feed hits the pipeline's own-player early-return, so it was never exercised); `reach_weighted_mixture_e2e` assertion short-circuits on `weights[4] > 0.0`. Also `preflop_gating_pinned` (cham-opponents) ends `let _ = chart;`. |
| L-19 | `cham-agent/src/pipeline.rs:410-425` | `mode.search` is entirely ignored (`search: None` hardcoded; no RiverSearcher field): enabling search in config yields silence, not search (masked because CLI always passes `enabled: false`). |
| L-20 | `cham-router/src/runtime.rs:124-134` | Changepoint likelihood multiplies every class equally → cancels in normalization; the drift shield works via the argmax-vote heuristic only, not the documented Bayesian recursion. |
| L-21 | `.github/workflows/bench.yml:104-113 vs 115-140, 133-140` | The `suite` dispatch input is parsed into an output and never used; all 8 compare-mode bench invocations mask failures with `\|\| true` (a crashing bench yields a green run + normal-looking table). |
| L-22 | `scripts/overnight-2026-09-25.sh:83-91` | MEM GUARD watches `pgrep -P $$ cargo` (the wrapper, tens of MB) — the actual `train-bp` child is never monitored; the 12 GB kill can't fire. Walk to cargo's child. |
| L-23 | `scripts/exp-015-grid.sh:12` | 60-cell grid runs in **debug** mode (`cargo run -q` without `--release`; every sibling script uses `--release`) — 10-50× slower, risking truncated results. |
| L-24 | `scripts/overnight-lbr-convergence.sh:19-26` | Advertised idempotency doesn't exist: `ROOT` is timestamped per invocation and there is no record-exists skip — reruns re-execute everything. |
| L-25 | `crates/cham-gpu/src/mtl.rs:87-103, 193-208` | EHS dispatches recompile MSL on every call (~100 ms each; `gpu-build --kind flop` ≈ 9 min of pure recompilation) — contradicts the module's own compile-once design. Perf only. |
| L-26 | `crates/cham-proofs/src/lib.rs:547-561` | `solve_2x2` degenerate branches return arbitrary `[0.5, 0.5]` mixes that are not equilibria (only `v` is consumed today; don't consume the strategies without fixing). |

---

## ✅ Verified-correct (reviewed, no findings)

Areas that were checked in depth and **passed** — worth recording so future reviews don't re-litigate them:

- **`cham-core` engine** (`engine/mod.rs`): fold refunds (uncalled street-bet difference), short all-in calls vs all-in bets, min-raise/full-raise tracking (all-in below min-raise doesn't reset the increment), street completion conditions, BB-first postflop ordering, split-pot odd chip to BB, `State::new` blind-cap edges, chip conservation under `play_random` fuzz invariants I2-I7.
- **`cham-core` evaluator** (`eval/mod.rs`): straight table incl. wheel; quads/trips/two-pair/pair kicker selection (incl. third-pair kickers and second-trip full-house partners); 7462-class dense scale assertion; multiset combinadic ↔ fast-table equivalence; `hole2_index`/`boardk_index` bijections; `equity_exact` enumeration; `equity_mc` rejection sampling.
- **`cham-core` misc**: `Range` [u64;21] ≥ 1326 bits, `class_id` triangular indexing (0..=168 verified by hand), `Deck::with_prefix` dedup, RNG child derivation, `Observables` projection (no leak surface), `is_legal` mirror.
- **`cham-engine`**: suit canonization orbits + idempotence, ladder legality clamps (proven legal vs engine incl. short stacks), key byte-stream composition, k-means determinism (revert-on-increase Lloyd).
- **`cham-eval`**: duplicate formula `(netA+netB)/2`, chips→mb conversions, seat rotation & chunk seeds, Holm step-down ordering, `z_for`/`required_seatings`, `preflop_key` bijection, ledger append-only + corruption=stop.
- **`cham-search`**: trigger boundary `>=` matches spec; budget accounting; concurrent touch/evict ghost entries benign.
- **`cham-router`**: `p^(1/T)` sharpening, Dirichlet mean/variance, per-hand weight freezing, ECE/top-1/recall formulas, no label leakage in dataset splits (trains on A, early-stops on B-dev).
- **`cham-gpu`**: shader math verified against the CPU evaluator — wheel→3, sentinel handling, prime products < 2⁶³, `nck` overflow-safety, workgroup 64 vs `div_ceil(64)` dispatch, buffer binding indices Rust↔WGSL, staging/map ordering, `reference.rs` denominators (990 / 45,540 / 1,070,190).
- **`cham-rec`**: byte-order envelope, append-only + corrupt-tail hard stop, monotonic seq resume.
- **`.cargo/config.toml`, `rust-toolchain.toml`, `deny.toml`, `clippy.toml`, `justfile`, `scripts/bench-summary.py`** and most one-off experiment scripts.

---

## Suggested fix order

1. **C-1…C-4 together** — they jointly determine every solved river strategy; after fixing, re-baseline the committed oracle JSONs and tighten `tests/search.rs` (assert `lbr_gap → 0`, fold-payoff invariance, `q`-side call matching).
2. **C-5 + M-15** — make the validation oracles actually independent (column-side check; P-2 real) before trusting any "validated" claim.
3. **H-1 + H-3 + H-4 + H-5** (tracker math) and **H-2** (seat) — the router features are built on these; then re-collect router data.
4. **H-10 + H-11** (stats/VR) — one-line σ fix + plumb the pre-runout board; then re-run any SPRT-stopped experiments.
5. **H-6, H-7, M-10** — restore the artifact-tamper contract and wire `play`'s feed.
6. **H-8, H-9, M-5, M-6** — training-mode correctness and provenance honesty.
7. Everything else by convenience; L-23 and L-22 first among infra (they distort experiment results).
