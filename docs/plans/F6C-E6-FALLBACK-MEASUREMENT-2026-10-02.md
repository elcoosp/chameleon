# F6c / report-E6 — off-tree fallback-rate measurement (2026-10-02)

**Question (report E6):** against an off-tree bettor, does F6c
translation reduce the fallback rate toward < 2%?

**Answer:** no, and the reason is structural.

## Method

New `OffTreeBettor` (`crates/cham-opponents/src/baselines.rs`, factory id
`offtree`) bets 0.75 / 0.6 / 1.5 pot postflop — sizes the tiny ladder
never produces (tiny: flop/turn 0.5, river 0.5 / 1.25). Pool:
`config/pool-offtree.toml` (`offtree` + `noisy:0.3:offtree`).

Command (each arm):

    CHAM_AGENT_BUNDLE=$PWD/artifacts/agent-honest-19dim \
      target/release/chameleon ladder --fast --agent full \
        --pool config/pool-offtree.toml

## Results

| arm | env | offtree mb/seating | fallback rate |
|---|---|---:|---:|
| A | (none) | -1279.1 ± 130.1 | **31.7%** (6888/21733) |
| B | `CHAM_OFFTREE_TRANSLATE=1` | -1279.1 ± 130.1 | **31.7%** (6888/21733) |
| C | `CHAM_SLOT_BUCKET=1` | -1537.5 ± 129.4 | **42.9%** (9589/22338) |
| D | both | -1537.5 ± 129.4 | **42.9%** (9589/22338) |

- **A and B are byte-identical**, including SE and VR: F6c translation
  is a complete no-op on the shipped bundle.
- **C changes the fallback rate** (31.7 → 42.9%), so the env plumbing
  works; the slot-index bucket reaches the key.
- **D == C**: with the slot bucket on, translation still adds nothing.

## Why translation is a no-op (the structural finding)

`record_action` (`cham-engine/src/ladder.rs`) derives `size_bucket` from
the **stack fraction** of the action:

    sf = bet / effective_stack
    bucket = round(sf * 12).clamp(1, 15)

At typical stack depths this rounds to **0 or 1 for every aggressive
size**. Example: pot 200, effective stack 10 000:

- 0.5-pot bet (100): sf = 0.0100, sf·12 = 0.12 → bucket 0
- 0.75-pot bet (150): sf = 0.0150, sf·12 = 0.18 → bucket 0

`translate` maps the 0.75-pot bet to the 0.5-pot slot, but the
*recorded key* is unchanged: both the raw and the translated action
produce bucket 0. The pseudo-harmonic mapping changes which engine
action is recorded, but the bucket — the only size-bearing part of the
key — is already saturated at 0.

**So F6c translation cannot reduce the fallback rate on this bundle,
not because it is wrong, but because the key it feeds ignores the size
it corrects.** Translation is only meaningful once `size_bucket` is a
function of the slot index (`CHAM_SLOT_BUCKET=1`) — the report's second,
coupled fix.

## And the slot bucket makes it worse (expected)

Arm C: slot-index buckets raise the fallback rate to 42.9%. This is
**expected and not a regression** — the shipped bundle was trained with
stack-fraction buckets, so slot-index buckets produce keys the bundle
never saw. The report's plan is: retrain with the slot bucket on, then
re-measure. The E6 gate (< 2% fallback) is a **post-retrain** target,
not a measure of the current bundle.

## What this means for F6c

1. **Do not ship `CHAM_OFFTREE_TRANSLATE` alone.** Measured: no effect.
2. **The translation and the slot-index bucket are a single coupled
   change.** Either both ship (with a retrain) or neither does.
3. **The E6 gate needs the retrain first.** Measuring fallback on the
   old bundle tells us the plumbing works (C) but not whether the gate
   is met — the bundle must be trained with the new keying.
4. The report's own sequencing (§5.2 Phase 2) already says this; the
   measurement confirms it and removes the option of shipping
   translation as a standalone win.

## Reproduce

    cargo build --release -p cham-cli

    # arm A
    CHAM_AGENT_BUNDLE=$PWD/artifacts/agent-honest-19dim \
      target/release/chameleon ladder --fast --agent full \
        --pool config/pool-offtree.toml

    # arm C (slot bucket)
    CHAM_SLOT_BUCKET=1 CHAM_AGENT_BUNDLE=$PWD/artifacts/agent-honest-19dim \
      target/release/chameleon ladder --fast --agent full \
        --pool config/pool-offtree.toml

Artifacts: `OffTreeBettor` (`1bd2621`), factory wiring (`a352bc7`),
tests (`3dd62b4`), pool (`0ba5e75`).

## Addendum: on-tree vs off-tree fallback (confounded, recorded for honesty)

A later run compared the shipped bundle against the normal pool
(`config/pool.toml`, on-tree opponents: arch:station, callbot, jamfix,
pnash, famB, noisy) versus the off-tree pool:

- **on-tree**: no fallback warning → rate **< 20%** (the
  `FALLBACK_WARN_RATE` threshold; exact count not printed).
- **off-tree**: **31.7%** (6888/21733).

This *looks* like "off-tree sizes raise fallback by >11 points," but the
two runs used **different opponents**, so it is confounded — the pools
differ in more than tree-fit. It is recorded here as a raw observation,
not a controlled result. A clean version would run the *same* opponent
set with and without off-tree sizes, which the current pools do not
provide.

The controlled result remains the A/B/C/D table above: translation off
vs on is byte-identical (no-op); the slot bucket changes fallback but
requires a retrain.
