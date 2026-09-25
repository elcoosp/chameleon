# SPECS/03 — Crate `cham-opponents` — v2

All opponents implement `cham_core::Agent`. v2 changes driven by review A4 + B1:

- **Scripts are probability-oracles:** exact equity lookups (no MC), **fresh independent draws per decision** (no persistent per-hand uniforms), so `action_probs(obs)` is analytic and per-decision independent — a hard requirement for one-sided CFR reach products.
- **Out-of-family evaluation opponents** (the anti-circularity fix): perturbed-Nash bots, a second script implementation family, and a human-like noise wrapper.

---

## 1. Module tree

```
crates/cham-opponents/src/
├── lib.rs            (OpponentsError, facade)
├── params.rs         (ArchetypeParams, JitterSpec, defaults)
├── percentile.rs     (169-class equity chart — EXACT enumeration via equity_exact, seeded tie-breaks)
├── archetype.rs      (ArchetypeAgent — point + jittered; analytic action_probs)
├── family_b.rs       (FamilyBAgent — second, structurally different script family, B1)
├── perturbed.rs      (PerturbedNashAgent — tilted robust-blueprint bots, Ganzfried–Sandholm style)
├── noisy.rs          (NoisyAgent — human-like mistake wrapper, B1)
├── baselines.rs      (CallBot, RaiseBot, JamBot, RandomBot, FishBot)
├── drift.rs          (SwitcherBot)
├── factory.rs        (OpponentSpec parse/build/id)
└── session.rs        (SessionParams flight record)
```

## 2. `percentile.rs` — exact chart

169 classes ranked by **exact** head-to-head equity vs uniform (`equity_exact` over all villain combos, no MC), descending. Anchors golden test unchanged (±0.005). Build ~1 s.

## 3. `ArchetypeParams` — point defaults (unchanged values, cleaned prose)

Same table as v1 (nit/TAG/LAG/station rows for `open_raise, complete, call_open, three_bet, call_3bet, four_bet, iso_check, cbet_flop, barrel_turn, barrel_river, donk, check_raise, bluff_river, call_factor, value_bet, size_idx, trap`) with jitter ranges ±0.05–0.10 as v1. The v1 in-prose self-corrections are gone; the decision procedure below is the single source of truth.

**v2 decision-procedure changes (A4):**
1. All equity thresholds use **exact** chart/`equity_exact` values (postflop ehs = exact equity vs uniform on the current board — a table-adjacent computation, ~10 µs, only when the script must decide).
2. All randomness is **fresh per decision**: draw `u ~ U(0,1)` from a stream derived `child(session_seed, &format!("h{hand}.{street}.{idx}.{kind}"))` — no `hand_urs` persistence (v1's per-hand uniforms made within-hand decisions correlated, which breaks the reach-product interpretation of `action_probs`).
3. Because draws are independent and thresholds are exact, **the marginal probability of every action is analytic**: `P(action) = threshold expression evaluated at the exact equity` — `action_probs` returns exactly this (including the percentile-gating preflop paths, where thresholds are piecewise in `pct(h)` and known analytically).

Point scripts remain benchmark instruments, not GTO; `preflop_gating_pinned` constants (1.6/2.5/2.2/2) unchanged.

## 4. `ArchetypeAgent`

```rust
pub struct ArchetypeAgent { params: ArchetypeParams, arch: ArchetypeId, chart: PercentileChart }
impl ArchetypeAgent {
    pub fn point(arch: ArchetypeId, chart: &PercentileChart) -> Self;
    pub fn jittered(arch: ArchetypeId, session_seed: u64, chart: &PercentileChart) -> Self;
}
impl Agent for ArchetypeAgent {
    // act(): identical decision procedure to v1 (§3 numbered paths), with §3 changes above.
    // action_probs(): analytic marginal over the SAME procedure — mandatory, tested for
    //   consistency: sampled frequencies over 50k decisions match probs within 3 SE.
}
```

Sizing: point scripts bet fixed fractions (`size_idx` → 33/66/100% pot; preflop open 2.5 bb, 3bet 3 bb, 4bet 2.2×) — deterministic given the decision, hence in `action_probs` too.

## 5. Out-of-family opponents (B1 — the anti-circularity fix)

The router/specialists never train on these. Set-C evaluation is **out-of-family by construction**, not just unseen seeds:

```rust
/// Robust-blueprint policy tilted toward a documented leak (Ganzfried–Sandholm safety perturbations).
/// tilt ∈ {OverFold(δ), OverCall(δ), OverRaise(δ)}: strategy mass shifted toward the leak direction
/// by δ (default 0.15) on every infoset where the tilt applies. Built from a loaded robust blueprint
/// (injection via trait object; cham-opponents depends on cham-core only — the blueprint artifact is
/// passed in as a boxed strategy closure by cham-eval, keeping the DAG acyclic).
pub struct PerturbedNashAgent { /* tilt, δ, strategy source */ }

/// Second script family: structurally different implementation (preflop CHART lookup + postflop
/// decision-list rules, written independently of the threshold procedure). Same 4 archetype labels,
/// different code paths — tests that specialists learned styles, not our first implementation's tics.
pub struct FamilyBAgent { arch: ArchetypeId, params: ArchetypeParams, chart: PercentileChart }

/// Human-like noise wrapper: with prob ε makes a "mistake" (random legal action biased toward
/// callable/passive), occasionally switches archetype mid-session (bounded drift). ε default 0.10.
pub struct NoisyAgent { inner: Box<dyn Agent>, epsilon: f64, rng_label: &'static str }
```

`OpponentSpec` gains variants: `Perturbed { tilt, delta, bp_path }`, `FamilyB(ArchetypeId)`, `Noisy { inner: Box<OpponentSpec>, epsilon }`. id strings: `pnash:overfold:0.15`, `famB:tag`, `noisy:0.1:jitter:lag@9231`.

## 6. Baselines, drift, factory, session — changes only where noted

CallBot/RaiseBot/JamBot/RandomBot/FishBot/SwitcherBot as v1 (baselines return `NotProbabilistic` except where their policy is trivially analytic — CallBot gets an exact `action_probs`). Factory parsing/id round-trips extended for the new variants. `SessionParams` records family + params draws as v1; **out-of-family sessions are labeled `family: "B"|"PN"|"noise"` in the record** so the eval/protocol can never accidentally count them as in-family.

## 7. Tests (contractual)

| Test | Pins |
|---|---|
| `percentile_chart_anchors` | 20 golden anchors, exact-equity values |
| `preflop_gating_pinned` | as v1 |
| `archetype_point_determinism` | same (spec, seed) → identical decision logs, 5k hands |
| `archetype_jitter_ranges` | 200 draws in range, mean ≈ default |
| **`action_probs_analytic_consistency`** | per archetype: empirical action frequencies over 50k seeded decisions match `action_probs` within 3 SE per action |
| **`action_probs_independent_per_decision`** | conditional frequencies given identical observable state in different hands are equal (no within-hand memory) |
| `archetype_stats_sanity`, `legal_fallback_paths`, `switcher_drifts`, `factory_parse_roundtrip`, `station_never_folds_tp` | as v1 |
| **`family_b_divergence`** | FamilyB vs family-A same-label archetype: strategy KL > 0.05 on a 500-state sample (they must genuinely differ) |
| **`perturbed_tilt_math`** | tilt shifts strategy mass by δ ± 0.01 in the documented direction on sampled infosets |
| **`noisy_wrapper_stats`** | realized mistake rate ≈ ε ± 0.02; wrapped agent never returns illegal actions |

## 8. DoD

```
DoD — cham-opponents
[ ] cargo nextest run -p cham-opponents green (tests above, by name)
[ ] clippy + cargo deny clean; deps ⊆ {serde, rand} (+ dev)
[ ] All point/jitter scripts implement analytic action_probs; out-of-family trio (PerturbedNash,
    FamilyB, Noisy) buildable from OpponentSpec and labeled by family in session records
[ ] No equity_mc outside the chart builder (grep)
[ ] README present
```
