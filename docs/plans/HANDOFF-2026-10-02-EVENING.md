# CHAMELEON — session handoff, 2026-10-02 evening

**Written:** 2026-10-02 ~20:30 CEST.
**Supersedes for "current state":** everything before it.
**Repository root:** `/Users/adm/Documents/Repos/chameleon`
**Branch:** `main`. The 2026-10-01/02 morning work is pushed to
`origin/main`; the evening batch (19 commits) is **local, not pushed**.
**HEAD:** `f8bc99f` or later (`git log --oneline -1`).

## 0. Read this first, in order

1. **`CORRECTED-METRIC-LEADERBOARD-2026-10-02.md`** — every measured
   number in one table. The single most useful doc.
2. **`SHIPPED-BUNDLE-EXPLOITABILITY-2026-10-02.md`** — the shipped
   bundle is ~15 bb exploitable; fresh policies are ~0.
3. **`SIZE-BUCKET-DEGENERACY-2026-10-02.md`** — the infoset key
   carries ~1 bit of size; the tiny ladder has ~1 size/street.
4. **`DCFR-SWEEP-CORRECTED-2026-10-02.md`** — DCFR(1.5,0,γ=2) wins on
   the corrected metric; the clairvoyant metric disagrees.
5. **`F6C-E6-FALLBACK-MEASUREMENT-2026-10-02.md`** — translation is a
   no-op; the report's E6 gate needs a retrain.

## 1. The three findings that matter

### 1.1 The shipped bundle is ~15 bb exploitable; fresh is ~0

`agent-honest-19dim/robust` measures **+15.43 bb** corrected
(BR(0)+BR(1), 5000/500/30). A fresh 5M tiny policy trained with the
current (F3/F4/F6a) trainer measures **-1.45 bb** — indistinguishable
from unexploitable. The shipped bundle predates the trainer fixes.

**Action: retrain the shipped bundle.** In progress (see §3).

### 1.2 The infoset key barely encodes size

`size_bucket` = `round(stack_frac * 12).clamp(1,15)`. Measured: 100% of
normal-size aggressive actions across all four streets get bucket **1**;
only a jam escapes (bucket 12). So the key is ~1 bit of size
information. Root cause: the **tiny ladder has 1 normal bet size per
street** (2 on river) — there is no size variety to encode.

**Consequence: F6c translation and `CHAM_SLOT_BUCKET` cannot help on
tiny.** They are correct code, but tiny has nothing for them to do.
The real lever is a richer ladder (`abstraction-tiny-rich.toml`), which
is the report's Phase 2.

### 1.3 DCFR(1.5, 0, γ=2) is the best corrected-metric policy

At 5M: corrected sum **-0.70** vs CFR+ 5M's **-1.45**. But the
clairvoyant metric ranks it *worse* — the metric that drove every prior
DCFR decision was pointing the wrong way.

## 2. What landed this session (evening batch, local)

Code:
- **F4 completed**: f64 strategy increments end-to-end (was f32-truncated
  before the f64 arena) — `35935b7`.
- **Bug-hunt pass 2**: checkpoint mkdir/write/rename failures now
  surface (were silently swallowed) — `558536b`.
- **Bug-hunt pass 3**: L-7 empty-distribution guard propagated to
  `pipeline::sample_index` + `rng::weighted_pick` — `1a20f37`, `1ad00bb`.
- **Bug-hunt F2**: stale RBP `theta0` doc-comment — `3039d19`.
- **`OffTreeBettor`** + factory wiring + tests — `1bd2621`, `a352bc7`,
  `3dd62b4`.
- **Provenance** now records `dcfr_alpha/beta`, `avg_gamma`,
  `avg_delay_override` — `5039315`.
- **Tests**: `size_bucket_distribution`, `slot_bucket_range`,
  `slot_inventory`, `offtree_bettor`, `f6c_translate_u` — all green.

Docs:
- `CORRECTED-METRIC-LEADERBOARD-2026-10-02.md` (new)
- `SHIPPED-BUNDLE-EXPLOITABILITY-2026-10-02.md` (new)
- `SIZE-BUCKET-DEGENERACY-2026-10-02.md` (new)
- `DCFR-SWEEP-CORRECTED-2026-10-02.md` (new)
- `F6C-E6-FALLBACK-MEASUREMENT-2026-10-02.md` (new)
- `BUGHUNT-2026-10-02.md` (new)
- plus earlier-batch handoffs already pushed.

## 3. What is running right now (background)

1. **Retrain `artifacts/agent-honest-19dim-retrained`** — robust
   (DCFR 1.5/0/γ=2) done; 4 experts in progress (nit done, tag running).
   Script `artifacts/retrain-2026-10-02/run.sh`, log
   `artifacts/retrain-2026-10-02/summary.txt`. Under load ~110-170, each
   expert takes ~90 min; ETA several hours.
2. **Early robust metric** — `artifacts/retrain-2026-10-02/early-robust-metric.txt`,
   measuring the just-trained robust arm at low priority. Lands in
   ~15-30 min; tells us whether the DCFR-γ2 retrain is on track before
   the experts finish.

**Do not disturb**: `pkr-trainer` ×2 and `expl_eval` (the user's other
projects) are the load. Our jobs are `nice`d.

## 4. What to do next

1. **When the retrain finishes**, measure the assembled bundle's
   corrected metric (`both_seats_tabular_br`, 5000/500/30). If it drops
   from +15.43 toward the fresh ~0, the retrain is the win.
2. **Then decide on the richer ladder.** `abstraction-tiny-rich.toml`
   + `CHAM_SLOT_BUCKET=1` is the only configuration where size
   resolution can matter (per §1.2). That is a separate retrain + curve.
3. **F10 step 2** (vector CFR+ river solver) — step 1 (kernels) is done
   and validated; see `F10-VECTOR-SOLVER-PLAN-2026-10-02.md`.

## 5. Gotchas carried forward

- The clairvoyant metric (`lbr::lbr_vs`) is 6-10x too high; quote
  `lbr::tabular_br` **with its budget**.
- `tabular_br` sum goes negative at low budget (300/200/12 → -9.3);
  use 5000/500/30 for headline numbers.
- `CHAM_OFFTREE_TRANSLATE` is a no-op on tiny (§1.2); do not ship it
  alone.
- The retrain uses the **default** (1-bit) bucket, by design — it
  isolates the trainer from the ladder.
- 19 commits local; `git push` runs the fmt+clippy pre-push gate (both
  currently clean).

## 6. Repository state

    git log --oneline -1     # f8bc99f or later
    git log --oneline origin/main..HEAD | wc -l   # 19 (evening batch)
    git status --short       # clean (or artifacts/ledger only)

    # green check: fmt clean, clippy clean, 282 tests pass

---

## Addendum (2026-10-03): bundle-resolution, routing, and the degeneracy guard

The evening batch above is superseded for "what changed" by this
addendum. Since it was written:

### Shipped-bundle promotion
- `artifacts/agent-honest-19dim` was overwritten with the 2026-10-03
  retrained policies (DCFR 1.5/0/γ2 robust + retrained experts). Backup
  at `artifacts/agent-honest-19dim-prev`. See `PROMOTION-2026-10-03.md`.
- The retrained bundle beats the old shipped one by **+2989 mb/seating
  mean, 8/9 opponents** (`RETRAIN-19DIM-RESULTS-2026-10-03.md`) and its
  corrected exploitability is **-1.29 bb vs +15.43**.
- `hero.rs`'s default bundle now prefers the promoted bundle, falling
  back to the tracked `artifacts/agent` on a fresh clone. Centralized in
  `guard::resolve_agent_bundle()`; `play`/`probe`/`guard` all use it
  (previously `ladder` and `play` could load *different* bundles).

### The router is degenerate (bigger than jamfix)
`probe --diag-fallback` now reports per-expert `argmax_pick` counts.
Result: the promoted bundle's router sends **99.9% of decisions to
expert 0 (nit)** — `--agent full` is effectively a single-expert agent.
Documented in `ROUTER-EXPERT-ROUTING-2026-10-03.md`; this is the
`SYNTHETIC-ROUTER-IS-DEGENERATE` finding quantified.

A **router-degeneracy guard** now warns at ladder time when one expert
takes ≥90% of picks (`guard::check_router_degeneracy`), recorded in the
ledger notes. Verified firing on the real bundle.

### jamfix machinery (for the regression fix)
- `JamBot` now implements `action_probs` (it had only `act`, so training
  against it fell back to uniform — `--opponent jamfix` trained vs noise).
- New `mix:<wa>:<a>~<b>` opponent spec + `MixerAgent` (convex mixture of
  two analytic opponents). `tests/mixer.rs`.
- A single-expert experiment (`nit` vs `mix:0.8:arch:nit~jamfix`, 5M) is
  training; result pending at
  `artifacts/exp-jamfix-mix-2026-10-03/summary.txt`.

### State
54 commits ahead of `origin/main`, tree clean, 292 tests pass, fmt +
clippy clean. No jobs running except the jamfix-mix training.
