# Phase C — PCS DCFR trainer, design (2026-10-08, rev 2)

**Purpose.** Build a real trainer for the blueprint. The shipped policy
(`agent-honest-19dim`) is exploitable at **6.41 ± 1.50 bb/hand** against
an exact card-perfect best response (Decision D1,
`docs/plans/D1-RESULT-2026-10-08.md`). Phase C replaces the tabular
pipeline that produced it.

**Rev 2 corrections** (this file replaces rev 1, commit 65bd602):

- Dropped all references to a "CHAMELEON-SOTA-PLAN.md" authority. That
  file is not in the working tree and has no git history. The prior
  session-start handoff asserted it; the assertion was wrong. Decisions
  are made below, explicitly.
- Corrected the module location. Rev 1 proposed
  `crates/cham-search/src/train/`, but `TrainerConfig` and the artifact
  writing live in `cham-blueprint`, and `cham-blueprint` does not
  depend on `cham-search`. The PCS module lives in `cham-blueprint`;
  the crate adds a `cham-search` dependency.
- `dcfr_gamma` is a NEW field. The existing `TrainerConfig` has
  `avg_gamma: f32` (default 0.9) which is a *different knob*: it scales
  a robust-mode weight as `gamma^(T-t)`, while DCFR's gamma scales the
  strategy-sum weight as `(t^gamma / (t^gamma + 1))`. Both are
  strategy-sum weights, but they decay differently and exist for
  different reasons. `avg_gamma` stays for the tabular path;
  `dcfr_gamma` is new for PCS.

## Gate (Decision D2)

A PCS-trained policy must:

- train in <= 4h wall clock on the tiny abstraction;
- beat the shipped blueprint's full-game VBR by > 3 SE, i.e. produce a
  policy whose D1 measurement is clearly below `6.41 - 3*SE(measure)`;
- improve monotonically in iterations (more iters => lower VBR, within
  measurement noise).

If the gate fails, do NOT proceed to W2/W3. Fix the trainer first.

## What PCS is

Public-chance-sampling CFR samples the *public* chance events (board
cards) rather than enumerating the full chance tree. For each update:

1. Sample a 5-card board uniformly without replacement from 52 cards.
2. Walk the `PublicTree` once with that board fixed.
3. At each infoset, accumulate regret and strategy as in CFR/DCFR.

Hero and villain **hole cards are NOT sampled**: both ranges are held
explicitly as vectors, exactly as the D1 VBR walker holds them
(`crates/cham-search/src/fullgame.rs`). This keeps card removal exact
and reuses the existing kernels (`showdown_cfv_two`, `fold_cfv`).

The variance introduced by board sampling is the price of
tractability; the D2 gate's 3-SE margin is calibrated to accept it.

## DCFR updates

Discounted CFR (Brown & Sandholm 2019):

- **Positive-regret discount**: `t^alpha / (t^alpha + 1)`.
- **Negative-regret discount**: `t^beta / (t^beta + 1)`.
- **Strategy-sum weight**: `(t / (t + 1))^gamma`.

The existing `TrainerConfig` already carries `dcfr_alpha: f64` and
`dcfr_beta: f64`. `cham-cli/src/cmd/train_bp.rs` invokes them with
`--dcfr-alpha 1.5 --dcfr-beta 0.0`. Phase C adds `dcfr_gamma: f64`
(default 2.0), a new field, and reuses the same alpha/beta CLI flags.

**Decision.** alpha = 1.5, beta = 0.0, gamma = 2.0. This matches the
shipped tabular trainer's CLI invocation, so a reader comparing PCS
output to the shipped blueprint compares one schedule to another, not
two.

## Architecture

    crates/cham-blueprint/src/pcs/
        mod.rs      - entry, iteration loop, uses TrainerConfig
        dcfr.rs     - one DCFR update on a sampled board
        sampling.rs - board sampler + deterministic RNG
        table.rs    - regret/strategy storage keyed by InfoSetKey

`cham-blueprint/Cargo.toml` gains:

    cham-search = { path = "../cham-search" }

for `PublicTree`, `showdown_cfv_two`, `fold_cfv`. No cycle:
`cham-search` does not depend on `cham-blueprint` (verified in recon).

Per sampled board:

- Build `State` with the sampled board baked into the deck prefix, same
  pattern as `fullgame::best_response`.
- Recursively walk the `PublicTree` from the root, carrying
  `(hero_reach, villain_reach)` vectors exactly as the walker does.
- At each node, look up regret/strategy tables by the encoder key
  `Encoder::key(&obs, &seq)`. Regret rows are wide enough for the
  node's action count.
- Terminal EV uses the same kernels as the walker.
- Hero and villain both accumulate regret; the strategy at a node is
  the DCFR regret-matching weighted average.

Storage: `FxHashMap<u64, Row>` per player, where `Row` holds a boxed
`[f64; W]` regret vector, a boxed `[f64; W]` strategy sum, and a visit
count. `W` is the node's action count (2..4 for the tiny ladder).

## Sampling scheme

- **Board sampling**: uniform over C(52,5) boards. The space is large
  (~2.6M boards); expect 10^6-10^7 iterations to converge.
- **Variance reduction (deferred)**: stratify by board texture so
  paired / monotone / suited boards are all represented every fixed
  number of iterations. Not part of the initial implementation.
- **RNG**: `cham_core::rng::rng_from_seed(seed)` for determinism. Same
  seed => byte-identical policy artifact.

## The D2 gate in concrete terms

After training N iterations at wall clock W:

    W <= 4h
    full_game_vbr(trained) <= 6.41 - 3 * SE_measurement
    VBR(iters) non-increasing within noise

The D1 harness (`crates/cham-agent/tests/d1_fullgame_vbr.rs`) is the
measurement. It is run against the trained policy bundle exactly as it
was run against the shipped one.

## Testing

1. **Small-deck reduction.** Train on a small deck (<= 8 cards) where
   the full game is enumerable; compare to exact CFR. The PCS trainer
   must converge to the same equilibrium. This is the correctness gate.
2. **Monotonicity.** VBR of checkpoints at iters = 10^3, 10^4, 10^5,
   10^6 must be non-increasing within measurement noise.
3. **Determinism.** Same seed twice => byte-identical artifacts.
4. **D2 comparison.** The full-game VBR of the trained policy, measured
   by the existing D1 harness, must beat the D2 threshold.

## Decisions (rev 1 called these "open questions")

- **DCFR schedule**: alpha = 1.5, beta = 0.0, gamma = 2.0.
- **Threading**: single-threaded first (correctness), then hogwild
  only after the small-deck reduction passes.
- **Warm start**: cold. Warm start from the shipped blueprint is a
  separate experiment; Phase C's baseline is a cold run.
- **Artifact format**: the PCS trainer's output MUST load through
  `BlueprintPolicy::load` (the shipped `policy.bin` format) so the D1
  harness measures it unchanged.
- **Belief bin**: hold at 0. Varying belief is out of scope.
- **`TrainerConfig` extension**: add `dcfr_gamma: f64` (default 2.0).
  Do NOT reuse or repurpose `avg_gamma`.
- **Module location**: `cham-blueprint/src/pcs/`; `cham-blueprint`
  gains a `cham-search` dependency.

## What this unblocks

- W2 (abstraction v3, more buckets) — the trainer must exist first;
  an abstraction change is only meaningful with a trainer that can
  train on it.
- W3 (combo river->turn solver) — same dependency.

## Relationship to the shipped trainer

`chameleon train-bp` trains the tabular pipeline. It stays. The PCS
trainer is a new command (`chameleon train-pcs`) that lives alongside.
D2's baseline to beat is the shipped blueprint's D1 number, not a
replacement of the shipped trainer.

## How to run (once implemented)

    chameleon train-pcs \
        --config config/abstraction-tiny.toml \
        --buckets artifacts/buckets-tiny \
        --iters 10000000 \
        --seed 7 \
        --out artifacts/pcs-tiny-<timestamp> \
        --wall-budget 14400 \
        --dcfr-alpha 1.5 --dcfr-beta 0.0 --dcfr-gamma 2.0

Then measure with the D1 harness:

    CHAM_D1_BP=artifacts/pcs-tiny-<ts>/robust \
    CHAM_D1_CONFIG=... \
    CHAM_D1_BUCKETS=... \
      target/debug/deps/d1_fullgame_vbr-<hash> --ignored --nocapture
