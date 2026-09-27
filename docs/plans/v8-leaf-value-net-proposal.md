# v8 leaf value net — proposal for human green light (v7 Item 8)

> **[PROPOSAL, NOT CODE].** No crate was created, no dependency was added,
> no spec was edited. This document is the "human green light" artifact the
> runbook requires. Implementation starts only as a new, separate runbook
> item (v9) after explicit owner approval.

## 1. Why (the honest gap statement)

Items 1–7 close "under-trained/under-tuned tabular CFR bot" → "well-executed
tabular CFR bot on a 16GB laptop." That ceiling is real: tabular methods under
the `SPECS/00` §6 fence (training table ≤ 6GB, inference artifacts ≤ 1.5GB)
cannot reach literal ReBeL/Pluribus parity, whose edge is continual re-solving
anchored by a learned counterfactual-value function (unbounded effective depth
at bounded compute). Nothing in Items 1–7 adds that capability. This proposal
is the only genuine architecture lever, and it needs a constitution amendment.

## 2. Constitution amendment (diff-ready patch to `docs/SPECS/00-conventions.md`)

**§2 dependency whitelist** — move `candle` (already named as the intended
crate in `SPECS/06` §8) from the forbidden list (`tch`/`burn` stay forbidden)
to the allowed set, scoped:

```diff
- Forbidden: `tch`, `burn`, `candle` (no inference-time NN).
+ Forbidden: `tch`, `burn` (no inference-time NN except the v8 value net).
+ Allowed (gated): `candle-core`, `candle-nn` — ONLY inside the new
+ `cham-search-nn` crate (feature-gated, off by default), CPU-only pinned-f32
+ inference. Any use outside that crate is a spec violation.
```

**§3 threading/determinism contract** — append:

```diff
+ v8 amendment (neural leaf values): bit-exact replay under a fixed seed
+ extends to NN inference iff: fixed weight file (blake3-hashed, mmap
+ convention like every other artifact), no dropout/randomness at inference,
+ f32 arithmetic pinned to one backend/precision mode, single-threaded
+ deterministic forward pass. Training-time nondeterminism (if any) is
+ confined offline exactly like train-buckets/train-bp wall-clock variance:
+ only the checked-in weight file is canonical.
```

## 3. Crate spec (style of `docs/SPECS/06-cham-search.md`)

```
crates/cham-search-nn/   (NEW, feature-gated, off by default — cham-gpu metal/wgpu pattern)
├── Cargo.toml           # candle-core, candle-nn; NOT added to any other crate's deps
├── src/
│   ├── lib.rs
│   ├── value_net.rs     # small MLP: in = (bucket histograms for both ranges,
│   │                     #   pot/stack ratio, street) → out = per-bucket-pair
│   │                     #   counterfactual value vector (DeepStack Fig 2, scaled
│   │                     #   down: k=32/16 tiny or k=300/200 full is a MUCH smaller
│   │                     #   input space than DeepStack's — few-hundred-unit MLP)
│   ├── train.rs         # offline: random turn/river subgames from self-play →
│   │                     # solved EXACTLY with existing RNR/FMBR (cham-search
│   │                     # unchanged) → (features, solved_values) regression pairs
│   └── infer.rs         # deterministic forward pass (pinned f32, no
│                        # batch-norm drift, blake3-hashed weight file)
```

Integration: `cham-search/src/subgame.rs` tree-depth limit — truncate earlier
(e.g. turn/mid-river) and use `cham-search-nn::infer` for leaf values. This
directly *replaces* Item 5.3's multi-leaf blend at the truncated depth (same
"past my horizon" problem; NN strictly more expressive once trained; B-7 blend
stays as the non-NN fallback and sanity check on NN outputs).

## 4. Compute/time budget (sourced from measured throughput)

Self-play subgame generation for training labels uses the same per-iteration
cost class as the blueprint MCCFR loop (`P4` gate, `cham-blueprint` benches):
budget it as a background/overnight job class on the single M1, same as the
Item 4 EMD GPU bulk-fill. Concrete label count (e.g. 100k–1M solved subgames)
× measured solve wall-clock (river RNR-2000 ≈ tens of ms post-Item-5.1 —
re-measure before sizing) sets the calendar time; NN training itself (small
MLP, CPU) is second-order. Do not start until Item 2's convergence curve gives
the per-iteration cost at the chosen operating point.

## 5. Pre-authorized start condition (SPECS/06 §8 literal checklist)

- [ ] M4 gates green with ≥ 2 days margin
- [ ] Items 1–7 substantially complete (convergence verdict recorded, EMD
      promoted or killed, shield A/B measured, fallback verdict recorded)
- [ ] Human green light on THIS document (explicit owner approval)
- [ ] v9 runbook item opened (this doc does not pre-authorize implementation)

## 6. What this does NOT get you

- Still not Pluribus (multiplayer) — HUNL-only by design (`README.md`).
- Still not literal ReBeL — ReBeL's recursive self-play with the net in the
  loop is training-infrastructure beyond "train against solver-labeled data"
  (the DeepStack-style step above); ReBeL-style recursion is a natural v2.
- Value-net training compute is separate from blueprint compute (overnight class).
