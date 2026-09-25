# SPECS/07 — Crate `cham-agent` — v2

Composition: (encoder + router + experts + searcher + tracker) → one `Agent`, in config-driven modes. v2 deltas: tracker consumes **PublicHistory** (leak fix), router weights frozen per hand, the reach-weighted behavioral mixture lives here (SPECS/05 §5 formulas), modes updated (soft-bucket mode cut, BayesPolicy arm added), loader is depth-flexible (v1 required 200 bb while `play --depth 100` existed).

---

## 1. Module tree

```
crates/cham-agent/src/
├── lib.rs          (AgentError, facade)
├── tracker.rs      (Tracker: EWM stats + opportunity counts from PublicHistory; owns estimator formulas)
├── pipeline.rs     (ChameleonAgent)
├── modes.rs        (AgentMode config enum — canonical set below)
├── loader.rs       (artifact integrity, depth-flexible)
└── trace.rs        (decision traces → cham-rec)
```

## 2. `tracker.rs` — estimators (normative; router features depend on them)

EWM half-life 60 hands; opportunity-based denominators (3bet only facing opens, cbet only checked-to-as-aggressor, etc.); denominators exposed for router features 14–17; maturity shrink `min(1, hands/150)` toward 0.5 on all EWM stats; `session_ev_trend` z-score over the last 200 hands; `hands_since_showdown`. **Input: `&PublicHistory` only** — a unit test asserts the tracker compiles against nothing richer (type-level) and a runtime test asserts folded-hole secrecy through `on_hand_end` (I9 extends here).

## 3. `modes.rs` — canonical set (config files `config/agents/<name>.toml`, hashed into the ledger)

| Mode | routing | search | notes |
|---|---|---|---|
| `full` | Mixture | On (post-G4) | the product |
| `no-search` | Mixture | Off | |
| `argmax` | Argmax | On/Off pair `argmax-no-search` | ablation |
| `robust-only` | — (robust expert only) | Off | the baseline arm |
| `bayes` | BayesPolicy (ExploitBayes blueprint) | On/Off | EXP-005 arm |
| `fmbr` / `rnr` / `reach` | Mixture | On with solver pinned | EXP-002 arms |

Two modes differ only in these knobs — never code paths. v1's `soft-buckets-on` mode is **cut** (feature removed; EXP-007 stretch may reintroduce behind a flag).

## 4. `pipeline.rs` — `ChameleonAgent`

```rust
pub struct ChameleonAgent { encoder: Encoder, router: RouterRuntime, experts: Vec<BlueprintPolicy>, // 4 specialists
                            robust: BlueprintPolicy, bayes: Option<BlueprintPolicy>, // bayes arm optional
                            searcher: RiverSearcher, tracker: Tracker, mode: AgentMode, trace: Recorder }
impl ChameleonAgent { pub fn load(mode: AgentMode, artifact_dir: &Path, depth_bb: i64, rec: Recorder)
                        -> Result<Self, AgentError>; }
impl Agent for ChameleonAgent {
    fn act(&mut self, obs: &Observables<'_>, rng: &mut Rng) -> Action {
        // 1. hand_start? → feats = router features (tracker frozen at hand start); w[5] =
        //    router.weights_for_hand(feats); FROZEN for the hand (per-hand field, not per decision)
        // 2. per decision: σ_k per expert (uncovered ⇒ robust σ substitution)
        // 3. σ_mix(a|i) ∝ Σ_k w_k·π_k(i)·σ_k(a|i)  — reach-weighted behavioral mixture (SPECS/05 §5);
        //    π_k from this hand's own earlier actions under expert k; fallback plain average if
        //    Σ w_k π_k(i) = 0; confidence-gated fallback (visits) per decision, weights untouched
        // 4. mode dispatch: argmax → one-hot expert; robust-only → robust σ; bayes → bayes policy σ
        // 5. search (if enabled & trigger): FMBR/RNR/ReachGadget override per mode's solver
        // 6. sample a ~ σ (Mixture/robust) — argmax/bayes-greedy consume NO rng (replayability)
        // 7. real action via ladder.to_real; trace (§5); return
    }
    fn on_hand_end(&mut self, ph: &PublicHistory, hero_net: i64) { tracker.observe_hand(ph, hero_net); }
}
```

One instance plays one seat; mirror matches instantiate two independent agents (own trackers).

## 5. `trace.rs` — decision trace (schema contract; updated fields)

```json
{"kind":"decision","run":"r-123","hand_idx":17,"street":2,"slot":3,"action":"Bet:450",
 "weights_frozen":[0.05,0.62,0.18,0.15,0.0],"argmax_k":null,"search":{"solver":"Rnr0.9",
 "triggered":true,"source":"Solved","iters":400,"truncated":false,"lbr_gap_ours":0.031},
 "expert_visits":[312,1024,61,0],"fallback_used":true,"abstraction_hash":"..."}
```

`weights_frozen` is the per-hand weight vector (recorded on the first decision of each hand and unchanged after). Consumers as v1: router dataset (`collect`), dashboard, `chameleon trace`. Traces are the only persistence of weights — no side channels.

## 6. `loader.rs` — artifact integrity (depth-flexible, one place)

Loads: abstraction config + blake3 hash (TOML+artifacts), 4 specialists + robust (hash-checked), optional bayes blueprint, router model + metrics, mode config. **Depth rule (v2 fix): every blueprint's `depth_bb` must equal the requested play depth** (`play --depth 100` works with 100 bb artifacts; the Slumbot anchor uses the 200 bb artifact set). Produces the `agent_load` record (the dashboard's "who am I playing" card). Hash mismatch or metrics-missing = hard error, as v1.

## 7. Tests (contractual)

| Test | Pins |
|---|---|
| `tracker_ewm_math`, `tracker_opportunity_counts` | as v1, now over PublicHistory |
| **`tracker_leak_proof`** | I9 through the agent: folded-hole secrecy across 1k fuzzed hands incl. serialized tracker state |
| **`weights_frozen_within_hand`** | 3 decisions in one hand → identical trace weights; next hand → allowed to differ |
| **`reach_weighted_mixture_e2e`** | Kuhn-toy integration: pipeline's sampled action distribution matches the closed-form reach-weighted mixture |
| `pipeline_mode_matrix` | all 8 canonical modes produce their structural trace differences |
| `pipeline_deterministic_replay` | full mode, seeded, single-threaded: byte-identical traces (the sacred test) |
| `fallback_paths` | uncovered specialist + uncovered robust → uniform + record |
| `loader_hash_guards`, `loader_depth_flex` | tamper → error; 100 bb and 200 bb artifact sets both load; mixed-depth set → error |
| `mirror_match_smoke` | 200 hands, no panics, valid traces |
| **`search_mode_lockout`** | `full` with search On refuses to build without a G4 ledger reference (SPECS/06 §7) |
| `argmax_no_rng_consumption` | as v1 |
| `tracker_maturity_shrink` | as v1 formula |

## 8. DoD

```
DoD — cham-agent
[ ] cargo nextest run -p cham-agent green (tests above, by name)
[ ] clippy + cargo deny clean; deps ⊆ {serde, serde_json, rand}
[ ] All canonical modes load & play through cham-eval; soft-bucket symbols ABSENT (grep)
[ ] Traces validate against SPECS/12; dashboard consumers parse them
[ ] README present
```
