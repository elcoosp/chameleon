# CHAMELEON — v2

**Play the opponent, not the game.** A routed mixture of archetype-specialist blueprints for HUNL, trained and benchmarked on an M1 (16 GB, pure Rust).

Spec set v2: hardened after a hard external review that found ten fatal/thesis-level issues in v1's poker core (invalid MCCFR estimator, MC noise inside infoset keys, board-less river keys, circular router features, self-defeating mixture math, circular evaluation, self-referential solver validation, invented Slumbot dialect, inconsistent statistics). All resolved — see [`REVIEW-RESOLUTIONS.md`](REVIEW-RESOLUTIONS.md) for the point-by-point map.

## What it is

- **4 specialist blueprints** (nit/TAG/LAG/station), one-sided ES-MCCFR vs *jittered, analytically-specified* archetype policies; robust (CFR+ self-play) fallback; **key-exact robust warm-start** per specialist.
- **An online router**: sharpened posterior (`p^(1/T)`), **per-hand frozen weights**, **reach-weighted behavioral mixture**, visit-based confidence fallback, drift shield. A Bayes belief-bin policy arm as the falsification test.
- **River real-time solving**: FMBR / RNR / reach-gadget, validated against independent oracles (never self-referential).
- **A flight-recorder eval loop**: proofs → probe → SPRT-guarded ladder → paired Holm-guarded A/B → Slumbot diagnostic → trimmed dashboard. Pure-function infoset keys (iso tables + river equity quantiles), Hogwild training with a deterministic mode, quantized mmap inference artifacts.

## Documents

| File | Read when |
|---|---|
| [`PLAN.md`](PLAN.md) | Vision, v2 architecture, risks, legacy-pkr annex |
| [`REVIEW-RESOLUTIONS.md`](REVIEW-RESOLUTIONS.md) | What the review demanded and where each fix landed |
| [`SPECS/00-conventions.md`](SPECS/00-conventions.md) | **Always, first** |
| [`SPECS/01`–`09`, `12`](SPECS/) | Building that crate (order: core → engine → opponents → blueprint → router → search → agent → eval → cli; rec anytime) |
| [`SPECS/10-experiment-protocol.md`](SPECS/10-experiment-protocol.md) | Any benchmark/A-B interpretation |
| [`SPECS/11-milestones.md`](SPECS/11-milestones.md) | What to build next; **M-1 proofs gate everything** |

## Build order (agent-facing)

`cham-proofs (M-1) → cham-core → cham-rec → cham-engine → cham-opponents → cham-blueprint → cham-router → cham-search → cham-agent → cham-eval → cham-cli`, gated M-1 → M5 (SPECS/11). Walking skeleton (M1) precedes scale (M2).

## Status

**Implemented.** The full workspace (11 crates, Rust edition 2024) builds warning-free
and is clippy-clean (`cargo clippy --workspace --all-targets -- -D warnings`). The test
suite (141 tests) is green, including the M-1 proofs gate:

- **P-1** ES-MCCFR on Kuhn converges to the Nash value (−0.0556 vs −1/18).
- **P-2** one-sided exploit training hits the exact best-response value vs a fixed caller.
- **P-3** the reach-weighted mixture beats the best single specialist and reaches ≥ 90 %
  of the exact Bayes-optimal EV on the hidden-type toy.
- **P-4** the FMBR/LP river machinery matches closed-form matrix-game solutions to 1e-6.

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

## Dev tools

`cargo-nextest`, `cargo-llvm-cov`, `cargo-mutants`, `cargo-deny` (wired in the justfile); benchmarks via `criterion` with regression thresholds.
