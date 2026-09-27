# CHAMELEON — v7 SOTA Competitiveness Runbook

> **Audience:** an implementing agent with repo write access, no judgment
> calls required, no prior context beyond this file + `docs/BOARD.md` +
> `docs/HANDOFF.md`. Same discipline as `docs/plans/v6-fix-runbook.md`:
> exact file, exact find/replace or exact new-file content, exact
> commands, exact pass condition. If a "find" string doesn't match
> exactly, stop and diff before proceeding — don't guess.
>
> **Grounding:** this plan is built entirely from what's actually in the
> repo dump and `artifacts/ledger/ledger.jsonl` — not from generic CFR
> advice. Every claim below cites the ledger row, report, or spec section
> it comes from. Read Part 0 before touching any code; it changes what
> "competitive" even means for this codebase right now.
>
> **Constitution unchanged:** `docs/SPECS/00-conventions.md` §2/§3/§11
> still govern every item except Item 8 (SOTA-4, the neural leaf value
> net), which is explicitly a constitution amendment and is written up as
> a *proposal*, not code to merge silently — see Item 8's header.

---

## Part 0 — Ground truth (read this before anything else)

Three facts from the repo's own artifacts change the plan completely
versus a generic "get closer to SOTA" checklist:

### 0.1 The bot is not currently measurably competitive with *anything* — including itself a week ago

`docs/reports/20260927-rbp-gate-stale-results.md` documents a **regret-based
pruning gate bug** (`crates/cham-blueprint/src/traversal.rs`): the
documented "pruning disabled at `theta0=0`" behavior was actually
pruning unconditionally from the first visit, so every action whose CFR+
regret floored at zero was **permanently frozen out** and training
collapsed to a pure (one-hot) strategy at every infoset. This was fixed
in commit `fe84467`. Every LBR/exploitability number in the ledger
**before** that commit — including the whole EXP-011 α/γ sweep, both
EXP-014 widened A/B rows, and every "competitiveness" number in
`docs/reports/competitiveness.md` — is stale. Confirmed by the ledger's
own last entry:

```
run: "honest-lbr-tiny-3M-s7"
seat0_lbr: 36746.62  seat1_lbr: 16536.3  seat_mean_lbr: 26641.46   (trained policy)
seat0_lbr: 40413.89  seat1_lbr: 35099.54 seat_mean_lbr: 37756.72   (uniform random policy)
```

**Read that number correctly.** After 3,000,000 correctly-trained
iterations on the *tiny* abstraction, the blueprint is only **29% less
exploitable than a uniform-random policy** (mean LBR 26,641 vs 37,757
mb/hand at depth 100). A converged HUNL blueprint at this scale should be
one to two orders of magnitude below that — Libratus-class blueprints
run LBR gaps in the low hundreds of mb/hand, not tens of thousands. This
is not a tuning problem, it's an **under-training / under-scale**
problem: 3M iterations on a k=32/16 abstraction is a smoke test, not a
blueprint.

**This means: every "make it competitive" lever downstream of the
blueprint (router quality, search quality, mixture composition) is
currently being layered on top of a policy that hasn't converged.**
Improving the router before fixing this is optimizing noise. Part 0.2
below reprioritizes accordingly.

### 0.2 What "SOTA techniques" already means in this repo — and what's actually missing

The repo has *already scoped* the standard SOTA lever list, correctly,
in `docs/backlog/perf.md`'s "Named SOTA techniques" section and
`docs/SPECS/06-cham-search.md` §8. Don't re-derive this — execute it:

| SOTA technique | Status in repo | Where |
|---|---|---|
| CFR+ / regret matching+ | shipped | `cham-blueprint/src/traversal.rs` |
| Linear/Discounted CFR (DCFR, Brown & Sandholm) | shipped, α/γ split exists | `regret_discount` + `avg_gamma`, **but every sweep result measuring it is stale (0.1)** |
| Potential-aware / EMD abstraction (Ganzfried & Sandholm) | **[TODO] B-6** — CPU-validated at small scale (`--profile exact`, ratio 0.24→0.31 at 60 deals, corrected to +5% at 120 deals — see EXP-017 in the ledger), full-orbit GPU bulk-fill **staged but not landed** | `docs/backlog/perf.md` B-6, `docs/BOARD.md` IN PROGRESS |
| Multi-leaf continuation strategies (DeepStack, Moravčík et al.) | **[TODO] B-7** — explicitly identified, not built | `docs/backlog/perf.md` B-7 |
| Real-time subgame solving (RNR/FMBR, DeepStack/Libratus) | shipped, but running at **13ms of a 250ms budget** — 19× headroom unused | `docs/SPECS/06` §2, `competitiveness.md` §1.2 |
| Bayesian opponent modeling | shipped this cycle (Dirichlet-multinomial fusion), **but EXP-015 shows it's exploitable by a simple switching manipulator in every one of 60 hyperparameter cells** | `crates/cham-router/src/runtime.rs`, `docs/reports/exp-015-router-manipulation-grid.md` |
| Deep counterfactual value networks (DeepStack/ReBeL) | **explicitly out of scope**, flagged as "the real SOTA path" pending a **human-approved constitution amendment** | `docs/SPECS/06` §8 |
| AIVAT variance reduction | **spec-only**, not implemented | `docs/plans/aivat-baseline-spec.md` |

So "make it competitive with SOTA" in this codebase is **not** primarily
a research problem — the algorithms are chosen correctly and mostly
already coded. It is an **execution-completion problem**: finish the
honest retrain, land B-6/B-7, use the search-budget headroom that's
being left on the table, and close the two measured integrity gaps
(router manipulability, and the 3.3–3.7% remaining fallback rate). Then,
*if* the user wants literal Libratus/Pluribus/ReBeL parity rather than
"strong tabular bot on a laptop," Item 8 is the one genuine big-refactor
lever, and it requires a human sign-off the repo's own spec already
anticipates.

### 0.3 Sequencing constraint

Every item below that measures policy strength (LBR, ladder EV,
exploitability deltas) is **worthless until Item 1 (honest full-scale
retrain) lands**, per `docs/reports/20260927-rbp-gate-stale-results.md`'s
own re-measurement plan. Items are numbered in dependency order — do not
skip ahead to Item 4 (EMD/GPU) or Item 6 (search depth) and then
interpret the results, because you won't know if a delta came from your
change or from measuring a still-collapsing-adjacent regime. The doc's
own plan (§ "Re-measurement plan") is folded into Item 1 below verbatim.

---

## Item 1 — Finish the honest re-baseline (prerequisite for everything else)

**Type:** measurement, no design changes. **Risk:** none. **Time:**
mostly wall-clock (multi-hour training runs), not engineering.

### Step 1.1 — confirm the fix is what you think it is

```bash
grep -n "theta_t\|theta0" crates/cham-blueprint/src/traversal.rs
git log --oneline -1 -- crates/cham-blueprint/src/traversal.rs   # expect fe84467 or later
```

### Step 1.2 — land the two in-flight honest LBR runs

`docs/reports/20260927-rbp-gate-stale-results.md` records
`theta-inf-3M-s7` as done and `theta-inf-10M-s7` as "in flight." Confirm
and, if not finished, run it:

```bash
grep -n "theta-inf-10M-s7" artifacts/ledger/ledger.jsonl || \
cargo run -q -p cham-cli -- train-bp \
    --config config/agents/robust-only.toml \
    --buckets artifacts/buckets-tiny \
    --iters 10000000 --seed 7 --thread-mode deterministic \
    --regret-discount-theta0 1e18 \
    --out artifacts/nopruning-diag/theta-inf-10M-s7
cargo bench -p cham-blueprint --bench exploitability -- --save-baseline theta-inf-10M-s7
```
(`--regret-discount-theta0` is illustrative of the flag that sets
`theta0`; confirm the actual flag name in `crates/cham-cli/src/cmd/train_bp.rs`
before running — `grep -n "theta0\|regret_discount" crates/cham-cli/src/cmd/train_bp.rs`.)

### Step 1.3 — retrain the full agent bundle (robust + 4 specialists) with the fix, tiny abstraction first

```bash
cargo run -q -p cham-cli -- train-bp --config config/agents/full.toml \
    --buckets artifacts/buckets-tiny --iters 10000000 --thread-mode deterministic \
    --out artifacts/agent-honest-tiny
DIAG_DEALS=60 cargo run -q -p cham-cli -- probe --diag-fallback \
    --bundle artifacts/agent-honest-tiny --agent full
cargo run -q -p cham-cli -- ladder --fast --agent full \
    --bundle artifacts/agent-honest-tiny --pool config/pool.toml
```

### Step 1.4 — re-run the two sweeps the stale-results report explicitly calls stale

```bash
# EXP-011 alpha/gamma — re-run on the honest 10M-iter regime, not 100k
cargo run -q -p cham-cli -- ab full full --deals 25000 \
    --a-bundle artifacts/agent-honest-tiny --b-bundle artifacts/agent-honest-tiny \
    --a-alpha 0.9 --a-gamma 0.5 --b-alpha 1.0 --b-gamma 0.9   # illustrative flags — confirm in ab.rs --help
# EXP-014 widened-tiny A/B — same, on honest weights
cargo run -q -p cham-cli -- train-bp --config config/agents/full.toml \
    --buckets artifacts/buckets-tiny --opponent-rotation config/training/rotation-widened.toml \
    --iters 10000000 --out artifacts/agent-honest-widened-tiny
```

### Step 1.5 — append ledger entries and lint

```bash
cargo run -q -p cham-cli -- lint-ledger --all
```

### Pass condition

`artifacts/agent-honest-tiny`'s ladder run shows fallback < 5% (per the
already-fixed EXP-013 renorm, this should hold), and its LBR at depth
100 is **materially below** the 3M-honest baseline's 26,641 mb/hand mean
(if it isn't — more iterations aren't converging and Item 2's abstraction
work becomes the higher-priority lever, not iteration count; record
either outcome, don't force a conclusion).

---

## Item 2 — Scale training compute by 1–2 orders of magnitude, correctly

**Type:** competitiveness, core lever. **Risk:** none (measurement-driven,
kill criteria included). **Time:** dominated by wall-clock.

### Why this is the top lever (not EMD, not search)

Libratus/Pluribus-class blueprints run **hundreds of millions to billions**
of MCCFR iterations. This repo's honest run used 3–10M on the *tiny*
abstraction. The gap between 26,641 mb/hand (measured) and "competitive"
(low hundreds of mb/hand) is far more consistent with under-training than
with abstraction coarseness — Item 2 should run **before** the multi-day
EMD/GPU rebuild (Item 4), because if 10× more iterations closes most of
the LBR gap on the *existing* abstraction, that's a much cheaper win than
a bucket rebuild, and it tells you how much headroom Item 4 actually has
to work with.

### Step 2.1 — measure the convergence curve, not just endpoints

Add LBR checkpoints during a long training run instead of only
before/after:

```bash
grep -n "pub fn\|checkpoint" crates/cham-blueprint/src/trainer.rs | head -30
```

If `trainer.rs` has no iteration-checkpoint hook, add one (illustrative —
confirm the real `TrainConfig`/loop shape first):

```rust
// crates/cham-blueprint/src/trainer.rs
pub struct CheckpointConfig {
    pub every_iters: u64,      // e.g. 1_000_000
    pub out_dir: PathBuf,      // artifacts/checkpoints/<run-id>/
}

// inside the training loop, after existing per-iteration work:
if let Some(cp) = &cfg.checkpoint {
    if t > 0 && t % cp.every_iters == 0 {
        let snap_path = cp.out_dir.join(format!("iter-{t}"));
        policy.snapshot_to(&snap_path)?;   // reuse existing snapshot/export path (table.snap machinery)
    }
}
```

### Step 2.2 — run the convergence sweep

```bash
cargo run -q -p cham-cli -- train-bp --config config/agents/robust-only.toml \
    --buckets artifacts/buckets-tiny --iters 100000000 --seed 7 \
    --thread-mode deterministic --checkpoint-every 5000000 \
    --out artifacts/checkpoints/robust-100M-s7
```

For each checkpoint, run the LBR bench (this is the loop `honest-lbr-tiny-3M-s7`
already established):
```bash
for ckpt in artifacts/checkpoints/robust-100M-s7/iter-*; do
  n=$(basename "$ckpt")
  cargo bench -p cham-blueprint --bench exploitability -- \
      --bundle "$ckpt" --save-baseline "convergence-$n"
done
```

### Step 2.3 — plot LBR vs iterations, decide the operating point

Write the checkpoint LBR series to `docs/reports/convergence-curve.md` as
a table (`iters, seat0_lbr, seat1_lbr, seat_mean_lbr`). Three outcomes,
each with a defined next step:

- **Still dropping steeply at 100M:** the algorithm is under-trained, not
  under-abstracted. Scale further (this is now purely a compute-budget
  decision — the M1's `train-bp --threads` default already parallelizes;
  consider running overnight/multi-day per `scripts/overnight-2026-09-25.sh`'s
  existing pattern) before touching Item 4.
- **Flattening well above "competitive" (hundreds of mb/hand):** the
  *abstraction* is now the bottleneck — this is exactly what Item 4 (EMD/
  GPU) is for; prioritize it.
- **Flattening near "competitive":** move to Item 3 (DCFR retune) to
  squeeze the last bit, then Item 5 (search) and Item 6 (router).

### Step 2.4 — verify

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -q -p cham-cli -- lint-ledger --all
```

### Pass condition

`docs/reports/convergence-curve.md` committed with the real numbers and
an explicit verdict (one of the three outcomes above), plus a ledger
entry (`type: "bench"`, `run: "convergence-sweep-100M-s7"`) following the
existing `honest-lbr-tiny-3M-s7` schema.

---

## Item 3 — Re-run the DCFR α/γ sweep under the honest regime

**Type:** tuning, cheap once Item 2's convergence curve exists.
**Risk:** none. **Time:** ~1 day (9-cell grid × honest iteration count).

The **stale** EXP-011 result (100k iters, collapsed regime) found γ=0.5
gave an 8.6% LBR improvement over the checked-in default (γ=0.9, α=1.0).
That result is explicitly flagged `STALE` in
`docs/reports/20260927-rbp-gate-stale-results.md` and must not be trusted
— but it's a reasonable **prior** for where to center the honest re-sweep,
since DCFR's γ (averaging-weight decay) interacts with how many iterations
you actually run, and the honest regime now has real per-iteration regret
dynamics to discount.

### Step 3.1 — re-run the grid at the honest iteration count from Item 2

```bash
for alpha in 1.0 0.9 0.5; do
  for gamma in 1.0 0.9 0.5; do
    cargo run -q -p cham-cli -- train-bp --config config/agents/robust-only.toml \
        --buckets artifacts/buckets-tiny --iters <ITERS_FROM_ITEM_2> --seed 7 \
        --regret-discount "$alpha" --avg-gamma "$gamma" \
        --out "artifacts/dcfr-sweep/a${alpha}-g${gamma}"
    cargo bench -p cham-blueprint --bench exploitability -- \
        --bundle "artifacts/dcfr-sweep/a${alpha}-g${gamma}" \
        --save-baseline "dcfr-a${alpha}-g${gamma}"
  done
done
```

### Step 3.2 — append to the ledger using the EXP-011 schema, marked as the corrected re-run

```bash
cat >> artifacts/ledger/ledger.jsonl << 'EOF'
{"run":"exp-011-dcfr-alpha-gamma-sweep-HONEST","type":"bench","a":{"grid":{"alpha":[1.0,0.9,0.5],"gamma":[1.0,0.9,0.5]},"iters":<ITERS>,"seed":7,"abstraction":"tiny","metric":"LBR@depth100 seat-mean mb/hand"},"b":{"baseline":{"alpha":1.0,"gamma":0.9}},"delta_mb":<MEASURED>,"ci":null,"sprt":null,"promote":false,"seatings":0,"artifact_hash":null,"notes":"Re-run of exp-011-dcfr-alpha-gamma-sweep (marked STALE by docs/reports/20260927-rbp-gate-stale-results.md — original was measured on the pre-fe84467 collapsed-policy regime) under the post-RBP-gate-fix honest training regime at <ITERS> iters. Supersedes the stale entry.","ts":<UNIX_TS>}
EOF
cargo run -q -p cham-cli -- lint-ledger --entry exp-011-dcfr-alpha-gamma-sweep-HONEST
```

### Pass condition

Corrected sweep committed; if the winning cell differs from the checked-in
default (`alpha=1.0, gamma=0.9`), update `config/agents/*.toml`'s default
and re-run Item 1's full retrain with the new default before promoting.

---

## Item 4 — Land the EMD bucket rebuild (full-orbit GPU bulk-fill)

**Type:** competitiveness, abstraction quality. **Risk:** medium (multi-hour
GPU job) but low correctness risk — CPU-validation scale already passed.
**Time:** multi-day wall-clock.

This is **already `docs/plans/v6-fix-runbook.md` Item 6**, and per
`docs/BOARD.md` it's **`[WIP]`** — "flop running in background
(`artifacts/gpu-tables/flop-full`, ~3.5h projected, `--resume`); turn
(55M orbits) queued after." **Do not re-derive this item — check whether
it finished, and if not, resume and finish it exactly as Item 6 of
`v6-fix-runbook.md` specifies (steps 6.1–6.7).** The only addition here:
gate its promotion decision on Item 2's convergence curve, not in
isolation, because a better abstraction under an under-trained blueprint
will look like a smaller win than it actually is.

```bash
grep -n "EMD bucket rebuild\|flop-full\|turn.*orbit" docs/BOARD.md
ls artifacts/gpu-tables/ 2>/dev/null
```

If flop/turn full-orbit tables are present and validated (§6.3 in
`v6-fix-runbook.md`), proceed straight to §6.4–6.7 there. If not, resume
the background job per its `--resume` flag (confirm via `--help`) and
wait for completion before continuing.

### Pass condition (unchanged from v6-fix-runbook Item 6)

`verify --gpu` bit-exact check passes; exploitability bench shows the
real (honest-regime) delta vs the tiny abstraction; `ab` gate (including
the shadow gauntlet from v6 Item 5) passes; `docs/BOARD.md` updated.

---

## Item 5 — Use the search-budget headroom (13ms of a 250ms live cap)

**Type:** competitiveness, cheap. **Risk:** low (purely a budget-config
change plus one new leaf-continuation feature). **Time:** ~2 days.

`docs/reports/competitiveness.md` §1.2 measures the current RNR(400 iters)
river solve at **13.15ms end-to-end**, against a 250ms live move-clock —
**19× margin unused**. This is free competitiveness: SOTA real-time
solvers (Libratus, DeepStack) spend their *entire* per-decision budget on
search; this repo currently spends 5%.

### Step 5.1 — raise the default iteration budget

```bash
grep -n "iters.*400\|Rnr\|SearchBudget" crates/cham-search/src/trigger.rs config/agents/*.toml
```

Find wherever `400` (RNR iterations) is the checked-in default
(`SearchConfig` construction, likely in `crates/cham-agent/src/pipeline.rs`
or a `config/agents/*.toml` search block) and raise it, then re-measure
wall-clock to confirm you're still inside budget:

```bash
cargo bench -p cham-search --bench solve -- rnr_400   # confirm current number
# after raising, e.g. to 2000 iters (5x):
cargo bench -p cham-search --bench solve -- rnr_2000
```

Since 400 iters costs 13.15ms, a naive linear scaling puts 2000 iters at
~66ms — still 3.8× inside the 250ms cap, with room for the search
overhead (subgame build, cache miss) to not scale linearly. **Measure,
don't assume** — the pass condition below requires the real number.

### Step 5.2 — extend search past river (river-only is explicitly `river_only: true` in `SearchConfig`, called out as "v1 stretch" in SPECS/06 §2)

Turn re-solving is explicitly named in SPECS/06 §8 as part of the "real
SOTA path" but was scoped out only for time, not for a hard blocker —
unlike Item 8 (the neural net), turn search with the *existing* RNR/FMBR
machinery needs no constitution change, just a bigger subgame tree and a
budget check. Confirm feasibility before committing to full build-out:

```bash
grep -n "river_only" crates/cham-search/src/trigger.rs crates/cham-agent/src/*.rs
```

If flipping `river_only: false` is a straightforward tree-size increase
(turn subgames are card-removal-larger but same solver family), run a
budget bench first:

```bash
cargo bench -p cham-search --bench solve -- --turn-subgame-smoke
```

If turn-subgame wall-clock at a reasonable iteration count blows the
budget by a wide margin, this is a **separate, larger item** — record it
as `[TODO] Item 5b — turn re-solving` in `docs/BOARD.md` rather than
forcing it into this pass; river-depth search headroom (Step 5.1) is the
one guaranteed-cheap win here.

### Step 5.3 — land B-7: multi-leaf continuation strategies (DeepStack technique)

This is the single highest-value SOTA technique still marked `[TODO]` in
`docs/backlog/perf.md` that doesn't require a constitution change.
DeepStack's finding (Moravčík et al. 2017): a solver that continues past
its solved depth with a *single fixed* blueprint continuation strategy is
itself exploitable, because the opponent can play to make that single
continuation systematically wrong. The fix: blend 2–3 *perturbed*
blueprint continuations at the leaf instead of one.

`crates/cham-search/src/prior.rs` already has `leaf_variants` per
`worklog.md`'s §5.1 entry ("`prior::leaf_variants` (base/call-heavy/
fold-heavy by Action semantics + `LeafSet::blend` combinator)") — **this
was built but check whether it's actually wired into the live solve path
or just unit-tested in isolation**:

```bash
grep -rn "leaf_variants\|LeafSet::blend" crates/cham-search/src/solve.rs crates/cham-search/src/subgame.rs
```

If it's only exercised by `prior.rs`'s own unit tests and not called from
`subgame.rs`'s tree construction or `solve.rs`'s leaf evaluation, wire it
in (illustrative — confirm real signatures first):

```rust
// crates/cham-search/src/subgame.rs — wherever leaf nodes currently
// get a single continuation value from the prior blueprint:
// BEFORE:
let leaf_value = prior.blueprint_continuation(leaf_state);

// AFTER:
let variants = prior::leaf_variants(leaf_state, &prior);   // base/call-heavy/fold-heavy
let leaf_value = variants.blend(&blend_weights);           // e.g. [0.6, 0.2, 0.2] — tune via Step 5.4
```

### Step 5.4 — measure the effect on LBR gap at the leaf

```bash
cargo bench -p cham-search --bench solve -- leaf_variant_blend
cargo run -q -p cham-cli -- verify --gpu --check-histogram-tables   # unrelated but cheap sanity check while here
```

Add a regression test asserting the blended-leaf LBR gap is strictly
≤ the single-continuation LBR gap on the existing oracle suite spots
(`crates/cham-search/src/oracle.rs`'s committed reference spots) — this
is the DeepStack thesis stated as a testable invariant.

### Pass condition

River search iteration count raised with measured wall-clock still
< 200ms (leaving margin); multi-leaf continuation wired into the live
solve path (not just unit-tested); oracle-suite LBR gap does not regress
and ideally improves; `cargo test -p cham-search` green; `docs/BOARD.md`
B-7 moved from `[TODO]` to `[DONE]` with the measured before/after LBR
gap.

---

## Item 6 — Harden the router against the EXP-015 manipulation vulnerability

**Type:** competitiveness/safety. **Risk:** none if scoped to the router
only. **Time:** ~2 days.

`docs/reports/exp-015-router-manipulation-grid.md` (and the ledger row
`exp-015-router-manipulation-sweep`) found the adaptive
`switch:arch:nit->arch:lag` manipulator earns **+1171 to +1476
mb/seating in all 60 hyperparameter cells tested** — there is no
`(switch_at, N0, temp)` combination in the current design space that
defends against a simple mid-session archetype switch. Per that report's
own re-measurement note, this result is **regime-dependent** (measured
against the pre-fix collapsed policy) — **re-run it after Item 1's honest
retrain before deciding this is still a real gap**, but the *mechanism*
finding (no hyperparameter cell in the existing design dominates) is
unlikely to be an artifact of policy collapse, since it's a router
property, not a blueprint-quality property.

### Step 6.1 — re-run the grid post-honest-retrain (cheap confirmation)

```bash
# same loop as v6-fix-runbook.md Item 8 Step 8.2, pointed at the honest bundle
for switch in 10 20 40 80 150; do
  for n0 in 4 8 16 32; do
    for temp in 0.5 0.7 1.0; do
      cargo run -q -p cham-cli -- self-exploit \
        artifacts/agent-honest-tiny/robust artifacts/buckets-tiny config/abstraction-tiny.toml \
        --deals 2000 --switch-at "$switch" --router-n0 "$n0" --router-temp "$temp" \
        > "artifacts/exp-015-grid-honest/switch${switch}-n0${n0}-temp${temp}.json" 2>&1
    done
  done
done
```

### Step 6.2 — if the vulnerability persists, implement B-8 (Bayesian sequential router update) properly, not just parameter-tune it

`docs/backlog/perf.md` B-8: replace `runtime.rs`'s fixed-α exponential
smoothing (now Dirichlet-multinomial fusion per `worklog.md` §5.2) with a
**change-point-aware** posterior — the current model assumes a
*stationary* opponent type within a session; a switching manipulator
violates that assumption by construction, and no fixed prior-strength
`N0` can distinguish "noisy but stationary" from "just switched types"
after the fact. The standard fix is a lightweight Bayesian online
change-point detector (Adams & MacKay 2007-style run-length posterior)
layered on top of the existing Dirichlet fusion:

```rust
// crates/cham-router/src/runtime.rs — new: run-length hazard model
// alongside the existing Dirichlet-multinomial posterior.
pub struct ChangepointShield {
    hazard_rate: f64,          // prior prob. of a type-switch per hand, e.g. 1/200
    run_length_posterior: Vec<f64>,  // P(run length = r | evidence so far)
}

impl ChangepointShield {
    // Bayesian online changepoint detection update (Adams & MacKay):
    // on each hand's evidence, update run-length posterior; a sharp mass
    // shift toward run-length=0 signals "the opponent's type just changed"
    // and should DECAY the accumulated Dirichlet counts faster than the
    // stationary N0 smoothing does.
    pub fn update(&mut self, hand_evidence_loglik: &[f64] /* per-archetype */) {
        // growth probabilities: P(r_t = r_{t-1}+1) ∝ (1-hazard) * P(r_{t-1}) * loglik
        // reset probability:    P(r_t = 0)         ∝ hazard * sum_r P(r_{t-1}=r) * loglik
        // renormalize; this is a standard O(t) or O(1)-with-truncation update —
        // see Adams & MacKay 2007 §2 for the exact recursion.
        todo!("implement the run-length recursion; truncate history at e.g. 200 hands for O(1) amortized cost")
    }

    // effective smoothing weight for the Dirichlet fusion: sharpen (lower
    // effective N0) when run-length posterior mass concentrates near 0.
    pub fn effective_n0(&self, base_n0: f64) -> f64 {
        let p_recent_change: f64 = self.run_length_posterior.iter().take(5).sum();
        base_n0 * (1.0 - p_recent_change).max(0.1)   // floor so it never fully forgets
    }
}
```

Wire `effective_n0` into `RouterRuntime::weights_for_hand` in place of the
fixed `N0` constant, gated behind a feature flag so it can be A/B'd
against the fixed-N0 baseline on the *same* EXP-015 grid:

```bash
cargo run -q -p cham-cli -- self-exploit \
    artifacts/agent-honest-tiny/robust artifacts/buckets-tiny config/abstraction-tiny.toml \
    --deals 2000 --switch-at 40 --router-changepoint-shield   # new flag, illustrative name
```

### Step 6.3 — verify

```bash
cargo test -p cham-router
cargo clippy -p cham-router --all-targets -- -D warnings
```
Add unit tests: (a) stationary-opponent case — changepoint shield's
`effective_n0` stays near `base_n0` when there's no switch (no regression
on non-adversarial opponents); (b) switching case — `effective_n0` drops
sharply within a small window of the switch point.

### Pass condition

Re-run EXP-015's 60-cell grid with the changepoint shield enabled; the
manipulator's earn rate must drop materially (not just in one cell — per
the original PREREG-EXP-015 follow-up rule, only promote if it dominates
*across* `switch_at` values) without regressing raw ladder EV against the
stationary archetype pool (`config/pool.toml`'s non-switching opponents).
Follow the PREREG-EXP-015 rule literally: a standard EXP-001-style ladder
check before any promotion.

---

## Item 7 — Close the remaining 3.3–3.7% fallback + the capacity-vs-coverage question

**Type:** competitiveness, correctness. **Risk:** none. **Time:** ~1 day
(mostly retraining wall-clock).

This is **`docs/plans/v6-fix-runbook.md` Item 7**, already specified —
execute it as written (§7.1–7.6) once Item 2's convergence work tells you
what "higher iters" should mean in absolute terms (use Item 2's chosen
operating point, not a guess). Per `docs/BOARD.md`, this was `[WIP]` as
`scripts/exp-014-hi-iters.sh` — **check if it finished and what the
verdict was** before re-running:

```bash
grep -n "exp-014-widened-full-hi-iters" artifacts/ledger/ledger.jsonl
cat artifacts/overnight-2026-09-25/*.log 2>/dev/null | grep -i "exp-014\|hi-iters" | tail -20
```

If the verdict is "capacity-confirmed" (fallback recovers toward 3.3%
with higher iters), this item is closed — fold the higher-iters setting
into the default training config. If "capacity-ceiling" (regression
persists even at higher iters), open the 5th-specialist item exactly as
`v6-fix-runbook.md` Item 7's pass condition specifies.

---

## Item 8 — [PROPOSAL, NOT CODE] A DeepStack/ReBeL-style leaf value net — the actual SOTA-parity path

**Type:** architecture, constitution amendment. **Risk:** high — this item
requires a **human decision**, exactly as the repo's own spec already
anticipates (`docs/SPECS/06-cham-search.md` §8: *"a small DeepStack-style
river/turn value net via `candle` (Metal) — the real SOTA path,
explicitly out of v1 scope, requires M4 gates green with ≥ 2 days margin
and a human green light"*). **Do not implement this by silently editing
`docs/SPECS/00-conventions.md`'s dependency whitelist.** This section is
the design a human would review to grant that green light — write it up,
don't merge it.

### Why this is the one item that's a genuine, not just executional, gap

Items 1–7 close the gap between "under-trained/under-tuned tabular CFR
bot" and "well-executed tabular CFR bot on a 16GB laptop." That is a real,
achievable, and honestly *strong* target — it is not, and cannot be,
literal parity with ReBeL or Pluribus, because those systems' edge comes
specifically from **continual re-solving anchored by a learned
counterfactual-value function**, which lets them search to arbitrary
effective depth without a combinatorial explosion in the endgame tree.
Nothing in Items 1–7 adds that capability — more MCCFR iterations, a
better k-means abstraction, and a bigger search budget are all still
*tabular*, and tabular methods have a hard ceiling this project's own
16GB/M1 fence enforces (`SPECS/00` §6: training table ≤ 6GB, inference
artifacts ≤ 1.5GB). If literal SOTA-technique-parity (not just
"competitive play") is the actual goal, this is the lever, and there is
no way to get it without the two changes below.

### 8.1 — What would have to change

1. **`docs/SPECS/00-conventions.md` §2 dependency whitelist**: add
   `candle` (already named as the intended crate in SPECS/06 §8;
   currently in the explicit *forbidden* list alongside `tch`/`burn`).
   This needs the "human decision" the whitelist header already requires
   for anything outside the closed set.
2. **`docs/SPECS/00-conventions.md` §3 threading/determinism contract**
   and the "no inference-time NN" rule referenced throughout
   `docs/backlog/perf.md` and `docs/plans/v4-experiment-brainstorm.md`'s
   "Guardrails" section: a leaf value net is by definition an
   inference-time neural network. The determinism contract (bit-exact
   replay under a fixed seed) is *compatible* with a NN if inference is
   made deterministic (fixed weights, no dropout/randomness at inference,
   `f32` arithmetic pinned to a specific backend/precision mode) — this
   needs to be stated explicitly as an amendment, not assumed.

### 8.2 — Design sketch (for the human-reviewed proposal doc, not for merging)

```
crates/cham-search-nn/                    (NEW crate, feature-gated, off by default —
                                            same pattern as cham-gpu's metal/wgpu features)
├── Cargo.toml        # candle-core, candle-nn; NOT added to any other crate's deps
├── src/
│   ├── lib.rs
│   ├── value_net.rs   # small MLP: input = (bucket histogram for both ranges,
│   │                  #   pot/stack ratio, street) -> output = per-bucket-pair
│   │                  #   counterfactual value vector (DeepStack Fig 2 architecture,
│   │                  #   scaled down: this repo's k=32/16 tiny abstraction or the
│   │                  #   full k=300/200 gives a MUCH smaller input space than
│   │                  #   DeepStack's, so start with something on the order of a
│   │                  #   few-hundred-unit MLP, not a CNN)
│   ├── train.rs        # offline training loop: generate random turn/river subgames
│   │                    # from self-play, solve them EXACTLY with the existing
│   │                    # RNR/FMBR solvers (cham-search, unchanged), record
│   │                    # (subgame_features, solved_values) pairs, train the net
│   │                    # to regress solved values — this is pure offline batch
│   │                    # work, same determinism class as train-buckets/train-bp,
│   │                    # NOT a live-inference concern yet.
│   └── infer.rs         # deterministic forward pass for live use (pinned f32,
│                         # no batch-norm running-stats drift, fixed weight file
│                         # loaded via the same blake3-hashed mmap convention as
│                         # every other artifact in this repo)
```

Integration point: `crates/cham-search/src/subgame.rs`'s tree-depth limit.
Currently the tree is solved to the river/showdown boundary; with a
trained value net, `subgame.rs` can truncate the tree **earlier** (e.g. at
the turn, or mid-river) and use `cham-search-nn::infer` for leaf values
instead of full enumeration — this is exactly what lets ReBeL/DeepStack
search deep at bounded compute, and it directly *replaces* Item 5.3's
multi-leaf-continuation blend at the truncated depth (both solve the same
"what happens past my solved horizon" problem; the NN is strictly more
expressive once trained, and B-7's blended-leaf approach is a reasonable
non-NN fallback / sanity check for the NN's outputs).

### 8.3 — What this does NOT get you (be honest about it in the proposal)

- Still not Pluribus (multiplayer) — this codebase is HUNL-only by design
  (`README.md`: "for HUNL"); extending to multiplayer is a separate,
  larger scope change untouched by this item.
- Still not literal ReBeL — ReBeL's self-play generates training data via
  the *value net's own* recursive self-play loop (fictitious self-play
  with the net in the loop), which is a training-infrastructure change
  beyond "train a net against solver-labeled data" (the DeepStack-style
  version sketched above is the more tractable first step; ReBeL-style
  recursive self-play is a natural v2 of this item once the DeepStack-style
  version is validated).
- Training compute for the value net itself needs to be budgeted
  separately from the blueprint training compute — on a single M1, this
  is realistically a background/overnight job class, same as the EMD GPU
  bulk-fill in Item 4.

### 8.4 — Proposal deliverable (what to actually produce for this item, right now)

Do not write `crates/cham-search-nn/` code yet. Produce:

```bash
mkdir -p docs/plans
```

Write `docs/plans/v8-leaf-value-net-proposal.md` containing: §8.1's
whitelist/contract amendment text verbatim (as a diff-ready patch to
`docs/SPECS/00-conventions.md`), §8.2's design as the crate spec (in the
style of `docs/SPECS/06-cham-search.md`), a compute/time budget estimate
sourced from Item 2's actual measured MCCFR throughput (`P4` gate,
`cham-blueprint` benches — self-play subgame generation for training data
uses the same per-iteration cost class), and an explicit "M4 gates green
+ 2 days margin" checklist matching `SPECS/06` §8's own pre-authorized
condition for even starting this work. **This document is the "human
green light" artifact** — present it to the repo owner rather than
starting implementation.

### Pass condition

`docs/plans/v8-leaf-value-net-proposal.md` committed; **no other files in
this item change**. If and when a human approves it, that approval is a
new, separate runbook item (v9), not something this document should
pre-authorize.

---

## Sequencing summary

| Order | Item | Depends on | Type |
|---|---|---|---|
| 1 | Honest re-baseline | RBP-gate fix (done, `fe84467`) | measurement, blocking |
| 2 | Scale training iterations | Item 1 | competitiveness, top lever |
| 3 | Re-run DCFR α/γ sweep | Item 2 | tuning |
| 4 | EMD bucket GPU rebuild | Item 2 (informs priority), already `[WIP]` | competitiveness |
| 5 | Use search-budget headroom + B-7 leaf blend | Item 1 (for clean measurement) | competitiveness, cheap |
| 6 | Router changepoint hardening | Item 1 | safety/competitiveness |
| 7 | Close remaining fallback / 5th specialist | Item 2, already `[WIP]` | correctness |
| 8 | Leaf value net (proposal only) | Items 1–7 substantially complete, M4 gates green | architecture, human-gated |

## Final check — run after every item lands

```bash
cargo nextest run --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -q -p cham-cli -- verify --proofs --perf
cargo run -q -p cham-cli -- lint-ledger --all
git log --oneline -15
```

All must be clean. Update `docs/BOARD.md`'s DONE section with a
`### v7-sota-competitiveness-runbook` block, one line per item, in the
same style already used for the `v5-deepdive-audit` and `V4
fallback-measurement` blocks — measured numbers, not adjectives.
