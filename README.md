# CHAMELEON

**Play the opponent, not the game.** A routed mixture of archetype-specialist blueprints for HUNL, trained and benchmarked on an M1 (16 GB, pure Rust).

Spec set v2: hardened after a hard external review that found ten fatal/thesis-level issues in v1's poker core (invalid MCCFR estimator, MC noise inside infoset keys, board-less river keys, circular router features, self-defeating mixture math, circular evaluation, self-referential solver validation, invented Slumbot dialect, inconsistent statistics). All resolved — see [`REVIEW-RESOLUTIONS.md`](docs/REVIEW-RESOLUTIONS.md) for the point-by-point map.

## What it is

- **4 specialist blueprints** (nit/TAG/LAG/station), one-sided ES-MCCFR vs *jittered, analytically-specified* archetype policies; robust (CFR+ self-play) fallback; **key-exact robust warm-start** per specialist.
- **An online router**: sharpened posterior (`p^(1/T)`), **per-hand frozen weights**, **reach-weighted behavioral mixture**, visit-based confidence fallback, drift shield. A Bayes belief-bin policy arm as the falsification test.
- **River real-time solving**: FMBR / RNR / reach-gadget, validated against independent oracles (never self-referential).
- **A flight-recorder eval loop**: proofs → probe → SPRT-guarded ladder → paired Holm-guarded A/B → Slumbot diagnostic → trimmed dashboard. Pure-function infoset keys (iso tables + river equity quantiles), Hogwild training with a deterministic mode, quantized mmap inference artifacts.

## Documents

| File | Read when |
|---|---|
| [`PLAN.md`](docs/PLAN.md) | Vision, v1 architecture, risks, legacy-pkr annex |
| [`REVIEW-RESOLUTIONS.md`](docs/REVIEW-RESOLUTIONS.md) | What the review demanded and where each fix landed |
| [`SPECS/00-conventions.md`](docs/SPECS/00-conventions.md) | **Always, first** |
| [`SPECS/01`–`09`, `12`](docs/SPECS/) | Building that crate (order: core → engine → opponents → blueprint → router → search → agent → eval → cli; rec anytime) |
| [`SPECS/10-experiment-protocol.md`](docs/SPECS/10-experiment-protocol.md) | Any benchmark/A-B interpretation |
| [`SPECS/11-milestones.md`](docs/SPECS/11-milestones.md) | What to build next; **M-1 proofs gate everything** |

## Build order (agent-facing)

`cham-proofs (M-1) → cham-core → cham-rec → cham-engine → cham-opponents → cham-blueprint → cham-router → cham-search → cham-agent → cham-eval → cham-cli`, gated M-1 → M5 (SPECS/11). Walking skeleton (M1) precedes scale (M2).

## Status

**Implemented.** The full workspace (11 crates, Rust edition 2024) builds warning-free
and is clippy-clean (`cargo clippy --workspace --all-targets -- -D warnings`). The test
suite (147 tests) is green, including the M-1 proofs gate:

- **P-1** ES-MCCFR on Kuhn converges to the Nash value (−0.0556 vs −1/18).
- **P-2** one-sided exploit training hits the exact best-response value vs a fixed caller.
- **P-3** the reach-weighted mixture beats the best single specialist and reaches ≥ 90 %
  of the exact Bayes-optimal EV on the hidden-type toy.
- **P-4** the FMBR/LP river machinery matches closed-form matrix-game solutions to 1e-6.

Performance (PERF-PLAN T1–T5, Apple M1, `target-cpu=native`): `eval_evaluate7`
single-pass + `evaluate7_batch` API · `State::apply_in_place` + hotspot-split
engine benches · `ThreadMode::Snapbatch` with thread-local delta buffers behind a
`RegretSink` (`Deterministic` bit-exact) · memoized fallback buckets + allocation-free
`Encoder::key_for` (single ladder derivation per visit — `mccfr_iter_200bb_tiny`
25.4 ms → 2.4 ms on the table-less path; `train-bp --mode robust --seed 7`
bit-identical infosets at ~1.4× wall-clock) · `train-bp --threads` (default:
available parallelism; 4 workers usually beats 8 on the M1).

Gates & guardrails (T6–T7): `verify --perf` now ENFORCES the P1–P6 gates from
criterion estimates (gate/threshold/measured/PASS|FAIL table, exit 1 on breach;
"run `just bench` first" when estimates are missing) instead of printing
thresholds. `ladder`/`probe`/`ab` refuse with exit 2 when a trained agent
(`full`, `argmax`, `robust-only`, …) is requested but `artifacts/agent` files
are missing, and fallback rates above 20% print a prominent WARNING that is
written into the ledger entry — silent-fallback mirror rows can no longer
masquerade as strength numbers. Absolute gate numbers must be re-baselined on
a quiet M1 (`just bench` under load oversubscribes and inflates every bench).

End-to-end through the binary (`target/release/chameleon`):
`verify --proofs --count-infosets` → GREEN · `train-buckets` (blake3-hashed artifacts) ·
`train-bp` (robust/exploit, quantized mmap policy artifacts) · `collect → train-router`
(G3 gates: top-1 ≥ 0.80, ECE ≤ 0.15) · `ladder --fast` (40k seatings vs the full pool
incl. out-of-family PN/FamB, ledger entry with CIs) · `ab` (paired, SPRT, promotion) ·
`slumbot` (mock; `--real` behind the verify-first gate) · `play` · `trace` · `dashboard`.

Scope notes (deliberate, recorded): the committed bucket artifacts are the **M1 tiny
abstraction** (`train-buckets --profile tiny`); the full k=300/200 tables are a
`--profile full` run away. `probe`'s Tier-1 wiring and the search lockout gate the
trained artifacts at M2/M4 — placeholder bands are marked in the code.


### GPU track (wgpu backend landed; cross-platform correctness green)

The Apple-Metal accelerator track now has a **cross-platform wgpu 30
backend** on top of the Metal-native one. Both are feature-gated and
inert by default (`cham-gpu` ships with `default = []`; nothing else in
the workspace activates either feature).

Correctness (P7 core, `crates/cham-gpu/tests/consistency_eval7.rs`):
- 1M/1M bit-exact vs CPU `evaluate7` on **Metal via wgpu** (macOS, local)
  and on **Metal-native** (macOS, `--features metal`).
- Linux CI runs the same 1M corpus against **Vulkan via llvmpipe**
  (software), gated in `.github/workflows/gpu.yml`.

Performance decision: the earlier "NO-GO" verdict from one-shot
measurements was **superseded by `docs/GPU-PLAN-AMENDMENTS.md` Amendment
001** — the original 10x bar was drawn against a CPU baseline ~13x
optimistic, and the local M1 Mini runs a concurrent GTO solver training
loop, producing a 5.66-11.85x spread across five identical trials. The
amended gates are `G_enum >= 3x` and `G_warm >= 2x` against a
**quiet-session** CPU reference; the CI macOS bench job (manual
`workflow_dispatch`) is where they get certified.

Amendment 002 made `wgpu` the target of record. The Metal-native kernel
stays as a stepping stone; `eval7.msl` is required to remain
WGSL-portable, and the WGSL implementation uses the u32-native table
layout + multiset rank (no u64 in the kernel).

## Dev tools

`cargo-nextest`, `cargo-llvm-cov`, `cargo-mutants`, `cargo-deny` (wired in the justfile); benchmarks via `criterion` with regression thresholds.


### GPU track

Two exact-EHS tables built (turn + flop), verify --gpu green.
