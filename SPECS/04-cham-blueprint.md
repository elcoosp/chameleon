# SPECS/04 — Crate `cham-blueprint` — v2

The trainer. v2 is a **correctness rewrite** of the traversal (v1's estimator was invalid: double-weighted opponent sampling, no importance correction on the sampled hero action, an unsound "baseline"), plus: Hogwild-vs-deterministic threading, visit-counter confidence (v1's regret-ratio "confidence" saturated exactly when least converged), delayed linear averaging in both modes, regret-based pruning, one regret row per infoset, the Bayes belief-bin mode, **robust-warm-start replacing the depth curriculum**, and quantized mmap inference artifacts.

---

## 1. Module tree

```
crates/cham-blueprint/src/
├── lib.rs            (BlueprintError, facade)
├── table.rs          (RegretTable: arena + open addressing; Deterministic & Hogwild backends)
├── traversal.rs      (ES-MCCFR walk — CORRECT estimator; seat randomization; RBP)
├── modes.rs          (TrainMode::{Exploit, ExploitBayes, Robust})
├── trainer.rs        (driver: iterations, threads, snapshots, resume, records)
├── warmstart.rs      (robust-at-same-depth warm start; legacy depth ladder = experiment only)
├── policy.rs         (BlueprintPolicy + quantized inference artifact build/load)
├── lbr.rs            (local best response vs a policy — G9 metric + exploitation ceilings)
├── provenance.rs
└── bench.rs          (P4)
```

## 2. `table.rs` — layout & threading

```rust
pub struct RegretTable { /* open addressing, power-of-two, 0.70 load, doubles by rehash */ }
// Per-infoset row (width W = popcount of the key's legal mask — SPECS/02 §5c):
//   [f32 regret × W][f32 strat_sum × W][f32 avg_weight][u32 visits]     = 8W + 8 bytes
// ONE regret row per infoset in ALL modes: the key carries the position bit, so in Robust mode each
// seat's updates hit its own infoset rows (v1 stored two arrays — redundant and wrong-sized).
```

Two backends behind one API (00 §3.5):

- `Deterministic`: plain `Vec<u8>` arena, single-threaded, bit-identical resume. Used by tests, proofs, and small runs.
- `Hogwild`: per-slot `AtomicU32` (relaxed CAS-add of the bit pattern); multi-threaded training; results interleaving-dependent — recorded as such in provenance. `#![forbid(unsafe_code)]` holds.

**f32 growth rule (review A8):** `strat_sum` accumulates `w_t·σ`; when a row's `max|strat_sum|` or `avg_weight` exceeds 2²² at snapshot time, both are scaled by 1/2ᵏ to bring the max under 2²⁰ (v1 would saturate f32 at 2²⁴ ≈ our 16M-iteration budget). Normalized strategies are scale-invariant, so this is lossless; tested by `renorm_preserves_strategy`.

Snapshots: bincode + zstd, tmp+rename atomic, every `snapshot_every` iterations, plus `provenance.json`.

## 3. `modes.rs`

```rust
pub enum TrainMode {
    /// One-sided ES-MCCFR vs a scripted opponent distribution (point or jittered).
    Exploit { opponent: OpponentSpec, jitter_seed: u64 },
    /// ONE policy vs a per-session-sampled hidden type with a quantized belief bin in the key
    /// (the Bayesian-game formulation; EXP-005 arm). See §5.
    ExploitBayes { families: Vec<OpponentSpec>, obs_noise: f64, bins: BeliefBins },
    /// Two-sided CFR+ self-play (regret matching+, alternating updates, linear discounting).
    Robust,
}
```

Jitter: per-iteration opponent re-draw as v1 (`child(jitter_seed ^ iter, "jd")`). **Normative:** in Exploit modes the scripted policy is consumed *only* through `action_probs` (analytic, SPECS/03 §4) at opponent nodes; opponent regrets are never allocated (test `opponent_regrets_never_exist` retained).

## 4. `traversal.rs` — the **valid** estimator (review A3; the v1 pseudocode argued with itself and is deleted)

External-sampling MCCFR (Lanctot): **sample chance and opponent actions; ENUMERATE all hero actions.** No reach multipliers on hero updates, no importance weights, no baselines.

```
fn walk(state, hero_seat, w_t) -> f64 /* hero utility, bb */:
  if terminal            : return payoff(hero_seat) in bb
  if all_in_runout       : sample remaining board (chance), recurse
  if chance (street deal): sample one unseen board card, recurse
  p = to_act(); obs = state.view(p)

  if p != hero_seat:                       # Exploit/ExploitBayes opponent node
      probs = opponent.action_probs(obs)   # analytic, per-decision independent (SPECS/03 §4)
      a ~ probs                            # sampled ONCE — sampling IS the reach weighting;
      return walk(apply(a), hero_seat, w_t)#   multiplying by reach_opp here would double-count (v1 bug)

  # hero node (Exploit modes) or current player (Robust, hero_seat := p, both seats updated on
  # alternating iterations — each seat's key carries its own position bit)
  key = encoder.key(obs); e = table.entry_or_insert(key, W)
  if rbp_prune(e): return rbp_value_estimate(e)      # regret-based pruning, below
  sigma = regret_matching_plus(e.regrets)            # σ_i(a) ∝ max(R_a, 0); uniform if all ≤ 0
  for a in 0..W: v[a] = walk(apply(ladder.to_real(a)), hero_seat, w_t)
  v_bar = Σ_a sigma[a]·v[a]
  for a in 0..W: e.regret[a] += (v[a] − v_bar)       # NO reach factor, NO 1/σ, NO baseline
  e.strat_sum[a] += w_t·sigma[a]  (per a);  e.visits += 1
  return v_bar
```

- **Regret-based pruning (Pluribus trick):** skip the enumeration when `Σ_a max(R_a, 0) < θ_t`; play/sample per current σ and return the sampled value; θ_t = θ₀·δ^t with θ₀ = 10 bb, δ = 0.99 (both configurable). Pruned nodes still update `visits`. Gate: `rbp_matches_full` — pruned and unpruned runs agree on exploitability within tolerance.
- **Averaging weights `w_t`:** **delayed linear averaging in BOTH modes** — `w_t = max(0, t − D)`, `D = iters/4`; Robust mode additionally multiplies by linear discounting `γ^{T−t}`, γ = 0.9 (Linear CFR). v1's "current iterate" extraction (jittery under sampling) and its Exploit-mode no-averaging note are both gone (review B6).
- **Seat (v1 never said):** Exploit modes draw `hero_seat ~ {SB, BB}` uniformly **per iteration** from the iteration's child RNG — specialists must learn both seats. Robust mode alternates the updated seat per iteration.
- Robust-mode updates: on seat `p`'s iteration, `p`'s node rows update per the hero block; the opponent seat is sampled from its own current σ (self-play). Alternation + two-sidedness tested by `robust_two_sided_updates` (retained, adapted to one-row-per-infoset).
- Values are bb-normalized (chips/100) — depth-comparable for confidence and warm-start.
- Determinism: iteration `t` uses `child(train_seed, &format!("iter{t}"))`; single-threaded `Deterministic` mode is bit-identical; Hogwild is not, and says so.

## 5. `ExploitBayes` — the Bayesian-game arm (review A7-4)

One policy, hidden opponent type, belief in the key:

- Each **session block** (default 2k iterations) samples one true opponent family/spec from `families`.
- The hero observes only a **noisy summary**: `obs_noise` corrupts the type-sufficient statistics (Dirichlet noise around the true type frequencies, concentration scaled by observed hands).
- **`BeliefBins`:** posterior (Dirichlet-multinomial, uniform prior) quantized to 4×3 bins = (argmax type × confidence tercile) + a "cold" bin (n < 30 hands) = **13 bins**; the bin byte is part of the infoset key.
- This trains a single policy whose strategy conditions on quantized belief — the sound alternative to runtime mixing. Table ×13 warning: at reduced action tree (00 §4 reduced tree) and 100 bb this fits the 6 GB budget; `verify --count-infosets` gates it.

## 6. `policy.rs` — inference artifacts (v2: quantized, mmap, shared)

```rust
pub struct BlueprintPolicy { /* mmap'd quantized artifact + provenance */ }
impl BlueprintPolicy {
    pub fn build_artifact(&self, out: &Path) -> Result<(), BlueprintError>;
    /// Strategy-only, u8-quantized per action (2 decimals), visits u32→u16 (saturating), mmap'd
    /// READ-ONLY via memmap2 + bytemuck (safe API), shared across all worker threads/processes.
    /// Target: all five experts ≤ 1.5 GB combined (v1 would have needed ~30 GB loading 5 training
    /// tables). No regrets ship in inference artifacts.
    pub fn load(path: &Path, expected_abstraction_hash: u64) -> Result<Self, BlueprintError>;
    pub fn strategy(&self, obs: &Observables<'_>, enc: &Encoder) -> Option<Vec<f64>>; // None = uncovered
    /// Confidence (v2, review B5): visit-based. c(i) = visits / (visits + C0), C0 = 64.
    /// v1's R⁺/(Σ|R|+1) is DELETED — under CFR+ flooring it saturates toward 1 precisely when
    /// the infoset is least converged. The u32 visit counter rides in every row (§2).
    pub fn confidence(&self, obs: &Observables<'_>, enc: &Encoder) -> Option<f64>;
    pub fn provenance(&self) -> &ProvenanceRecord;
}
pub struct ProvenanceRecord { pub abstraction_hash: u64, pub artifact_hash: u64, pub mode: TrainModeTag,
    pub opponent_id: Option<String>, pub depth_bb: i64, pub iters: u64, pub train_seed: u64,
    pub thread_mode: ThreadMode, pub threads: u32, pub parent: Option<String>,
    pub wall_s: f64, pub infosets: usize, pub created_unix: i64 }
```

## 7. `warmstart.rs` — replace the curriculum (review B7)

The 10→20→40→80→200 bb ladder is **cut from the default recipe**: 10 bb strategy is jam/fold and is structurally unlike 200 bb; the √-scaled regret transfer was arbitrary. Default specialist recipe:

```rust
/// Warm-start from the ROBUST blueprint at the SAME depth — keys align exactly (same abstraction,
/// same encoder), so the transfer is key-exact: regret_dst = robust_regret (unscaled — shared game),
/// strat_sum_dst = robust_strat × 0.1 prior weight, visits_dst = robust_visits.
pub fn warmstart_from_robust(src: &BlueprintPolicy, dst_table: &mut RegretTable);
```

The depth-ladder idea survives only as **EXP-006** (pre-registered, SPECS/10 §8), gated by `ladder_beats_plain_warmstart` — if it can't beat the robust warm-start on the probe metric at matched compute, it dies in the ledger like any other failed hypothesis.

## 8. `lbr.rs` — local best response (the honest exploitability number)

```rust
/// Exact best response against a fixed policy ON OUR ABSTRACTION (tabular BR via full traversal of
/// the abstract tree; opponent = the given policy's action_probs). Returns LBR value in bb/hand.
/// Consumers: G9 (robust LBR), G1 ceilings (BR vs each point script = the exploitation ceiling),
/// probe Tier-1, frontier plot (SPECS/08 §8). Memory-bounded: runs on the abstraction config;
/// for ExploitBayes/big tables, LBR runs per belief bin.
pub fn lbr_vs(policy: &dyn Fn(&Observables<'_>) -> Vec<f64>, depth_bb: i64,
              cfg: &AbstractionConfig, enc: &Encoder) -> Result<LbrReport, BlueprintError>;
```

## 9. Tests (contractual)

| Test | Pins |
|---|---|
| `table_insert_lookup_rehash`, `table_snapshot_roundtrip` | as v1 (row layout updated) |
| `renorm_preserves_strategy` | scaling rows at 2²² leaves normalized strategies bit-close (< 1e-6) and unblocks growth |
| **`exploit_enumeration_estimator`** | vs CallBot on a 3-action toy subtree: per-action regret updates match hand-computed ES-MCCFR values for 100 iterations (the v1 double-weighting bug makes this test fail — keep both implementations in the test history comments) |
| `exploit_vs_constant_callbot` | 200k iters @100bb: LBR-verified EV ≥ +0.40 bb/hand and rising |
| `opponent_regrets_never_exist` | negative test, as v1 |
| **`seat_randomized`** | exploit run covers both seats (per-iteration draws; seat histogram printed) |
| `rm_plus_floors`, `robust_two_sided_updates` | adapted to one-row layout |
| **`rbp_matches_full`** | pruned vs unpruned: LBR within 5 mb/hand after equal iters; pruned wall-time < 40% of full on the 200bb pilot |
| **`delayed_averaging_monotone`** | averaged σ̄ is stabler than current iterate (variance of strategy distance over last 10% of iterations, avg < current) |
| `jitter_redraw_per_iter`, `determinism_same_seed` (Deterministic mode), `resume_continues_bitstream` | as v1, single-threaded only |
| **`hogwild_smoke`** | 8 threads × 50k iters: table consistent (row sums finite, no torn rows), infosets ≈ single-thread run's |
| **`warmstart_exact_keys`** | every robust key exists in dst post-transfer; strategy close to robust at t=0 |
| `warmstart_beats_cold` | robust warm-start reaches probe-metric parity in ≤ 40% of cold iters (replaces `curriculum_beats_cold`) |
| **`exploit_bayes_bins`** | belief bin from synthetic noisy stats: cold → bin 12; concentrated → argmax×tercile; key includes bin |
| **`lbr_known_values`** | LBR vs uniform in Kuhn-equivalent toy (via cham-proofs cross-check) matches closed form |
| `confidence_visits` | c = v/(v+64); uncovered None; saturation behavior sane at 10/100/10k visits |
| `mccfr_throughput_200bb` (criterion) | P4 (provisional ≥ 1.5k iters/s single-thread; recalibrated by the spike) |

## 10. DoD

```
DoD — cham-blueprint
[ ] cargo nextest run -p cham-blueprint green (tests above, by name)
[ ] clippy + cargo deny clean; deps ⊆ {serde, bincode 1.3, zstd, rustc-hash, memmap2, bytemuck, rand}
[ ] Traversal matches the §4 pseudocode exactly; no baseline/importance-weight code anywhere (grep)
[ ] Artifacts: training tables + quantized inference artifacts (5 experts ≤ 1.5 GB) + provenance.json
[ ] Flight records per SPECS/12; thread_mode recorded; one-training-at-a-time honored (00 §6)
[ ] cargo-mutants traversal triage done; P4 measured at 200bb and recorded in decisions.jsonl
[ ] README present
```
