Got it — this dump fills in the trainer, CLI, and search crates plus the workspace rules (`forbid(unsafe_code)`, closed dependency whitelist, edition 2024 / Rust 1.85). Here is the remade README:

````markdown
<div align="center">
  <img src="docs/logo.png" alt="Chameleon Logo" width="200"/>
  <p>
    <strong>An audit-first heads-up no-limit Hold'em agent, written in Rust.</strong><br/>
    A multi-crate Cargo workspace covering the whole loop: ES-MCCFR / CFR+ blueprint training, opponent-conditioned expert routing, ledger-gated river solving, and an evaluation stack that refuses to report a number it cannot defend — self-verifying blake3 artifacts, iteration-budgeted deterministic eval, best-response telemetry with error bars, and a promotion ledger that binds every gate result to the exact artifacts that produced it.
  </p>
  <p>
    <img src="https://img.shields.io/badge/Rust-1.85%20%7C%202024-000000?style=flat-square&logo=rust" alt="Rust"/>
    <img src="https://img.shields.io/badge/License-MIT-blue?style=flat-square" alt="License MIT"/>
    <img src="https://img.shields.io/badge/Crates-12-6F4E37?style=flat-square" alt="Crates"/>
    <img src="https://img.shields.io/badge/unsafe-forbidden-success?style=flat-square" alt="Unsafe forbidden"/>
    <img src="https://img.shields.io/badge/Deps-Closed%20Whitelist-4B32C3?style=flat-square" alt="Closed dependency whitelist"/>
    <img src="https://img.shields.io/badge/Artifacts-blake3%20Self%2DVerifying-8B0000?style=flat-square" alt="Self-verifying artifacts"/>
    <img src="https://img.shields.io/badge/Verification-Independent%20Oracles-007ACC?style=flat-square" alt="Independent oracles"/>
    <img src="https://img.shields.io/badge/Eval-SPRT%20%2B%20Ledger-228B22?style=flat-square" alt="Eval"/>
    <img src="https://img.shields.io/badge/Determinism-Byte%2DIdentical%20Eval-FF4500?style=flat-square" alt="Determinism"/>
    <img src="https://img.shields.io/badge/GPU-Optional%20Metal%20%2F%20wgpu-708090?style=flat-square" alt="GPU optional"/>
  </p>
</div>

---

# Chameleon

> [!NOTE]
> Chameleon is research-stage. The workspace builds, tests and gates pass, and the full train → load → play → evaluate → promote loop is usable — but the router is trained on synthetic data, artifact formats are still stabilising, and several known gaps are tracked openly. See [Project Status](#project-status).

---

## Table of Contents

- [Why Chameleon](#why-chameleon)
- [Features](#features)
- [Architecture](#architecture)
- [Getting Started](#getting-started)
- [Usage](#usage)
- [Configuration](#configuration)
- [Artifacts and Integrity](#artifacts-and-integrity)
- [Development](#development)
- [Project Status](#project-status)

---

## Why Chameleon

Every claim the code makes about itself can be checked from files alone.

- **No `unsafe`, by workspace decree.** `unsafe_code = "forbid"` is a workspace-level lint, not a convention. Dependency FFI (mmap, Metal) is scoped and justified in the root `Cargo.toml` (decisions D-001, D-008); the crates' own code is safe Rust.
- **A closed dependency whitelist.** Every dependency is listed and justified in the workspace manifest (SPECS/00 §2). Unmaintained `bincode` was already swapped for `postcard` after RUSTSEC-2025-0141. Even test tooling respects the rule: the CLI integration tests spawn the real binary via `CARGO_BIN_EXE` with std only — no `assert_cmd`.
- **Determinism you can pin.** All evaluation runs iteration budgets, not wall clocks — the only `Instant::now()` outside the trace layer lives in `cham-search`'s budget module. The `Deterministic` regret-table backend is bit-identical; every deal gets a seeded child RNG; a cached subgame solve is bit-equal to a fresh one, by test.
- **Artifacts that prove themselves.** `policy.bin` embeds its provenance and a blake3 payload hash; loading recomputes the hash and refuses a mismatch. The loader additionally enforces abstraction-hash agreement, blueprint-depth uniformity, and a pre-allocation memory budget.
- **Numbers that refuse to lie.** Evaluation commands refuse to run a trained agent without its artifacts; a fallback rate above 20% and a router that sends ≥90% of picks to one expert are surfaced as loud warnings, not silent weakness. Best-response reports carry a held-out standard error, and deltas below ~2 SE are treated as noise.
- **Every gate is auditable.** The ledger records each A/B, ladder, and promotion as JSONL, bound to the blake3 identity of the exact bundle that produced it. A promotion must first survive a shadow gauntlet against recent champions.

If "can I trust this number?" is the question you ask most, Chameleon is built for you.

---

## Features

### Blueprint training (`cham-blueprint`)

- **Correct ES-MCCFR** — chance and opponent actions sampled, hero actions enumerated; no reach multipliers, no importance weights, no baselines. Regret-based pruning (Pluribus θ-schedule), delayed linear averaging, per-iteration seat randomization, bb-normalized values.
- **Three training modes** — `Exploit` (one-sided vs scripted opponents, or against a frozen snapshot of a previous policy for self-exploit), `ExploitBayes` (one policy vs a hidden opponent type, quantized into 13 belief bins), and `Robust` (two-sided CFR+ self-play with regret matching+ and discounting).
- **Two threading backends** — `Deterministic` (bit-identical across runs) and `Hogwild` (atomic, throughput-first). Plus key-exact robust warm-starting for depth ladders.

### Agent (`cham-agent`)

- **Seven routing modes** — `mixture`, `argmax`, `sample-expert`, `hedged` (argmax once the top weight clears a confidence threshold), `bounded` (per-hand exploitation commitment with the sequence-form bound ε ≤ (1−λ)·ε_robust + λ·ε_expert), `robust-only`, and `bayes`.
- **Reach-weighted behavioral mixture** — `σ_mix(a|i) ∝ Σ_k w_k·π_k(i)·σ_k(a|i)` over four specialist blueprints plus a robust policy, with confidence-gated fallback and drop-and-renormalize semantics on uncovered tiers.
- **Leak-proof opponent modeling by construction** — the tracker consumes `&PublicHistory` only; hidden information is unreachable at the type level (invariant I9). Duplicate-seat matches model the opposite seat, never a hard-coded one.

### River search (`cham-search`)

- **Three solvers behind one `solve()`** — FMBR (maximum exploitation vs the prior), RNR(p) (restricted Nash response — the principled safety knob), and ReachGadget (the conservative arm), over extensive-form strength-class subgames with pseudo-harmonic off-tree mapping and card removal.
- **Independent oracles** — support-enumeration Nash solvers for matrix games plus Kuhn/Leduc and LP reference spots pin the machinery; the AGPL `postflop-solver` is a dev-time-only documentation procedure, never linked (cargo-deny tripwire).
- **Content-keyed caching** — an LRU L1 (2,048 entries) memoizes subgame builds, persists to `river-cache.bin` across sessions, and re-validates every hydrated entry.
- **O(n) counterfactual-value kernels** with card removal, the groundwork for vector-form solving.

### Evaluation and promotion (`cham-cli`, `cham-eval`)

- **Paired A/B with SPRT** (δ₀ = 0, δ₁ = 25 mb, α = 0.05, β = 0.10), per-opponent confidence intervals, and variance-reduction telemetry on every printout.
- **Guardrails everywhere** — untrained-artifact refusal, fallback-rate and router-degeneracy warnings, and a stale-binary protection test: every routable agent name must be recognized, or the tools stop rather than silently measure a baseline.
- **Hash-bound ledger** — every gate result is appended to `artifacts/ledger/ledger.jsonl` with the blake3 identity of both arms' bundles.
- **Promotion ladder** — A/B pass → shadow gauntlet against recent champions → promote → new shadow snapshot.
- **Honest router data** — `collect --real` plays instrumented matches against the nit/tag/lag/station archetypes, alternates the hero seat per session, and emits opponent-only feature vectors (10, 11, or 19 dims) that are pure functions of the opponent's behaviour.

---

## Architecture

| Crate | Description |
|-------|-------------|
| `cham-core` | Poker primitives: cards, deck, seeded RNG with child derivation, the engine (`State`, `Action`, streets), `HandHistory`/`PublicHistory`, `Observables::view`, the `Agent` trait. |
| `cham-engine` | Abstraction configs and validation, the `Encoder` (infoset keys via `ActionSeq`, action ladder, slots), bucket-quality audit. |
| `cham-opponents` | Scripted pool: percentile-calibrated archetypes (nit/tag/lag/station), simple baselines (`callbot`, `raisebot`, `jamfix`, `fish`, `random`, `uniform`), `OpponentSpec` parsing. |
| `cham-rec` | Decision and load records — traces are the only persistence of weights (SPECS/12). |
| `cham-blueprint` | The trainer: ES-MCCFR / CFR+, regret tables, quantized artifacts, local best response (SPECS/04). |
| `cham-router` | Opponent-conditioned routing: `SoftmaxModel`, `RouterRuntime`, the binary dataset format, honest feature vectors. |
| `cham-search` | Inference-time river solving: subgames, priors, solvers, cache, oracles (SPECS/06). |
| `cham-agent` | Composition: tracker + router + experts + searcher → one `Agent` (SPECS/07). |
| `cham-eval` | A/B runner with SPRT, the JSONL ledger, ingest, dashboard rendering. |
| `cham-gpu` | Feature-gated GPU kernels: Metal FFI (macOS) and wgpu (Vulkan/Metal/DX12). CPU is always the fallback. |
| `cham-proofs` | Verification support backing the `verify` tooling (see the crate's docs for scope). |
| `cham-cli` | The `chameleon` binary — orchestration only (SPECS/09); no CFR, tracker, or stats code lives here. |

### Data flow

```
      train-buckets → Encoder            train-bp → regrets → policy.bin ×5
      train-router → honest features → router.bin
                    │
                    ▼   self-verifying artifacts (blake3 + provenance)
           ┌──────────────────┐
           │      loader      │  blake3 · abstraction hash · depth · memory budget
           └────────┬─────────┘
                    ▼
┌──────────────┐  ┌────────────────────────────────┐  ┌─────────────────────┐
│  opponents   │─▶│         ChameleonAgent         │─▶│  cham-rec traces    │
│ archetypes · │  │ tracker ← &PublicHistory (I9)  │  │ (JSONL, SPECS/12)   │
│  Slumbot     │  │ router → w[5], frozen per hand │  └─────────────────────┘
└──────────────┘  │ σ_mix ∝ Σ w_k·π_k·σ_k          │
                  │ river search (G4 ledger-gated, │
                  │ content-keyed LRU cache)       │
                  └───────────────┬────────────────┘
                                  ▼
┌─────────────────────────────────────────────────────────────┐
│  play · probe · ladder · ab                                 │
│  guards refuse untrained/stale runs · SPRT · paired CIs ·   │
│  vr_factor · fallback & router-degeneracy warnings          │
└──────────────────────────┬──────────────────────────────────┘
                           ▼
      ledger.jsonl (hash-bound gates) → shadow gauntlet → promote
                           │
                           ▼
                  dashboard (static HTML)
```

The in-repo `SPECS/` documents (00–12) are the design source of truth; code comments cite them by section, and `docs/plans/` holds the dated design notes behind recent changes.

---

## Getting Started

### Prerequisites

- **Rust** 1.85 or newer (install via [rustup](https://rustup.rs/)). The workspace uses the 2024 edition.
- **Git** — to clone the workspace.
- Nothing else. Training opponents are in-repo, artifacts are built locally, and no subcommand needs the network except `slumbot`.

### From source

```bash
git clone https://github.com/elcoosp/chameleon.git
cd chameleon
cargo build --release
```

The binary lands at `./target/release/chameleon`.

### First run

```bash
# 1. Integrity check
./target/release/chameleon verify

# 2. Train a tiny end-to-end bundle (buckets → blueprints)
./target/release/chameleon train-buckets
./target/release/chameleon train-bp

# 3. Play the hero against an archetype
./target/release/chameleon play

# 4. Evaluate and report
./target/release/chameleon ladder
./target/release/chameleon dashboard
```

> [!TIP]
> The in-repo justfile routes every invocation through `cargo run -q -p cham-cli --` so you never measure a stale binary — the same stale-binary gotcha that produced guard tests in `cham-cli`. Prefer that pattern during development.

---

## Usage

```
verify · train-buckets · train-bp · train-router · collect · probe · ladder ·
ab · slumbot · play · trace · dashboard · audit-buckets · gpu-doctor
```

| Command | What it does |
|---------|--------------|
| `verify` | Integrity and setup checks. |
| `train-buckets` | Build abstraction bucket tables from a config. |
| `train-bp` | Train blueprints (Exploit / ExploitBayes / Robust) and write `policy.bin` artifacts. |
| `train-router` | Train the softmax router over a collected dataset. |
| `collect` | Build the router dataset — synthetic stub (default) or instrumented real matches (`--real`, `--raw-opponent-19`). |
| `probe` / `ladder` | Quick strength probe / depth-ladder evaluation against the opponent pool. |
| `ab` | Tier-3 paired A/B with SPRT, per-opponent CIs, and optional promotion. |
| `slumbot` | Online evaluation against Slumbot (the one networked subcommand). |
| `play` | Run the hero against a chosen opponent. |
| `trace` | Inspect decision traces. |
| `dashboard` | Render a static HTML dashboard (headline, winrates, frontier, ledger) — inline SVG, no JS. |
| `audit-buckets` | Bucket-quality audit: within/between EV variance ratio, optionally self-generating data. |
| `gpu-doctor` | GPU availability probe — always exits 0, never a gate. |

Exit codes: `0` pass, `1` failure, `2` guard/budget refusal. Artifact-consuming commands print their blake3 hashes.

```bash
# A/B two routable agents with SPRT and promotion
chameleon ab full hedged --deals 20000 --promote

# Collect honest 19-dim router training data from real matches
chameleon collect --real --raw-opponent-19

# Audit abstraction quality from generated play
chameleon audit-buckets --generate
```

> [!TIP]
> `argmax` and `bayes` routing consume no RNG, so a replayed hand is bit-for-bit reproducible. Combined with iteration-budgeted search, evaluation results are byte-identical across machines.

---

## Configuration

Behavior is tuned through environment variables; there is no config file to drift out of sync.

| Variable | Default | Purpose |
|----------|---------|---------|
| `CHAM_AGENT_BUNDLE` | (resolution below) | Override which agent bundle is "the shipped bot". |
| `CHAM_HEDGE_THRESHOLD` | `0.5` | Argmax-vs-mixture switch for `hedged` routing. |
| `CHAM_HEDGE_DEBUG` | off | Hedge-path debug logging. |
| `CHAM_EXPLOIT_BUDGET_MB` | `0` (robust only) | Per-hand exploit budget (mbb) for `bounded` routing. |
| `CHAM_EXPERT_EXPL_MB_0..3` | `2000` | Measured per-expert exploitability (mbb) — set from fine-information best-response runs. |
| `CHAMELEON_MEMORY_BUDGET_MB` | `8192` | Pre-load bundle budget; oversized bundles are refused with the largest contributor named. |
| `CHAM_FALLBACK_MODE` | `renorm` | `substitute` restores legacy fallback semantics. |
| `CHAM_OFFTREE_TRANSLATE` | off | Translate the opponent's off-tree bet sizes into abstract classes in infoset keys. |
| `CHAM_IGNORE_ABSTRACTION_HASH` | off | Skip abstraction-hash verification — diagnostics only. |
| `CHAM_EXPLOIT_BP` / `_BUCKETS` / `_CONFIG` / `_DEALS` / `_SEED` | — | Inputs for the exploitability telemetry bench. |

Bundle resolution order: `CHAM_AGENT_BUNDLE` → `artifacts/agent-honest-19dim` (if present) → `artifacts/agent`.

> [!WARNING]
> `CHAM_IGNORE_ABSTRACTION_HASH=1` prints a loud warning for a reason: loaded keys may not match the trained abstraction. Never use it in a gate. Likewise, `CHAM_OFFTREE_TRANSLATE` is off by default because the shipped bundle was keyed without translation — enabling it changes key matching against real opponents.

---

## Artifacts and Integrity

An agent bundle is a directory:

```
artifacts/agent/
├── abstraction.toml        # validated at load
├── buckets/                # abstraction bucket tables
├── router.bin              # optional; deterministic default router otherwise
├── experts/
│   ├── 0/policy.bin        # specialist blueprint 0
│   ├── 1/policy.bin        # specialist blueprint 1
│   ├── 2/policy.bin        # specialist blueprint 2
│   └── 3/policy.bin        # specialist blueprint 3
├── robust/policy.bin       # robust blueprint (required)
└── bayes/policy.bin        # optional
```

Each `policy.bin` (`CHBP` format, v2):

- u16-quantized probabilities in 1/10,000 units (v1's u8 rounding dropped sub-0.5% actions); v1 artifacts still load.
- Embedded provenance JSON: abstraction hash, artifact hash, mode, depth, iterations, seed, thread mode, infoset count, wall time, parent link.
- **Self-verifying**: the payload hash (`blake3(keys ‖ offsets ‖ rows)`) is stamped into the embedded provenance and recomputed — and enforced — on every load. A hand-edited probability fails loudly.
- Strategy-only: no regrets ship in inference artifacts. Row decode is lazy (binary search + one row on first use), and independent loads replay decisions bit-identically.
- Confidence is visit-based: `c = visits / (visits + 64)`.

Side artifacts: `artifacts/river-cache.bin` (persistent subgame cache, validated on hydration), `artifacts/ledger/ledger.jsonl` (gate results), `artifacts/shadow/` (champion snapshots).

---

## Development

```bash
# Build
cargo build --workspace

# Test
cargo test --workspace

# Lint and format
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# A specific bench
cargo bench -p cham-blueprint --bench mccfr
cargo bench -p cham-search --bench solve
```

Benchmarks double as spec gates:

| Gate | Bench | Asserts |
|------|-------|---------|
| P4 | `cham-blueprint::mccfr` | ES-MCCFR throughput @ 200bb, mid abstraction, ≥ 1.5k iters/s (provisional) |
| P5 | `cham-search::solve` | River solve, reference spot, 400 iters, ≤ 250 ms wall |
| B10 | `cham-agent::decision` | Hero decision latency (search off / forced-on) and artifact load |
| — | `cham-search::trigger_cache` | Content-keyed cache ≥ 1.3× on the recurring trigger stream |
| — | `cham-blueprint::exploitability` | LBR telemetry (mb/hand + held-out SE) printed to CI logs |

Workspace conventions worth knowing:

- `[profile.dev]` runs at opt-level 3 so tests exercise the real algorithms; `[profile.test]` stays at O1 for the fast loop with O3 carve-outs for the numerics-critical crates.
- `[profile.release]` uses thin LTO with one codegen unit; benches inherit it.
- The dependency whitelist, the unsafe-scope decisions, and every clippy allowance are documented inline in the root `Cargo.toml`.
- `proptest` and `insta` (dev-only) cover property and snapshot testing.

---

## Project Status

### Working end-to-end

- The full loop: train → self-verifying artifacts → guarded loader → agent → evaluated matches → hash-bound ledger → dashboard.
- Artifact v2 writer with v1 backward-compatible reading, lazy row decode, and blake3 payload verification (H-6).
- All seven routing modes validate and dispatch (the `hedged` gap is covered by a regression test); search is wired through the agent's search bridge with the G4 ledger lockout as the auditable opt-in.
- River solvers pinned by independent oracles — verified support-enumeration Nash, Kuhn/Leduc equilibria, in-repo LP spots — with the AGPL reference solver kept out of the link graph by a deny tripwire.
- Eval guards live in every consuming command: untrained-artifact refusal, fallback-rate and router-degeneracy warnings, stale-binary protection, SPRT + paired CIs, shadow-gauntlet promotion.
- Persistent, validated subgame caching; O(n) CFV kernels; `gpu-doctor` probe with feature-gated Metal/wgpu and a CPU fallback.
- Workspace hygiene: `unsafe` forbidden, closed dependency whitelist, postcard migration complete.

### Tracked gaps

- **The router is trained on synthetic data and is degenerate** — the stub dataset encodes the answer in a feature, and the honest real-data gate currently fails (0.761 top-1, 0.45 TAG recall). `argmax` is the shipped default because it measures best on the current bundle (+6,567 vs +3,184 mb/seating vs mixture); `full-mixture` restores the historical behaviour. This is the main open thread.
- **`ab --clusters N > 1` refuses loudly** — session-clustered CIs exist in `cham-eval` but are not wired into the A/B runner yet.
- **Two best-response implementations coexist**: the legacy clairvoyant `lbr_vs` (which overstates exploitability — measured 115× on a near-Nash Kuhn strategy) and the infoset-consistent `tabular_br` / `tabular_br_fine` with held-out SE. Prefer the latter.
- **`bounded` routing is robust-only by default** — the exploit budget is 0 until measured `CHAM_EXPERT_EXPL_MB_*` values exist.
- **Off-tree bet-size translation is opt-in** and changes infoset key matching; leave it off unless the bundle supports it.
- **Combo-level subgame expansion** (≤ 64×128 leaf paths) is the stated stretch goal; strength-class collapse is the shipped scope (decision D-012).
- **The `action_distribution` measurement hook covers `mixture`, `argmax`, and `robust-only`**, not `bayes`.
- **GPU is optional and probed, not a default path** — all shipped compute is CPU.

> [!WARNING]
> Chameleon is a research harness, not a turnkey product. The guarantees it makes — leak-proofness, integrity, determinism, auditable gates — are real; the *playing strength* of any particular bundle is a function of the artifacts you train and the router data you feed it, not of this README.

---

<p align="center">
  <em>Chameleon is a work in progress. Bug reports, benchmark studies, and design discussions are welcome.</em>
</p>
