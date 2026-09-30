# CHAMELEON — Session handoff, 2026-09-30 morning

**Written:** 2026-09-30 12:30 CEST.
**Supersedes for "what changed today":**
`SESSION-HANDOFF-2026-09-29-EVENING-ADDENDUM.md` (00:06) and
`HANDOFF-2026-09-29-FULL.md` (22:02).
The 09-29 handoffs remain the primary reference for the freeze
investigation; this doc covers what landed after.

**Repository root:** `/Users/adm/Documents/Repos/chameleon`
**Branch:** `main`
**HEAD:** see `git log --oneline -1`.

---

## 1. What is running RIGHT NOW

| PID | job | ETA | output |
|---|---|---|---|
| 77016 | `ladder --fast --agent full-mixture` on `agent-honest-5Mrobust` | ? | `artifacts/ladder-hybrid-5Mrobust-full-mixture.log` |
| 77054 | `ladder --fast --agent full-hedged` (threshold 0.00) on `agent-honest` | ? | `artifacts/ladder-hedge-thr0.00.log` |
| 32395 | `pkr-trainer --iterations 30000000` (external, high priority) | hours | `outputs/v40-k250/` |

The two `chameleon ladder` runs are slow because `pkr-trainer` is
consuming ~4-6 of the 8 cores. Elapsed 14 min for the first mode of
each pipeline where 6 min was typical yesterday.

All other pipelines are idle. The `ladder-hybrid` and `ladder-hedge-sweep`
scripts are the only active chameleon work.

---

## 2. The biggest finding of the day

**The LBR SOTA (par-5M robust) is NOT the ladder SOTA.** The shipping
bundle is `artifacts/agent-honest` with `--agent full` (argmax+synthetic),
which scores +6 587 mb/seating on `ladder --fast`. The par-5M robust
policy alone (which every session document has called "the SOTA") scores
only +820 on the same ladder. See `PAR5M-ROBUST-LADDER-2026-09-30.md`.

**Consequence for the freeze work:** every LBR improvement this session
(delay0, avguniform, eps, delay0+eps02) is in service of a metric that
does NOT match the shipping ladder. If the freeze work doesn't move the
ladder, it doesn't matter. Nobody has yet measured the LBR-improved
20M policy against the ladder.

**Re-measurement of the 09-28 SOTA** in
`LADDER-ARMMAX-REPRODUCED-2026-09-30.md` confirms the argmax+synthetic
configuration reproduces exactly on today's codebase (+6 587 vs +6 567).

---

## 3. All landing results this session (1000-deal LBR, depth 100)

| variant | SB | BB | mean | verdict |
|---|---:|---:|---:|---|
| tiny 5M no-fix (peak, shipping) | 13 977 | **12 858** | **13 417** | SOTA on mean |
| tiny 20M no-fix | 13 319 | 14 706 | 14 012 | baseline |
| tiny 20M warmfix | 13 554 | 14 090 | 13 822 | net win |
| tiny 20M eps=0.02 | 13 682 | 13 652 | 13 667 | net win |
| tiny 20M delay0 | 13 431 | 13 618 | 13 524 | best single-lever mean |
| tiny 20M avguniform | 13 976 | 13 133 | 13 555 | best BB at 20M |
| **tiny 20M delay0+eps02** | **13 608** | **13 251** | **13 429** | **ties 5M peak mean** |
| tiny 5M warmfix | 14 648 | 12 674 | 13 661 | net loss at 5M |
| medium 20M | 13 237 | 14 021 | 13 629 | best SB at 20M |
| tiny 20M alpha=0.9 | 35 344 | 25 834 | 30 589 | catastrophe |
| tiny 20M alpha=0.5 | 38 721 | 29 102 | 33 912 | worse still |
| full 9M | 18 079 | 14 298 | 16 188 | sample-starved |

Full matrix in `RESULTS-MATRIX-2026-09-29.md`.

**Delay0 + eps=0.02 is the new 20M SOTA on mean**, statistically tied
with the tiny-5M peak. SB better by 369, BB worse by 393. Choose per
ladder preference.

---

## 4. All landing results on the archetype ladder (mb/seating, 2500 deals/pair)

| bundle / routing | mean | wins | doc |
|---|---:|---:|---|
| agent-honest / full (argmax+synthetic) | **+6 587** | 9/9 | `LADDER-ARMMAX-REPRODUCED-2026-09-30.md` |
| agent-honest / robust-only | +720 | 3/9 | `PAR5M-ROBUST-LADDER-2026-09-30.md` |
| par-5M robust-only | +820 | 3/9 | same |
| agent-honest / full-mixture | +4 388 (09-28) | 6/9 | `SOTA-2026-09-28.md` |
| agent-honest / full-hedged | **−1 800** | 0/9 | `HEDGED-ROUTING-BUG-2026-09-30.md` |

**The routing lever is worth ~+5 900 mb/seating** (argmax vs robust-only,
same bundle). That is 10x larger than any training-budget lever
measured this session.

**Hedged routing loses structurally** — the confidence signal is a
session-level Dirichlet posterior, not a per-hand router confidence,
so hedged plays mixture-early, argmax-late. See
`HEDGED-ROUTING-BUG-2026-09-30.md`.

---

## 5. Code fixes that landed (chronological)

### 5.1 M-6 warning is serial-only (commit `da90405`)

The stale "trainer is single-threaded, no worker pool" warning was
firing on the parallel Robust path, which DOES spawn workers. Provenance
also hardcoded `threads: 1`. Both fixed.

### 5.2 `--checkpoint-dir` exposed on `train-bp` (commit `dcb96eb`)

CLI never declared it; the freeze-evolution script needed it.

### 5.3 `sb_root_internals` diagnostic `#[ignore]`d (commit `d106161`)

Trains 300k iters with no asserts; blocked the workspace test run.

### 5.4 `tracker_raw_freq.rs` tests fixed (commit `0b1b4e9`)

Three tests used `Player::Sb` (hero) as the opponent. Fixed to
`Player::Bb`; 14/14 green.

### 5.5 Parallel trainer honors `checkpoint_every` (commits `e6ffbc9`, `4d4b0a7`)

The parallel trainer never wrote iter-N checkpoint files. Fixed:
`slice_len = checkpoint_every` when the latter is set, so slice
boundaries align with checkpoint boundaries. Verified by smoke test
(500k iters, `--checkpoint-every 100000` → 5 files).

### 5.6 `modes.rs` accepts `hedged` routing (commit `88b8b4a`)

`AgentMode::validate` matched only 4 routing strings; `hedged` was
missing. The probe refused it with "unknown routing: hedged". Fixed
plus added `AgentMode::hedged()` constructor and 4 regression tests
(commit `6069578`, `97a4e9e`).

### 5.7 `ladder-matrix` concurrency guard (commit `8273a56`)

`mkdir "$LOCKDIR" || exit 0` at the top of the script so two
invocations can't race on the same log files (this bit us once today).

---

## 6. Known issues NOT fixed

### 6.1 `river_eq_edges` mismatch in the full abstraction

`artifacts/buckets-full/meta.json` has 17 edges but the config
declares `river_eq_bins=64` (should be 65). See
`RIVER-EQ-EDGES-MISMATCH-2026-09-30.md`. Fixing it invalidates every
full-abstraction policy ever trained (overnight-scale job). Tiny
buckets are consistent; not on the shipping path.

### 6.2 `freeze-diag` has never produced snapshots

The `freeze-diag-2026-09-29.sh` script ran successfully but wrote only
`artifacts/freeze-diag/robust-7/table.snap` (the final snapshot), no
iter-N files. This was because of 5.5. Now that 5.5 is fixed, the
freeze-evolution diagnostic should produce real iter-N output.

### 6.3 The `ladder-hybrid` pipeline lost a log to race

At 12:11 the hybrid pipeline's `full` mode log was truncated by a
concurrent duplicate I accidentally launched. The number (+8 587 was
not captured; the content we have says only the header). Same class
as the `ladder-matrix` race — the guard at 5.7 needs to be applied to
the hybrid script too.

---

## 7. What a successor should do next

**Immediate (in order):**

1. **Read `artifacts/ladder-hybrid-5Mrobust-full-mixture.log`** and
   `artifacts/ladder-hybrid-5Mrobust-full.log` when they finish. This
   answers: does upgrading the robust fallback from 500k to 5M
   change the shipped ladder? (Both are argmax on the same 4 experts;
   only the robust fallback slot differs.)

2. **Read `artifacts/ladder-hedge-thr*.log`** when the sweep finishes.
   Threshold=0.00 should match `full` (+6 587). If it doesn't, the
   hedged decision path differs from the argmax path in some other way
   — a second bug. Threshold=1.00 should match `full-mixture`.

3. **Measure the delay0+eps02 20M robust on the ladder** (LBR improved
   but never ladder-measured). If it doesn't beat agent-honest's robust
   (+720), the LBR work is off-target.

**Medium-term:**

4. **Router features for TAG/LAG separation.** The 10-dim honest set
   gets TAG recall 0.52, LAG 0.58 — far below the 0.70 gate. Needs
   raise-rate-by-street features or bet-size distributions. See
   `ROUTER-RAW-FEATURES-RESULT-2026-09-29.md`.

5. **Make hedged routing per-hand.** Use the router's sharpened prior
   entropy or the posterior variance `last_post_var` as the confidence
   signal instead of `weights[top]`.

6. **Investigate the `full-mixture` row loss.** The matrix doc has a
   missing row; the hybrid is running full-mixture against a different
   bundle. Re-run the matrix once locking is guaranteed.

**Longer-term:**

7. **Full-abstraction re-build.** Rebuild buckets-full with 65 edges,
   retrain the 4 experts + robust, re-run the 9M LBR. Overnight-scale.

8. **Medium abstraction at 5M** for comparison against tiny-5M at
   matched wall.

**Not worth pursuing:**

- DCFR at alpha ≤ 0.9 (documented negative twice).
- More tiny 20M LBR runs without a new lever.

---

## 8. The state of the frontier

**Shipping configuration:** `artifacts/agent-honest` with
`--agent full` (argmax+synthetic), tiny abstraction, γ=1.0.
Ladder: +6 587 mb/seating, 9/9 wins (reproduced today).
LBR (robust-only): 23 280 / 13 957 (200 deals, from the 09-28 SOTA doc).

**Best LBR:** tiny 5M robust (13 977 / 12 858) — but its ladder is
only +720, dominated by the mixture.

**Best 20M LBR:** delay0+eps02 (13 608 / 13 251, mean 13 429) — ties
the tiny-5M mean, better SB, worse BB. **Not ladder-measured.**

**No experiment this session has produced a new ladder SOTA.**

---

## 9. Session hygiene notes

- The `wr.sh` file was corrupted once today (mid-session) — apparently
  by a heredoc whose contents contained backticks that got expanded
  by the shell inside a `cat > "$DOC" << 'EOF'` block. The corruption
  was repaired at 12:20 by rewriting `wr.sh` with `bash -n` syntax
  check on the result. **Watch for backticks inside heredoc content**;
  `<< 'EOF'` (single-quoted) should prevent expansion but the
  corruption indicates the quoting was somehow broken.
- Duplicate `watchexec` processes (5 total, watching `wr.sh` and
  `wr1.sh`) can race on files. The `wr1.sh` watchers watch a file
  that doesn't exist. User should kill the redundant watchers to
  eliminate the race class.
- `setsid(1)` doesn't exist on macOS. `perl -e 'use POSIX qw(setsid); setsid; exec @ARGV'` works.
  `nohup ... & disown` works for a while, but watchexec's `--clear`
  may kill the process group. In practice, the detached children DID
  survive today; the earlier "deaths" were actually just slow starts.
