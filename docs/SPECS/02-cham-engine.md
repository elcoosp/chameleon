# SPECS/02 — Crate `cham-engine` — v1

The abstraction layer. **v1 core principle: infoset keys are pure functions of (hole, board, geometry) — zero Monte Carlo at encode time, zero in-key noise.** v1's MC-in-the-key was a silent-corruption bug and its P3 gate was unsatisfiable. Also: river keys are now board-aware (v1 made A♠K♠ on 2-3-4-5-7 and on A-Q-J-T-9 the same infoset — fatal), depth bands are replaced by SPR bands (fixing the v1 self-contradiction with cross-depth alignment), legal masks live in the key, and soft buckets are **cut**.

---

## 1. Module tree

```
crates/cham-engine/src/
├── lib.rs              (EngineError, facade)
├── config.rs           (AbstractionConfig, blake3 abstraction_hash incl. bucket artifacts)
├── canon_index.rs      (Waugh-style suit-isomorphism joint (hand,board) orbit indexing)
├── tables/
│   ├── build.rs        (OFFLINE builders: flop/turn CDF features → k-means → bucket tables; may use equity_mc)
│   ├── flop.bin        (canonical flop orbits × u16 bucket)   ~1.3M × 2B
│   ├── turn.bin        (canonical turn orbits × u16 bucket)   ~55M × 2B ≈ 110 MB, mmap'd
│   └── meta.json       (k, feature def, seeds, inertia, blake3)
├── buckets.rs          (runtime lookup: pure fn of (hole, board); river equity quantile)
├── action_ladder.rs    (ladders, raise cap, pseudo-harmonic off-tree mapping)
├── encoder.rs          (InfoSetKey: street|pos|spr_band|bucket|seq|legal_mask)
├── features.rs         (RouterFeatures — 20-dim, tracker-only, hand-frozen; built from PublicHistory stats)
└── bench.rs            (P3a/P3b)
```

## 2. `config.rs` — `AbstractionConfig` (v1)

```rust
pub struct AbstractionConfig {
    pub version: u32,
    pub buckets: { preflop: "exact169", flop_k: u32 /*300*/, turn_k: u32 /*200*/,
                   river_eq_bins: u32 /*64*/, river_texture_classes: u32 /*8*/ },
    pub ladder: { preflop_open_bb: Vec<f64>,        /* [2.2, 3.0] */
                  raise_fracs: Vec<f64>,            /* [0.5, 1.0] */
                  flop_bet_fracs: Vec<f64>,         /* [0.33, 0.75] — REDUCED TREE: ≤2 sizes + jam */
                  turn_bet_fracs: Vec<f64>,         /* [0.33, 0.75] */
                  river_bet_fracs: Vec<f64>,        /* [0.33, 0.66, 1.25] */
                  raises_per_street_cap: u32,       /* 2 — cap the raise war, keep tables tractable */
                  all_in_always: bool },
    pub spr_bands: Vec<f64>,                        /* log-spaced edges, default 16 bands over [0.3, 40] */
    pub seq_history_len: u32,                       /* 8, per street */
}
```

v1 knobs **removed**: `soft_bucket` (cut — with EMD/CDF buckets the benefit is negligible; revisit as EXP-007 stretch), `depth_bands` (replaced by `spr_bands`), `equity_mc_rollouts` (runtime has no MC).

`abstraction_hash` = **blake3(TOML ‖ flop.bin ‖ turn.bin ‖ meta.json)** — bucket artifacts are covered (v1 hashed only the TOML, so retraining buckets silently invalidated blueprints).

## 3. Offline bucket tables (built once, committed, mmap'd at runtime)

**Features (offline only):** the **river-equity CDF histogram** — for a (hand, board) at flop/turn, the distribution of *final river equity vs uniform* over runouts, quantized to **16 equal-mass bins** (computed by seeded MC in the OFFLINE builder only; frozen into the artifact). This is the potential-aware, EMD-compatible feature the review demands: for 1-D histograms, EMD = L1 distance between CDFs, so **k-means on the CDF vectors is a near-free EMD abstraction** — strictly stronger than v1's 2-D EHS/EHS².

**Canonical indexing (`canon_index.rs`):** Waugh-style suit-isomorphism: enumerate canonical (hand, board) orbits — flop ≈ 1.3M, turn ≈ 55M — and store each orbit's bucket id. Runtime lookup = canonicalize (bit tricks, O(1)) → mmap index → u16 bucket. **Buckets are a pure function of (hole, board).** The same real hand always lands in the same bucket, on every visit, forever.

- `tables/build.rs` (offline command `chameleon train-buckets`, release, rayon): sample/enumerate orbits → 16-bin CDF features → seeded k-means++ (flop k=300, turn k=200, ≤ 50 Lloyd iters, inertia delta < 1e-6) → write `.bin` + `meta.json` with seeds, inertia curve, blake3. Build time budget: minutes, not hours.
- **River needs no table:** exact equity vs uniform via `equity_exact` (~10 µs), then the **global quantile thresholds** (64 fixed edges, computed offline from the equity distribution over all canonical combos and committed in `meta.json`) assign the bin; multiplied by an 8-class board texture (paired², monotone, connected×high-card cross) → ≤ 512 river buckets. Per-board equity results are memoized in a small `ArrayVec`-backed cache during a single traversal visit; cross-visit caching is unnecessary (P3b: ≥ 100k river encodes/s — see 00 §6 for the arithmetic).

## 4. `action_ladder.rs` — ladders + **pseudo-harmonic off-tree mapping**

```rust
pub struct ActionLadder { /* cfg */ }
impl ActionLadder {
    /// Canonical slot order (facing no bet): [Check, Bet(f1..fk), Jam]; (facing bet): [Fold, Call, Raise(f1..fk), Jam].
    /// Bet "to" math as v1 (pot-after-call fractions, floor to chip, clamp [min_raise_to, max], dedupe).
    /// Raises per street capped at raises_per_street_cap; beyond the cap Raise slots are absent.
    pub fn slots(&self, obs: &Observables<'_>) -> ArrayVec<AbstractAction, 12>;
    pub fn to_real(&self, obs: &Observables<'_>, slot: usize) -> Action;
    /// Nearest-slot mapping — used ONLY for infoset key encoding (deterministic).
    pub fn nearest_slot(&self, obs: &Observables<'_>, a: Action) -> usize;
    /// PSEUDO-HARMONIC off-tree weights (v1, review D7): for a real off-tree size with pot fraction f_real,
    /// w_i ∝ 1/(ε + (f_real − f_i)²) over the top-2 nearest slots, ε = 0.01, normalized.
    /// Consumers: search range weighting (SPECS/06 §3) and AIVAT baselines (SPECS/08 §5).
    /// NEVER used to make keys nondeterministic.
    pub fn harmonic_weights(&self, obs: &Observables<'_>, a: Action) -> [(usize, f64); 2];
}
```

## 5. `encoder.rs` — the key (v1)

```rust
pub struct Encoder { cfg, ladder, flop: MmapTable, turn: MmapTable, river_meta: RiverMeta }
impl Encoder {
    pub fn from_config(cfg: AbstractionConfig, models_dir: &Path) -> Result<Encoder, EngineError>;
    pub fn abstraction_hash(&self) -> u64;   // blake3 over TOML + bucket artifacts

    /// Key byte stream (FNV-1a mixed, order fixed):
    ///   street u8 | position u8 | SPR band u8 | our_bucket u16 | legal_mask u32 |
    ///   canonicalized action seq (per street, window 8: actor u8, class u8, slot u8, size_bucket u8; boundary markers)
    /// Changes vs v1, each mandated by review:
    ///   (a) our_bucket at RIVER = texture_class(8) × eq_bin(64) — BOARD-AWARE (v1 used the raw combo
    ///       id with no board: fatal). Flop/turn buckets come from the iso tables (pure, board-aware by
    ///       construction). Preflop = 169 class ids.
    ///   (b) SPR band (log-spaced, 16 bands) replaces depth bands on ALL streets — pot context enters
    ///       turn/river keys (v1 aliased 3-bet pots with limped pots) AND cross-depth alignment still
    ///       holds: fractionally-identical sequences at different stack depths produce the same SPR,
    ///       hence the same key (test below). Position bit + per-infoset seat makes ONE regret row
    ///       per infoset sufficient in robust mode (SPECS/04 §6).
    ///   (c) legal_mask u32: bitmask over this state's canonical slots — included in the key because
    ///       dedupe/clamping changes the row width W; two states with different W are different
    ///       infosets, full stop (v1 keyed them together and rows mismatched — I8).
    ///   (d) NO Monte Carlo anywhere on this path.
    pub fn key(&self, state_view: &Observables<'_>) -> InfoSetKey;   // InfoSetKey(pub u64) ≠ 0 (OR high bit)
    pub fn n_slots(&self, state_view: &Observables<'_>) -> usize;    // == row width W, guaranteed by mask
    pub fn spr_band(&self, obs: &Observables<'_>) -> u8;
}
/// Router features (SPECS/05 §2): 20 dims, tracker-only, frozen at hand start.
/// Built by cham-agent from its Tracker + cham-opponents opportunity counts; cham-engine defines the
/// type and the serialization contract only (no blueprint access — the v1 circular features are gone).
pub struct RouterFeatures(pub [f32; 20]);
```

**Removed from v1:** `FeatureVector` with hero bucket/EHS/texture dims (25–30) and blueprint-confidence dims (31–32). The former carry no opponent-type signal; the latter were circular (router output feeding router input) and violated the crate DAG (`cham-engine` cannot read blueprints).

**Invariant I8:** `key()` and `n_slots()` must agree — the legal mask in the key determines W; a row written with W=6 and read expecting W=5 is a build-breaking bug, fuzz-tested.

## 6. Tests (contractual)

| Test | Pins |
|---|---|
| `abstraction_hash_covers_artifacts` | retrain buckets with same TOML → different hash (blake3 chain) |
| `canon_index_orbits` | flop orbit count ≈ 1.3M ±5k, turn ≈ 55M ±0.5M; canonicalization is a bijection on orbits |
| `bucket_pure_function` | same (hole, board) encoded 1000× across fresh Encoders → identical bucket, bit-for-bit (the anti-MC-noise test) |
| `river_bucket_board_aware` | AKs on 2-3-4-5-7 vs A-Q-J-T-9 → **different** buckets; equal-equity pairs → same bin |
| `river_eq_quantiles` | committed thresholds: bin populations within ±10% of uniform over a 200k-board sample |
| `kmeans_emd_determinism` | same seed → bit-equal centroids; inertia decreasing |
| `ladder_amounts_math`, `ladder_canonical_order`, `raise_cap_enforced` | sizing math, ordering, cap behavior incl. cap interacting with short stacks |
| `harmonic_weights_math` | hand-computed top-2 weights for 5 off-tree sizes; sums to 1 |
| `key_composition` | hand-built state → hand-computed key (insta golden) |
| `key_depth_alignment` | fractionally-identical (SPR-preserving) sequences at 20bb vs 40bb → **same key** (now consistent with SPR bands); fixed-size opens at both depths → different keys (SPR differs) |
| `key_legal_mask` | same bucket/seq, different legal set (short stack) → different keys; W == popcount(mask) (I8) |
| `no_mc_in_encode` | encode path greps clean of `equity_mc`/`Rng` (structural test) |
| `router_features_contract` | 20 dims, finite, documented ranges (SPECS/05 §2) |
| `encode_perf` (criterion) | P3a ≥ 1M/s (flop/turn), P3b ≥ 100k/s (river) |

## 7. DoD

```
DoD — cham-engine
[ ] cargo nextest run -p cham-engine green (tests above, by name)
[ ] clippy + cargo deny clean; deps ⊆ {serde, toml, rustc-hash, memmap2, blake3, rayon, rand} (+ dev)
[ ] Artifacts committed: flop.bin, turn.bin, meta.json (+ builder command train-buckets)
[ ] P3a/P3b pass; soft-bucket code ABSENT (grep: no `soft_bucket` symbols)
[ ] verify --count-infosets: sampling-based estimator with the M-1 spike caveat (no "analytic growth";
    rare-path counts come from the actual M1 pilot table growth, SPECS/11)
[ ] README present
```
