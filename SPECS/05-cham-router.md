# SPECS/05 — Crate `cham-router` — v2

The classifier + switching policy. v2 fixes the three math/structure errors of v1 (review A6, A7):

1. **Features are opponent-type-informative only** — tracker stats + opportunity counts, frozen at hand start. The circular self-confidence features (31/32) and opponent-blind hero features (25–30) are deleted; N = 20.
2. **Mixture weights are per-hand** (frozen within the hand — experts must stay coherent across streets), sharpened by `w ∝ p^(1/T)` (softmax over probabilities flattened, making a certain posterior weigh the right expert at 0.58 — G2 would fail by construction).
3. **The behavioral mixture is reach-weighted**: `σ_mix(a|i) ∝ Σ_k w_k·π_k(i)·σ_k(a|i)` — the Kuhn-correct form; v1's per-infoset average is a best response to nothing *and* mathematically the wrong behavioral strategy.

The binary dataset format, session-clustered splits (A / B-dev / B-test / C), and the pure-Rust softmax trainer carry over with edits.

---

## 1. Module tree

```
crates/cham-router/src/
├── lib.rs          (RouterError, facade)
├── features.rs     (20-dim contract — the ordered list; built by cham-agent at HAND START)
├── model.rs        (SoftmaxModel: forward, sgd; pure Rust ~200 LOC)
├── dataset.rs      (.rbin binary rows; session-clustered splits A/B-dev/B-test/C)
├── train.rs        (cross-entropy trainer, class balancing, early stop)
├── runtime.rs      (per-hand weights: sharpening, hand-hysteresis, shield)
└── metrics.rs      (top-1, per-class recall, ECE, confusion; family-aware)
```

## 2. `features.rs` — 20 dims, hand-frozen (order contractual; golden vector test)

Computed **once per hand, at hand start**, from the tracker state as of that moment (tracker updates only at hand ends — no within-hand leakage). One row per hand; the effective sample is **sessions**, and rows within a session are correlated — which is why splits and CIs are session-clustered.

| # | Feature | Range |
|---|---|---|
| 0 | hands_seen (log10(n+1)/3.5) | [0,1] maturity |
| 1–13 | EWM opponent stats (half-life 60 hands): vpip, pfr, three_bet, fold_to_3bet, call_3bet, cbet_flop, fold_to_cbet, barrel_turn, wtsd, aggression, showdown_won, fold_vs_bet, limp | [0,1] each, shrunk toward 0.5 by min(1, hands/150) |
| 14–17 | opportunity counts (log-scaled): faces_open, faces_3bet, cbet_opportunities, bets_faced | [0,1] |
| 18 | session EV trend z (clamp ±3)/3 | [−1,1] shield signal |
| 19 | hands-since-showdown (log/50) | [0,1] |

Deleted vs v1: street/pot/texture/ehs dims (no type signal), expert-confidence dims (circular + DAG violation).

## 3. `model.rs` / `train.rs`

`SoftmaxModel` (K=4, N=20) unchanged in shape; training: minibatch 512 SGD, lr 0.05 ×0.5/10 epochs, L2 1e-4, class-balanced losses, ≤ 100 epochs, early stop on **B-dev** loss plateau. Gates: top-1 **B-dev** ≥ 0.80; per-class recall B-dev ≥ 0.70; **ECE ≤ 0.15 on B-test AND on family-C sessions** (calibration, not accuracy, is the meaningful out-of-family metric — v1's "C top-1 ≥ 0.60" was vacuous because jitter draws are continuous, so every draw was already unseen).

## 4. `dataset.rs` — binary rows, session-clustered (review C8)

v1's JSONL (12M rows × 34 floats ≈ 5 GB text) is replaced:

```
.rbin = magic "CHMR" u32 | version u32 | n_rows u32 | n_features u32 (=20) |
        rows: [f32 × 20][u8 label][u16 session_id][u8 family]   // 84 B/row; 2M rows ≈ 170 MB
```

- One row per **hand** (uniformly sampled decision within the hand, seeded), features frozen at that hand's start; cap 2M rows (`collect` subsamples deterministically beyond that).
- **Splits by session**, not row: `set(seed) = A if h(seed)%10 < 6; B-dev if < 8; B-test if < 9; C-registered else` (FNV-1a). **C is out-of-family by construction** (families PN/FamilyB/Noisy — SPECS/03 §5) and is never a router training source.
- Trainer refuses: wrong magic/version/hash, A-or-B rows in the C file, any class < 2k rows in A, session leakage across splits (session ids checked disjoint).

## 5. `runtime.rs` — weights (the fixed math)

```rust
pub struct RouterRuntime { model: SoftmaxModel, temp: f64 /* sharpening T, default 0.7 (T<1) */,
                           alpha_hand: f64 /* hand-level hysteresis, default 0.3 */,
                           shield_beta: f64 /* 0.5 */, shield_z: f64 /* −1.5 */ }
impl RouterRuntime {
    /// Called ONCE PER HAND (hand start) with features; result FROZEN for the whole hand
    /// (v1 re-blended per decision — a 3-decision time constant is not hysteresis and changes
    /// experts mid-hand, making ranges incoherent across streets).
    ///
    ///   p      = model.forward(features)                 // posterior over 4 archetypes
    ///   w_inst = normalize( p_i^(1/T) )                  // SHARPENING: softmax over probabilities
    ///                                                    //   flattens — a certain posterior must
    ///                                                    //   give weight 1.0 to the right expert,
    ///                                                    //   p^(1/0.7) does exactly that
    ///   w      = α·w_inst + (1−α)·w_prev_hand            // hand-to-hand hysteresis
    ///   shield: if trend_z < shield_z: w = (1−β)·w + β·e_robust
    /// Returns [f64; 5]: four archetype weights + robust weight (robust weight starts 0; enters via
    /// confidence-gated fallback at decision time, SPECS/07 §4, and via the shield here).
    pub fn weights_for_hand(&mut self, features: &[f32; 20]) -> [f64; 5];
}
```

**Behavioral mixture (normative math, applied by cham-agent):** for infoset `i` with slots `A`,

```
π_k(i)   = Π_{hero's own earlier actions this hand} σ_k(a | i_pred)      // own-line reach under expert k
σ_mix(a|i) ∝ Σ_k w_k · π_k(i) · σ_k(a|i)                                 // if Σ_k w_k π_k(i) = 0:
                                                                         //   fall back to Σ_k w_k σ_k(a|i)
```

`π_k(i)` costs ≤ 8 strategy lookups per decision (hand length), all from the mmap'd inference artifacts. This is the behaviorally correct mixture (v1 averaged per-infoset and ignored own-reach — the Kuhn counterexample in the review). Uncovered experts contribute via the robust-substitution rule (unchanged); hard error if the robust expert is also uncovered → uniform legal + `fallback_uniform` record.

`argmax` mode remains an ablation arm (one-hot `w`). A third arm, `BayesPolicy` (the ExploitBayes blueprint, SPECS/04 §5), is loaded as a *policy* (not via runtime weights) for EXP-005.

## 6. Tests (contractual)

| Test | Pins |
|---|---|
| `features_golden_vector` | scripted tracker state → exact 20-dim vector (insta golden) |
| **`features_no_blueprint_inputs`** | structural: RouterFeatures construction takes no blueprint/policy arguments (DAG) |
| `softmax_forward_sums_one`, `sgd_learns_xorish` | as v1 |
| **`sharpening_math`** | p = (1,0,0,0) → w = (1,0,0,0) exactly; p = (0.7,0.1,0.1,0.1) → w₁ ≥ 0.85 at T=0.7 (v1's softmax gave 0.58 — the failing case is the regression test) |
| **`weights_frozen_per_hand`** | runtime returns identical weights for all decisions of one hand; updates only at hand boundaries |
| **`reach_weighted_mixture`** | Kuhn-toy instance: reach-weighted σ_mix matches hand-computed behavioral strategy; unweighted v1 form differs (documented) |
| `hysteresis_math` | hand-computed two-hand sequence incl. reset-per-session |
| `shield_triggers` | as v1, on trend_z |
| `fallback_redistribution` | uncovered expert → robust for that decision; weights untouched next hand |
| `dataset_binary_roundtrip` | .rbin write/read bit-identical; session-disjoint split enforcement; wrong-family-in-C refusal |
| `metrics_gates_negative` | bad model fails gates (exit 1), including ECE-on-B-test/family-C |
| `argmax_vs_mixture_vs_bayes` | three arms produce structurally distinct traces |

## 7. DoD

```
DoD — cham-router
[ ] cargo nextest run -p cham-router green (tests above, by name)
[ ] clippy + cargo deny clean; deps ⊆ {serde, serde_json, bincode 1.3, zstd} (+ dev)
[ ] Artifacts: model.bin + meta.json + metrics.json under artifacts/routers/<hash>/
[ ] Gates wired to exit codes; records per SPECS/12 (kind router_train)
[ ] README present
```
