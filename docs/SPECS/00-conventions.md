# SPECS/00 — Workspace Conventions (READ FIRST, ALWAYS) — v1

This file is a contract. Every crate spec assumes it. The implementation agent re-reads this file at the start of every session.

**v1 changes (post-review):** closed dep whitelist extended (memmap2, bytemuck, arrayvec, blake3, criterion/proptest/insta as dev-deps); deterministic-vs-Hogwild threading contract; no-`Vec` hot-path rule; optimized test profiles; action canonicalization; artifact hashes via blake3; `cham-rec` is a leaf crate with its own spec (`SPECS/12`).

---

## 1. Workspace layout & toolchain

```
chameleon/                     (workspace root)
├── Cargo.toml                 workspace = ["crates/*"], resolver = "2"
├── rust-toolchain.toml        channel = "stable" (pin exact version at scaffold time)
├── .cargo/config.toml         [build] rustflags = ["-C", "target-cpu=apple-m1"]  (host is Apple M1)
├── justfile                   (all commands via `cargo run -q -p cham-cli --` — no bare binary)
├── clippy.toml
├── config/                    (TOML configs, see §7)
├── crates/
│   ├── cham-core/   ├── cham-engine/   ├── cham-opponents/
│   ├── cham-blueprint/ ├── cham-router/ ├── cham-search/
│   ├── cham-agent/  ├── cham-eval/     ├── cham-rec/     ├── cham-cli/
│   └── cham-proofs/                 (M-1 tiny-game proofs: Kuhn, Leduc, LP river, Bayes mixture)
└── artifacts/                 (git-ignored; created on first run)
```

**Crate dependency edges (acyclic, mandatory):**

```
cham-rec        → (leaf: depends on nothing but serde/serde_json)
cham-core       → (leaf: does NOT depend on cham-rec; core emits no records)
cham-engine     → cham-core
cham-opponents  → cham-core
cham-blueprint  → cham-core, cham-engine, cham-opponents
cham-router     → cham-core, cham-engine
cham-search     → cham-core, cham-engine
cham-agent      → cham-blueprint, cham-router, cham-search, cham-engine, cham-core
cham-eval       → cham-agent, cham-opponents, cham-core
cham-proofs     → (leaf: self-contained tiny games; imports nothing from the workspace)
cham-cli        → cham-eval, cham-blueprint, cham-router, cham-engine, cham-agent, cham-proofs
```

`cham-proofs` is deliberately disconnected: it re-implements micro-versions of the algorithms (≤ 1.5k LOC) so that proofs cannot pass by accident of shared code. It remains forever as a regression suite.

**Profiles (root Cargo.toml):**

```toml
[profile.dev]     opt-level = 3          # tests run the real algorithms; debug builds must not be glacial
[profile.test]    opt-level = 3
[profile.release] lto = "thin", codegen-units = 1
```

## 2. Dependency whitelist (CLOSED — adding anything else requires a human decision)

| Crate | Version | Used by | Notes |
|---|---|---|---|
| `rand` | 0.8 | all | trait `Rng`, `SeedableRng` |
| `rand_chacha` | 0.3 | all | ChaCha8Rng — THE rng of the project |
| `serde`, `serde_derive` | 1 | all | config + flight records |
| `serde_json` | 1 | rec, eval, cli, engine | JSONL |
| `bincode` | **1.3** (NOT 2.x) | blueprint, router | snapshots |
| `zstd` | 0.13 | blueprint, router | snapshot compression |
| `toml` | 0.8 | cli, engine, opponents, agent | configs |
| `clap` | 4 (derive) | cli | CLI |
| `thiserror` | 1 | all | error enums |
| `anyhow` | 1 | cli only | top-level error contexts |
| `rayon` | 1 | eval, engine | parallel matches / offline table builds |
| `ureq` | 2 (json) | eval | Slumbot client (sync HTTP) |
| `rustc-hash` | 1.1 | engine, blueprint, search | FxHasher for infoset keys (lookup only) |
| `memmap2` | 0.6 | blueprint, engine | read-only mmap of tables/bucket tables (safe API) |
| `bytemuck` | 1 | blueprint | `cast_slice` for zero-copy views (safe API) |
| `arrayvec` | 0.7 | core, engine, search | fixed-capacity hot-path collections |
| `blake3` | 1 | engine, blueprint, eval | artifact/provenance hashes (tamper-evident) |
| `holdem-hand-evaluator` | latest 0.x | core (conditional) | pure-Rust perfect-hash evaluator; **adopt only if it passes gate P1 on M1**, else implement in-crate bitmask evaluator behind the same `cham_core::eval` API |
| dev: `criterion` | 0.5 | benches/ | the benchmark harness (gates P1–P6 live here, not in ad-hoc tests) |
| dev: `proptest` | 1 | several | property tests |
| dev: `insta` | 1 | engine, agent | golden/snapshot tests |
| dev: `tempfile` | 3 | several | per-test dirs |

CLI dev-tools (not Cargo deps, wired into the justfile): `cargo-nextest`, `cargo-llvm-cov`, `cargo-mutants` (negative-test philosophy), `cargo-deny` (whitelist enforcement).

Forbidden: `nalgebra`, `ndarray`, `linfa`, `tch`, `burn`, `candle` (v1 stretch only, see SPECS/06 §8), `tokio`, `rusqlite`, `polars`, anything with a C toolchain.

## 3. Determinism & threading contract

1. **One RNG type everywhere:** `cham_core::rng::Rng = rand_chacha::ChaCha8Rng`. Every consumer takes `&mut Rng` explicitly. No `thread_rng`, no `SmallRng`, no `StdRng`.
2. **Seeds are u64 and recorded.** Every run/match/session/iteration-block logs its seed chain. A hand is replayable from `(match_seed, hand_index)`.
3. **No HashMap-iteration-order dependence** (unchanged): iterate → BTreeMap/Vec/sorted; `FxHashMap` is lookup-only.
4. **Floats:** `f64` EV/probabilities; `f32` table storage with the renormalization rule (SPECS/04 §6); no `f32` money; epsilon comparisons via `cham_core::consts`.
5. **Two threading modes, explicitly named — do not mix them:**
   - `Deterministic` (single-threaded): bit-identical results under a fixed seed. All determinism/resume/proof tests run in this mode. This is the mode of the "sacred" byte-identical test.
   - `Hogwild` (multi-threaded training): regret/strat rows stored as `AtomicU32` (relaxed-order CAS-add of bit patterns); results depend on interleaving and are **not** bit-reproducible — this is fine for training, which converges regardless of update order. Every training artifact records `thread_mode` and `threads`; only `Deterministic` runs may be resumed bit-identically.
   - The workspace remains `#![forbid(unsafe_code)]`; Hogwild uses safe atomics, never aliased `&mut`.
6. **Time is not logic:** wall-clock may gate quantity of work **in live play only** (`SearchBudget::WallClock`). Evaluation always uses `SearchBudget::Iterations` — byte-identical eval is sacred (SPECS/06 §5).
7. **The determinism test:** full play pipeline twice, same seed, byte-identical traces, single-threaded.

## 4. Units, money, and representation

| Quantity | Type | Convention |
|---|---|---|
| Chips, bets, pots, stacks | `i64` | 1 bb = 100 chips, SB = 50 |
| Development depth | — | **100 bb everywhere in v1** (training, ladder, A/B); 200 bb only for the Slumbot anchor run |
| EV, equity, probabilities | `f64` | equity ∈ [0,1]; EV in bb |
| Winrates | `f64` | canonical **mb/hand**; 1 bb/100 = 10 mb/hand |
| **"Hands" (normative definition)** | — | one **deal** = one shuffle; a duplicate pair = the same deal played twice with seats swapped = 2 seatings. All sample sizes and σ are quoted in **seatings**; σ is calibrated **per opponent** (SPECS/08 §4), never borrowed from CallBot |
| Cards | `Card(u8)` | idx 0..=51, rank=idx/4, suit=idx%4 |
| Hands (2 cards) | `Hand2(u16)` | canonical suit-isomorphic form (01 §2) |
| **Action (canonicalized)** | `enum Action { Fold, Check, Call, Bet { to: i64 }, Raise { to: i64 } }` | **`AllIn` is NOT an Action variant** — all-in is `Bet/Raise { to: stack_cap }` with `LegalAction::is_all_in = true`. Exactly one encoding exists. `to` = this street's total bet level after the action |

**Positional truth (unchanged):** HU preflop SB acts first; postflop BB acts first, every street. Unit-tested; do not "fix" it.

**Min-raise rule (unchanged):** full raise = `last_full_raise_size`; all-in below min-raise does not reopen action nor reset the increment. Fuzz-tested.

## 5. Error handling

Unchanged from v1: per-crate `thiserror` enums, no `unwrap`/`expect`/`panic!` outside tests, `anyhow` in CLI only, eager config validation, `unreachable!("cham-xxx: invariant I<n>")` with the invariant registry (I1–I7 in SPECS/01 §7; new: `I8` legal-mask/key-width agreement, `I9` public-history contains no hidden card).

## 6. Performance gates (criterion benches, release + `target-cpu=apple-m1`)

| ID | Gate | Threshold |
|---|---|---|
| P1 | 7-card evaluator | **≥ 100M evals/s** (v1's 1M was 100× too low and would pass an allocation-crippled design) |
| P2 | Engine `apply`/step | **≥ 10M actions/s** |
| P3a | Encode: flop/turn (table lookup path) | ≥ 1M encodes/s |
| P3b | Encode: river (exact equity enumeration path) | ≥ 100k encodes/s — the math: ~1000 `evaluate7` ≈ 10 µs at P1; do not "fix" this by making river keys lossy again |
| P4 | ES-MCCFR throughput @ **200 bb**, mid abstraction, single thread | **provisional ≥ 1.5k iters/s**; recalibrated by the M-1/M1 spike (SPECS/11) before any training budget is trusted — memory-bound reality is 300–1500 iters/s/thread |
| P5 | River solve (standard reference spot, 400 iters) | ≤ 250 ms wall in `WallClock` mode; `Iterations` mode is unbounded by definition |
| P6 | Full-pipeline match throughput (engine + policies + encode) | ≥ 60k seatings/min aggregate on 4 threads; recalibrated at M1 |
| P7 | `ladder --fast` end-to-end | ≤ 30 min |

Memory: **one training job at a time** (macOS uses 3–4 GB of the 16 GB; two 6 GB trainings swap). Training table ≤ 6 GB per job. Inference artifacts (strategy-only, quantized, mmap-shared): all five experts together ≤ 1.5 GB (SPECS/04 §8).

## 7. Configuration & hashing

TOML under `config/`; eager validation; no `#[serde(default)]` on gameplay-affecting fields. Two hash levels:

- `abstraction_hash` = **blake3(abstraction.toml bytes ‖ bucket-artifact bytes)** — the hash covers the centroids/bucket tables, not just the TOML, so retraining buckets invalidates dependent blueprints loudly instead of silently.
- `artifact_hash` = blake3 over each artifact file; recorded in provenance and ledger. FNV-1a remains in use **only** inside hot paths as a hash-mixing step for infoset keys (speed), never as a tamper-evidence mechanism.

## 8. Testing rules

Unchanged (tests by name are contractual; seeded properties print seed+inputs on failure; goldens via `insta`; properties via `proptest`), plus:

- Performance gates are **criterion benches** with regression thresholds, run by `just bench` and by `chameleon verify --perf` (which shells the bench binary). No gate lives in an `#[ignore]` test anymore.
- `cargo-mutants` runs on `cham-core::engine` and `cham-blueprint::traversal` in CI-locally; surviving mutants in traversal math are triaged before M2 (a mutant that survives the estimator tests means the tests are too weak).

## 9. Flight records

`cham-rec` is a leaf crate with its own spec: **`SPECS/12-cham-rec.md`** (schema, record-kind registry, flush/fsync rules). Every crate that records goes through it. No crate re-implements JSONL writing.

## 10. Definition-of-Done template

```
DoD — <crate>
[ ] cargo nextest run -p <crate> green (all listed tests present by name)
[ ] cargo clippy -p <crate> -- -D warnings
[ ] cargo deny check (whitelist + advisories)
[ ] All public API signatures match this spec exactly
[ ] Criterion benches assigned to this crate pass gates (just bench)
[ ] Flight records validate against SPECS/12 schema
[ ] crates/<crate>/README.md exists (≤ 30 lines)
```

## 11. Forbidden patterns (grep-listed in `just verify`)

- `thread_rng`, `SmallRng`, `StdRng`; `Instant::now()` outside `cham-search::budget` and `cham-rec` timestamps
- `unsafe` anywhere; `unwrap`/`expect` in `crates/*/src` (tests + `cham-cli/src/main.rs` excepted)
- `Vec` allocation inside per-decision hot paths (engine `apply`/`legal_actions`, encoder `key`, traversal, solver inner loops) — `ArrayVec`/fixed arrays/borrowed views only; enforced by review + `cargo-mutants` triage, and the P2/P3 gates
- HashMap iteration for decisions/outputs; `#[serde(default)]` on gameplay knobs; `mod utils` / `misc.rs`
- `AllIn` action variants (canonicalization, §4)
