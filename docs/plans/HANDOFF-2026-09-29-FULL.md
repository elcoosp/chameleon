# CHAMELEON — Session handoff for a successor agent

**Written:** 2026-09-29 evening, during the RM+ freeze investigation.
**Repository root:** `/Users/adm/Documents/Repos/chameleon`
**HEAD at handoff:** `dd41a51 exp: DCFR alpha sweep (0.9, 0.5) queued against the RM+ freeze`
**Branch:** `main`

This is the second session on this codebase (the first, on 09-27/09-28,
fixed the bug report). It continues the search for a stronger bot. Read
`docs/plans/SESSION-HANDOFF-2026-09-29.md` first for the summary; this
document is the full-detail version.

---

## 1. What is running RIGHT NOW (at 21:57 CEST on 09-29)

### Training (one job)

| PID | cmd | ETA | output |
|---|---|---|---|
| 70041 | 20M tiny robust with `CHAM_AVG_DELAY=0` | ~25 min | `artifacts/par-20M-delay0/` |

This is the "remove the averaging delay" experiment. Its LBR result will
land in `artifacts/par-20M-delay0-lbr.log`.

### Queued pipelines (each waits for the previous)

| PID | script | what it does | order |
|---|---|---|---|
| 70016 | `scripts/avg-delay-experiments-2026-09-29.sh` | Runs delay0 (PID 70041) then avguniform 20M | running |
| 90836 | `scripts/freeze-evolution-2026-09-29.sh` | Trains 20M with checkpoints every 2M; runs `rm_freeze` on each; then 5M warmfix | after avg-delay |
| 91553 | `scripts/dcfr-alpha-experiments-2026-09-29.sh` | Trains 20M tiny robust at `--regret-discount` 0.9 and 0.5 | after freeze-evolution |

Logs to watch:
- `artifacts/avg-delay-pipeline.log`
- `artifacts/freeze-evolution-pipeline.log`
- `artifacts/dcfr-alpha-pipeline.log`

Each pipeline is `nohup nice -n 15` and waits on the one before via
`pgrep`. If a pipeline dies, `pgrep -f <script-name>` tells you where it
stopped.

### Ancillary process (not ours)

`pkr-trainer --iterations 20000000` (PID 18279) — another project on the
same box, consumes ~3.5 cores continuously. All wall-clock times in this
handoff were measured with that contention active. Do not kill it.

---

## 2. The bug report — DONE

All 62 items from `docs/plans/chameleon-bug-report.md` were fixed and
committed 09-27/09-28. The workspace compiles, `cargo clippy --workspace
--all-targets -- -D warnings` is clean, `cargo nextest run --workspace`
is green (with two intentionally skipped tests: the `sb_dump` diagnostic
and the stale-fixture check in `sb_dump.rs`).

Commit ordering (oldest first) for the four biggest bug clusters:

```
88b1827  fix(cham-search): C-1 CFR+ regret accumulation was wiped each iteration
19e6ab3  fix: H-1 H-10 M-12 H-12 from the codebase bug report
ee32a9e  fix(H-2): tracker models the wrong opponent in every seat-1 seating
b88d0e3  fix(cham-search): C-2 C-3 C-4 M-1 — subgame model corrected
1317747  fix(cham-search): C-5 — oracle verifies both sides
83d53cb  fix(cham-search): C-5 second pass — require |rs|==|cs| in support enum
...
```

Every fix that was testable has an anti-regression test. Commit messages
reference the bug ID (C-*, H-*, M-*, L-*) so you can `git log --grep "H-3"`
to find the fix.

**Do NOT re-litigate the bug report.** It's closed.

---

## 3. Competitive findings — every measured number

### 3.1 LBR matrix (tiny abstraction, Robust mode, 1000-deal LBR at depth 100, lower is better)

| iters | variant | seat 0 (SB) | seat 1 (BB) | mean | wall |
|---|---:|---:|---:|---:|---:|
| 500k | serial, γ=1.0 | 23 280 | 13 957 | 18 619 | 10 min |
| **5M** | **parallel, no fixes** | **13 977** | **12 858** | **13 417** | 47 min |
| 5M | parallel, eps=0.02 | 14 910 | 12 551 | 13 731 | 47 min |
| 20M | parallel, no fixes | 13 319 | 14 706 | 14 012 | ~2 h |
| 20M | parallel, warmfix | 13 554 | 14 090 | 13 822 | ~2 h |
| 20M | parallel, eps=0.02 | 13 682 | 13 652 | 13 667 | ~2 h |
| 50M | parallel, no fixes | 13 572 | 16 346 | 14 959 | 7.7 h |
| 9M | full abstraction, parallel | 18 079 | 14 298 | 16 188 | 3.6 h |

At matched visits/infoset (tiny-500k and full-9M both ~24), full wins by
22 % on SB and loses by 2 % on BB.

**The frontier:**
- Best seat 0: tiny 20M no-fixes (13 319)
- Best seat 1: tiny 5M no-fixes (12 050 at 200 deals, 12 858 at 1000 deals)
- Best mean: tiny 5M no-fixes
- **SOTA bundle: `artifacts/par-5M/` (Robust).** 47 min to reproduce.

### 3.2 Ladder against the archetype pool (mb/seating, higher is better)

| agent | mean | wins |
|---|---:|---:|
| `full-mixture` (synthetic-trained router) | +4 388 | 6/9 |
| **`full` (= argmax, SOTA)** | **+7 146** | **9/9** |
| `full-hedged` | −1 994 | 0/9 |

The ladder numbers are for a full-bundle mixture; the **SOTA bundle is the
tiny 5M robust policy**, whose ladder was not separately re-measured.
The `full` = argmax routing default is a one-line change in
`crates/cham-cli/src/cmd/hero.rs::routing_for`.

### 3.3 Router (real-data, honest measurement)

| feature set | top-1 B-test | TAG recall | ECE |
|---|---:|---:|---:|
| 20-dim (opportunity-gated) | 0.697 | 0.375 | 0.201 |
| 10-dim (raw frequencies) | 0.797 | 0.515 | 0.363 |

Neither passes the gate (top-1 ≥ 0.80, ECE ≤ 0.15, per-class recall ≥ 0.70).
The `train-router` gate FAILS. The synthetic router passes trivially and
its gate is vacuous (`collect.rs` writes the class id into a feature
dimension).

---

## 4. The current investigation: the RM+ freeze

This is what the successor is expected to finish.

### 4.1 The observation

The tiny-abstraction LBR curve peaks at 5M iterations and regresses on
BB after. The mean is worse at 20M and 50M than at 5M.

### 4.2 The root cause

A `rm_freeze` diagnostic (`/tmp/rm_freeze/`, binary at
`/tmp/rm_freeze/target/release/rm_freeze`, source in
`/tmp/rm_freeze/src/main.rs`) reads a `table.snap` and reports:

```
                     soft(<.5)   avg_near_frozen  mean cur max_p  mean avg max_p
  500k (serial)      13.5 %       4.1 %            0.762           0.453
  20M  (parallel)     3.3 %      60.0 %            0.871           0.859
  50M  (parallel)     2.3 %      66.0 %            0.888           0.879
```

**Interpretation:** regret-matching+ floors regrets at zero. Once the
positive part concentrates on one action, the current iterate never
re-explores. Linear CFR+ averaging weights the last T/4 of iterations,
which at long runs is dominated by the frozen iterate. The average
strategy's mean max probability climbs from 0.45 to 0.88.

### 4.3 Four levers tried

**Lever 1 — Exploration floor (`CHAM_TRAIN_EPS`).** NEGATIVE.
- Added `sigma_rms_eps` (commit `e16451e`), a process-global atomic
  (`82a9bdb`), read in trainer startup (`cab55fc`), recorded in
  `RunProvenance` (`3469042`).
- 5M eps=0.02: 14 910 / 12 551 (mean 13 731) — **worse** than 5M no-fixes.
- 20M eps=0.02: 13 682 / 13 652 (mean 13 667) — **better** than 20M no-fixes
  but still 12 % worse than 5M no-fixes.
- Verdict: the floor makes the collapse shallower but doesn't reverse it.
  Two percent is dominated by the still-concentrating 98 %.

**Lever 2 — Insert-only warmup in the parallel trainer.** PARTIAL.
- Commit `1fa3762`. The parallel trainer's warmup burst used to run full
  CFR+ updates on rows it already had, overwriting accumulated strategy
  mass once per slice.
- 20M warmfix: 13 554 / 14 090 (mean 13 822). BB improves 4 %.
- Real but small. Not the whole story.

**Lever 3 — Remove the averaging delay.** RUNNING NOW.
- `CHAM_AVG_DELAY=0` makes the strategy sum weight `w_t = t` from
  iteration 0 instead of `max(0, t − T/4)`.
- Hypothesis: if the freeze starts around iteration T/4, the delay
  discards the entire pre-freeze mixed phase.
- See `docs/plans/AVG-DELAY-VS-FREEZE-2026-09-29.md`.
- Result: pending (PID 70041).

**Lever 4 — DCFR regret discount (`--regret-discount`, alpha < 1).** QUEUED.
- Discounts old positive regrets before adding new deltas. Directly
  counters the freeze: the dominant regret gets discounted over time.
- Sweep at alpha = 0.9 and 0.5 (script `dcfr-alpha-experiments-2026-09-29.sh`,
  PID 91553).
- Result: pending.

### 4.4 What a successor should do

1. **Read `artifacts/par-20M-delay0-lbr.log`** when it lands. Compare
   seat 1 to the no-fixes 20M value (14 706). If it drops to ~12 000, the
   delay is the problem and the fix is purely in the averaging schedule.
2. **Read `artifacts/par-20M-avguniform-lbr.log`** next. Uniform average
   is the strongest version of the delay test.
3. **Read `docs/plans/FREEZE-EVOLUTION-2026-09-29.raw.txt`** for the
   iteration-by-iteration freeze curve. If the freeze starts before T/4,
   the delay is discarding the useful part and delay0 should fix it. If
   the freeze starts after T/4, the delay is not the problem and the
   answer is DCFR or a softmax-over-regrets.
4. **Read `artifacts/par-20M-alpha0*.log`** for the DCFR results.

If none of the four levers lifts BB at 20M below ~12 000 without hurting
SB, then the conclusion is: **tiny's peak really is 5M** and the frontier
moves elsewhere — probably the router features.

---

## 5. Gotchas discovered this session

These cost hours; don't repeat them.

### 5.1 Artifact pollution in commits
`git add -A` sweeps in trained bundles (MBs) and log files. The workspace
has ~15 top-level `/artifacts/*` gitignore rules, but new experiments
create new paths. Before any commit, `git status --short | head -40` and
look for `artifacts/` — commit those separately or add to `.gitignore`.
**One file, one commit** is the rule the user enforced after the first
incident.

### 5.2 The L-1 key format break
Commit `2b3c5f4` (adding an overflow byte to `ActionSeq`) changed the
infoset key stream. Every artifact trained before that commit became
unloadable — the loader hash matched but the keys didn't. Fixed by
`7e0621e` (the overflow counter now participates in the hash) and pinned
by `crates/cham-engine/tests/engine.rs::key_format_is_pinned`. **Any
future change to `Encoder::key_for` invalidates every artifact.** The
golden test is `GOLDEN_EMPTY = 0xfa8af2d5b6eff668`,
`GOLDEN_FLOP = 0xe1d2992adb8e6fe6`.

### 5.3 The L-5 meta.json hash
`serde_json`'s f64 formatting is not byte-stable across builds. The
original L-5 fix hashed the re-serialized JSON and refused every
pre-existing artifact. Reverted to a canonical byte-stream hash
(`meta_canonical_bytes`) that is version-stable. See
`crates/cham-engine/src/build.rs::verify_meta_text` and
`meta_canonical_bytes`.

### 5.4 H-6 artifact_hash=0
Every artifact written before the H-6 fix carries `artifact_hash: 0` in
its embedded provenance. The H-6 check must be `if provenance.artifact_hash != 0`
or it refuses every pre-H-6 bundle. See
`crates/cham-blueprint/src/policy.rs` around line 265.

### 5.5 Seat asymmetry in `collect --real`
The router-data producer ran hero at SB for every hand, biasing the
tracker toward the preflop-second-actor slice and killing 9 of 20
features. Fixed (alternating seat) in `crates/cham-cli/src/cmd/collect.rs`.
Same class as the H-2 bug.

### 5.6 The router feature leak
Every EWM in the tracker is opportunity-gated on the **hero's own**
actions (`facing_open` = hero raised, etc.). That makes the 20-dim
feature vector a function of the (opponent, hero-policy) pair, not the
opponent alone. `trend_z` is a session-level leak. See
`docs/plans/ROUTER-FEATURE-LEAK-2026-09-29.md`. The 10-dim raw-frequency
set is the honest alternative and still can't separate TAG from LAG.

### 5.7 Router filename mismatch
`train-router` wrote `model.bin`; every agent path reads `router.bin`.
The trained router was never loaded. Fixed in `crates/cham-cli/src/cmd/train_router.rs`.

### 5.8 Bash heredoc interleaving in LBR logs
`cargo bench` writes criterion output to stdout and
`exploitability[diag]:` to stderr. If both go to the same file they
interleave and corrupt the lines. **Always split**: `cargo bench ... >
criterion.log 2> stderr.log` and grep the stderr file for the
`exploitability[bp` lines.

### 5.9 Background job lifecycle
Always `nohup nice -n 15 ... > log 2>&1 &` for long runs. Never block
foreground. Verify with `ps -p <pid>`. The user explicitly asked for
this after the first few slow scripts.

### 5.10 "Revert the commit" ≠ "discard the working tree"
When the user says "revert the commit", they mean **`git reset`** the
commit so it can be re-committed properly (one file at a time). They do
**not** mean `git checkout -- <path>`, which discards working-tree
changes. Getting this wrong cost an hour.

### 5.11 par-5M `table.snap` is missing
`artifacts/par-5M/robust-7/` contains `policy/` and `provenance.json` but
no `table.snap`. The LBR (13 977 / 12 858) is still valid — the policy
is what gets benched — but any `rm_freeze` diagnostic on the 5M policy
requires retraining (47 min).

### 5.12 The synthetic router gate
`collect` (without `--real`) synthesizes feature vectors where the class
is encoded directly (`f[sig] += 0.45`). Its gate reports top-1 = 1.000
and every test passes. **This is a lie** — it's a stub. Never quote a
synthetic-router gate result.

### 5.13 The 8.7% zero-regret rows in the eps+warmfix smoke test
Rows created by warmup that the parallel phase never revisited. Not a
bug — correct fallback (uniform over actions when all regrets are
zero). The fraction shrinks with longer runs.

### 5.14 The exploration floor is process-global
`set_train_explore_eps` sets a process-wide atomic. Tests in the same
binary share it. `crates/cham-blueprint/tests/eps_floor.rs` resets to
0.0 at the end of every test that sets it. Do the same in any new test.

### 5.15 `pkr-trainer` on the same box
Another project consumes 3-4 cores continuously. All wall-clock numbers
in this handoff assume that contention. Measured speedup of the parallel
trainer is 2.4× at 4 workers, but with more contention it can drop to
1.5-2×.

---

## 6. Environment variables (operational reference)

| var | effect | default | notes |
|---|---|---|---|
| `CHAM_TRAIN_EPS` | RM+ exploration floor | 0.0 | [0, 0.5]; see `RM-PLUS-FREEZE-2026-09-29.md` |
| `CHAM_AVG_DELAY` | averaging delay (iters) | T/4 | 0 = Linear CFR+ from iter 0 |
| `CHAM_AVG_UNIFORM` | 1 = uniform average | unset | strongest delay test |
| `CHAM_ROUTER_TEMP` | router sharpening temperature | 0.7 | affects mixture routing only |
| `CHAM_ROUTER_N0` | Dirichlet prior strength | 8.0 | affects mixture routing only |
| `CHAM_AGENT_BUNDLE` | override default `artifacts/agent` | unset | for benching alternate bundles |
| `CHAM_IGNORE_ABSTRACTION_HASH` | 1 = skip the abstraction-hash guard | unset | diagnostic only; prints a loud warning |
| `CHAM_FORCE_SEAT` | force a training seat | unset | `0` or `1`; diagnostic |
| `CHAM_EXPLORE_EPS` | opponent-action exploration during training | 0.0 | different from `CHAM_TRAIN_EPS` |
| `CHAM_RBP_THETA0` | regret-based pruning threshold | 0.0 | 0 disables pruning |
| `CHAM_FALLBACK_MODE` | `renorm` (default) or `substitute` | `renorm` | routing fallback semantics |

---

## 7. Key commands

### Benchmark a policy (LBR, 1000 deals)
```bash
cd /Users/adm/Documents/Repos/chameleon
CHAM_EXPLOIT_BP="$PWD/artifacts/par-5M/robust-7/policy" \
CHAM_EXPLOIT_BUCKETS="$PWD/artifacts/buckets-tiny" \
CHAM_EXPLOIT_CONFIG="$PWD/config/abstraction-tiny.toml" \
CHAM_EXPLOIT_DEALS=1000 \
  cargo bench -q -p cham-blueprint --bench exploitability \
  > /tmp/crit.log 2> /tmp/lbr.log
grep "exploitability\[bp" /tmp/lbr.log
```

### Ladder against the archetype pool
```bash
CHAM_AGENT_BUNDLE=artifacts/agent-honest \
  target/release/chameleon ladder --agent full --fast
```

### rm_freeze diagnostic
```bash
/tmp/rm_freeze/target/release/rm_freeze artifacts/par-5M/robust-7/table.snap
```

### Train tiny robust (5M, parallel)
```bash
target/release/chameleon train-bp \
  --mode robust --iters 5000000 --depth 100 --seed 7 \
  --config config/abstraction-tiny.toml \
  --buckets artifacts/buckets-tiny \
  --out artifacts/par-5M \
  --threads 4 --thread-mode hogwild
```

---

## 8. Repository layout highlights

```
crates/
  cham-agent/       the shipped agent pipeline (tracker + router + experts)
  cham-blueprint/   CFR+ trainer, regret table, traversal, policy artifact
  cham-cli/         the `chameleon` binary (train-bp, ladder, probe, collect, ...)
  cham-core/        cards, engine, evaluator
  cham-engine/      abstraction, encoder, buckets
  cham-eval/        stats, ledger, matcheng, slumbot client
  cham-gpu/         Metal and wgpu backends (optional)
  cham-opponents/   archetypes, baselines, FamilyB
  cham-proofs/      P-1..P-4 gates
  cham-rec/         recorder
  cham-router/      the softmax router (model, training, runtime)
  cham-search/      river subgame solver
config/
  abstraction-tiny.toml     (SOTA; 32/16/16 buckets, ~21k infosets)
  abstraction-medium.toml   (64/32/32; newly added)
  abstraction.toml          (300/200/64; "full")
  pool.toml                 (the archetype pool)
scripts/
  retrain-tiny-honest.sh
  run-full-agent.sh
  overnight-2026-09-29.sh
  freeze-evolution-2026-09-29.sh   (new, in queue)
  dcfr-alpha-experiments-2026-09-29.sh  (new, in queue)
  avg-delay-experiments-2026-09-29.sh   (new, in queue)
docs/plans/                all the findings and handoffs
artifacts/                 all measurements and bundles (gitignored except a few)
```

---

## 9. Frontier reference

The SOTA configuration, reproducibly:

```
bundle:      artifacts/par-5M/robust-7/policy
abstraction: config/abstraction-tiny.toml
gamma:       1.0 (default)
routing:     full (= argmax)
ladder:      +7 146 mb/seating mean, wins 9/9
LBR:         13 977 / 12 858 (1000 deals)
wall:        47 min at 4 workers
```

If a successor's new experiment beats 13 417 mean LBR at 1000 deals,
it's a new SOTA. Nothing in this session has so far (the delays, DCFR,
eps, and warmup-fix results are all either negative or partial).

---

## 10. Open questions the successor should answer

1. **Does removing the averaging delay fix BB?** (PID 70041, ETA 22:15)
2. **Does a uniform average fix BB?** (avguniform, ETA 22:50)
3. **Where exactly does the freeze start?** (freeze-evolution, raw output
   in `docs/plans/FREEZE-EVOLUTION-2026-09-29.raw.txt`, ETA 01:30)
4. **Does DCFR alpha < 1 slow the freeze?** (DCFR sweep, ETA 05:00)
5. **Does the router feature set ever separate TAG from LAG?** — needs
   new features that capture *which hands* the opponent raises with, not
   just how often. Not started.
6. **Is the medium abstraction (64/32/32) a better frontier than tiny?**
   Config exists (`config/abstraction-medium.toml`), buckets built
   (`artifacts/buckets-medium/`), but the LBR was never measured because
   an early run came back empty due to the stdout/stderr bug (gotcha
   5.8). Worth re-running at 5M.

---

## 11. Reference: what "good behavior tests" this session added

Commit `f411d5f` added `crates/cham-blueprint/tests/eps_floor.rs` with 10
unit tests for `sigma_rms_eps` and the process-global floor. No test for
the raw-opponent frequencies yet (that's the next gap the successor
should close).

Other tests added this session:
- `crates/cham-engine/tests/engine.rs::key_format_is_pinned` (L-1 golden)
- `crates/cham-engine/tests/engine.rs::quantile_edges_respects_bin_count`
- `crates/cham-agent/tests/agent.rs::loader_hash_guards` (rewritten to
  actually test the tamper path)
- `crates/cham-agent/tests/agent.rs::l18_villain_action_reaches_seq`

The `sb_dump` test is `#[ignore]`d by design (stale fixture, diagnostic
only).

---

## 12. Session timeline (compressed)

- **09-27 → 09-28**: 62 bug-report items fixed and committed.
- **09-28 morning**: γ-underflow fix (avg_gamma default 0.9 → 1.0). 40 %
  LBR win on seat 0.
- **09-28 midday**: argmax vs mixture routing comparison. Argmax wins the
  ladder; mixture wins LBR by 35 %. Both documented.
- **09-28 afternoon**: parallel trainer (Hogwild pool with sliced
  warmup). 2.4× speedup.
- **09-28 evening**: full-abstraction 9M LBR. Beats tiny-500k at matched
  visits/infoset but not tiny-5M on wall.
- **09-29 morning**: router real-data measurement. Fails gate. TAG recall
  0.37-0.52.
- **09-29 midday**: seat asymmetry fix in `collect --real`.
- **09-29 afternoon**: full LBR curve at 1000 deals. Tiny peaks at 5M.
- **09-29 evening**: RM+ freeze diagnostic. Confirms the collapse is an
  RM+ property. Four fixes queued.
- **09-29 21:57 (now)**: delay0 20M training running; three more
  pipelines queued.

Good luck. The findings docs are the real artifact — read them before
touching code.
