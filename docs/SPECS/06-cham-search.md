# SPECS/06 — Crate `cham-search` — v1

Inference-time river solving. v1 replaces v1's anchored CFR+ (an unsound hack that would **un-exploit us**: solving a Nash subgame against a blueprint-reach range discards exactly the specialist's edge — thin value vs stations — and v1's λ-anchor had no safety guarantee; review B2). The v1 solver family:

- **Fixed-Model Best Response (FMBR)** — vs known-ish opponents: pure BR against the prior strategy. Maximum exploitation.
- **Restricted Nash Response (RNR)** — opponent plays the model with probability p, freely with 1−p: a principled interpolation with a safety knob.
- **Reach-gadget safe solving** — for the robust arm when robustness matters more than edge.

Terminology fix (review B3): this is an **extensive-form river subgame** (betting tree, no chance nodes), not a "matrix game"; the cheap exact metric is the **local best-response (LBR) value gap**.

---

## 1. Module tree

```
crates/cham-search/src/
├── lib.rs          (SearchError, facade)
├── trigger.rs      (when to search)
├── subgame.rs      (range model, card removal, extensive-form tree construction)
├── solve_fmbr.rs   (fixed-model best response)
├── solve_rnr.rs    (restricted Nash response — CFR+ with the RNR opponent-constraint objective)
├── solve_reach.rs  (reach-gadget safe solve for the robust arm)
├── prior.rs        (blueprint strategy + visit-based confidence priors)
├── budget.rs       (the ONLY Instant::now() outside cham-rec)
└── oracle/         (independent validation oracles + reference spots, committed)
```

## 2. `trigger.rs`

```rust
pub struct SearchConfig { pub enabled: bool,
    pub solver: SolverChoice /* Fmbr | Rnr{p} | ReachGadget */,   // default Rnr{p: 0.9}; ablation arms pin the others
    pub budget: SearchBudget /* Iterations{iters: u32} | WallClock{ms: u64} */,
    pub min_pot_bb: f64 /* 8.0 */, pub river_only: bool /* true — turn search is v1 stretch */ }
pub enum SearchBudget { Iterations { iters: u32 }, WallClock { ms: u64 } }
pub fn should_search(obs: &Observables<'_>, cfg: &SearchConfig) -> bool;
```

**Budget rule (review C5):** `Iterations` in ALL evaluation paths — wall-clock truncation makes results depend on CPU load and breaks the byte-identical contract. `WallClock` exists only for live play (`play`, Slumbot), default 250 ms soft cap (P5 measured in WallClock mode; Iterations mode unbounded by definition).

## 3. `subgame.rs` — construction

Extensive-form river tree: check/bet-fracs/raise (≤ cap)/call/fold for both seats, `river_bet_fracs` + jam. Ranges:

- **Opponent prior:** routed blueprint's reach over its abstraction, weighted by **pseudo-harmonic** mapping of their actual off-tree sizes (SPECS/02 §4), then **visit-confidence flattening** (v1 signal): per-path product of `c(i) = visits/(visits+64)`; paths with product < 0.1 floored at 0.1× prior share — the solver knows where the blueprint is unvisited.
- **Our range:** blueprint-consistent hands for our line (actual hole always included), ≤ 64 combos; opponent ≤ 128 combos by prior weight.
- Payoffs: `evaluate7` showdown + fold values, bb-normalized; per-(board, pair) results memoized within the build.
- v1's "matrix game" framing and its exact-BR-proxy claim are replaced: exploitability of the solved extensive-form subgame is reported as the **LBR gap** per seat, computed exactly on the built tree (cheap: enumeration over ≤ 128×64 leaf paths per action).

## 4. Solvers

```rust
pub struct SolveResult { pub our_strategy: Vec<f64>, pub their_strategy: Vec<f64>, pub iters: u32,
                         pub truncated: bool, pub lbr_gap: (f64, f64) /* (ours, theirs) in bb */ }
pub fn solve(sg: &Subgame, prior: &PriorStrats, cfg: &SearchConfig) -> SolveResult;
```

- **FMBR:** best-respond to the prior opponent strategy on the tree (backward induction over the opponent's fixed policy) — pure exploitation, zero robustness. Used vs point/known scripts and as the RNR limit p→1.
- **RNR(p):** solve the game where the opponent node is replaced by: with probability p the opponent plays the prior strategy (their node becomes a chance node over prior actions), with 1−p they play freely (CFR+ updates on the free branch). Our resulting strategy is the best response to that mixture. p default 0.9; the RNR objective and its updates are exactly as in the literature (the review's pointer); implemented with the same CFR+ machinery (regret matching+, alternating, linear averaging).
- **Reach-gadget:** standard gadget construction on the subgame root ranges (opponent's off-tree reach compensated at the root) — the conservative arm for the robust policy; used when `solver = ReachGadget`.
- All solvers: deterministic under fixed iters (Iterations budget); priors seed initial strategies; **v1's λ-anchor penalty is deleted** (no safety guarantee; RNR's p is the principled knob).
- Integration contract with `cham-agent` as v1 (SearchOutcome, graceful degrade to blueprint action, illegal-action impossibility), plus `solver` id in the trace.

## 5. `oracle/` — independent validation (review B4; v1's oracle was self-referential)

The committed reference solutions must come from **independent** oracles, never "this solver at 10k iterations":

1. **Kuhn & Leduc equilibria** (in `cham-proofs`) — solver machinery computes known Nash values to 1e-6.
2. **Tiny LP-solvable river spots** — 2×2/3×2 action trees solved by an in-repo enumerative LP (closed-form matrix-game solver at fixed ranges) — the FMBR/RNR result must match.
3. **`postflop-solver` (AGPL) as a DEV-TIME oracle only**: a documented offline procedure compares our full-river subgame solutions against it on 10 committed spots; results (EV deltas) committed as JSON. **AGPL discipline: never linked, never shipped, never distributed with artifacts** — recorded in `cargo-deny` exclusions and the decisions ledger.

Acceptance (G5): mean EV loss vs independent oracles ≤ 10 mb/hand across the suite; strategy distance ≤ 0.12 where strategies are comparable. Suite provenance records the oracle used per spot.

## 6. Tests (contractual)

| Test | Pins |
|---|---|
| `trigger_config` | threshold logic, Iterations vs WallClock semantics |
| `subgame_card_removal` | ranges exclude dead cards; actual hole included; harmonic weights applied to off-tree sizes |
| `prior_confidence_flatten` | hand-computed weights; low-confidence path floored at 0.1× share |
| **`fmbr_exploits_station`** | scripted toy: FMBR extracts strictly more EV vs a station prior than blueprint play; RNR(0.9) between FMBR and blueprint; monotone in p |
| **`rnr_p_interpolation`** | EV(p) monotone non-decreasing in p toward FMBR limit; safety bound at p=0 matches reach-gadget within tolerance |
| **`reach_gadget_safety`** | gadget solve's LBR gap (ours) ≤ blueprint's LBR gap on 10 spots (the safety property that anchors were supposed to give) |
| `solver_matches_independent_oracles` | the §5 suite — the shipping gate |
| `solver_determinism_fixed_iters` | bit-identical under fixed iters regardless of machine speed |
| `budget_wallclock_only_live` | Iterations mode contains no time reads (structural); WallClock truncates gracefully |
| `illegal_action_never` | fuzz 10k triggered decisions through real states |
| `solve_perf_wallclock` (criterion) | P5 ≤ 250 ms on the standard spot at 400 iters |

## 7. Tests of integration consequence (owned here, consumed by cham-agent)

`search_off_by_default_until_g4` — the agent builder refuses `SearchMode::On` unless the config carries a passing G4 ledger reference (prevents shipping an unproven solver by accident).

## 8. Stretch (v1 roadmap, not v1)

Turn re-solving with depth-limited rollouts; a small DeepStack-style river/turn value net via `candle` (Metal) — the real SOTA path, explicitly out of v1 scope, requires M4 gates green with ≥ 2 days margin and a human green light.

## 9. DoD

```
DoD — cham-search
[ ] cargo nextest run -p cham-search green (tests above, by name)
[ ] clippy + cargo deny clean; deps ⊆ {rustc-hash, arrayvec}; Instant::now() ONLY in budget.rs
[ ] Three solvers (FMBR, RNR, ReachGadget) + independent-oracle suite committed with provenance
[ ] AGPL dev-oracle isolation verified (cargo-deny exclusion + no build-link)
[ ] Decision traces: kind search_decision {solver, triggered, truncated, iters, lbr_gap, source}
[ ] P5 pass; README present
```
