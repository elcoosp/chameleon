# Session summary — 2026-09-30

This session covered roughly 13:00–15:15 CEST. It ran alongside the
morning's post-12:00 retrain of the 4 tiny experts at 5M each (still
running) and the 19-dim honest router collection (started 15:11).

## The single most important finding

**The synthetic router is degenerate.** `agent-honest/router.bin`
picks class 2 (LAG) on every decision of every hand. So the "argmax
routing" in the shipped configuration is actually "play the LAG
expert every hand". The 09-28 SOTA doc's +6 587 mean is really the
LAG expert's ladder score. See
`SYNTHETIC-ROUTER-IS-DEGENERATE-2026-09-30.md`.

## The freeze investigation, closed

**The RM+ freeze onset is at 5M iterations (T/4 for a 20M run).**
The freeze-evolution diagnostic measured it directly:

| iter | mean avg max_p |
|---:|---:|
| 2M | 0.397 |
| 4M | 0.398 |
| 6M | 0.828 (post-freeze) |
| 20M | 0.853 |

The Linear CFR+ `T/4` delay is exactly the freeze onset. delay0 and
avguniform recover the mixed pre-freeze phase in the average. The
BB LBR improves 7–11%. See `FREEZE-ONSET-AT-5M-2026-09-30.md`.

**But the LBR improvement does NOT translate to the ladder.**
`20M-LADDER-NEGATIVE-2026-09-30.md`: the 20M delay0+eps02 policy has
the same LBR mean as the 5M peak (13 429 vs 13 417) but a *worse*
robust-only ladder mean (+624 vs +820). LBR and the archetype ladder
are decoupled.

**Three independent examples of the decoupling**:

1. 500k robust vs 5M robust: 10x LBR difference, tiny ladder difference
2. 5M robust vs 20M delay0+eps02: tied LBR, 5M wins ladder by +196
3. 500k robust vs 5M robust as argmax fallback: 5M wins LBR, loses
   ladder by −153

## Routing dominates the shipped ladder

| configuration | ladder mean |
|---|---:|
| agent-honest / full (argmax+synthetic) | **+7 136** |
| agent-honest / full-hedged | +7 107 |
| agent-honest-5Mrobust / full | +6 983 |
| agent-honest / full-mixture | +4 388 |
| agent-honest-5Mrobust / full-mixture | +4 401 |
| agent-honest / robust-only | +720 |
| 20M delay0+eps02 / robust-only | +624 |

The single biggest lever is routing (argmax vs robust-only): ~+6 400.
The 5M robust fallback upgrade is worth +13 on mixture, −153 on argmax.
The 20M LBR improvement is worth −196 on robust-only.

## The router search: 10 → 11 → 19 dims

| feature set | top-1 B-test | TAG recall | LAG recall | ECE | gate |
|---|---:|---:|---:|---:|---|
| 20-dim opportunity-gated (leaky) | 0.697 | 0.375 | 0.514 | 0.201 | FAIL |
| 10-dim raw opponent | 0.797 | 0.515 | 0.584 | 0.363 | FAIL |
| 11-dim + tilt | 0.766 | 0.517 | 0.503 | 0.305 | FAIL |
| **19-dim + bet-size histogram** | pending | pending | pending | pending | pending |

The 19-dim collection is running now (started 15:11). Training is
queued behind it. See `ROUTER-BET-SIZE-FEATURE-DESIGN-2026-09-30.md`.

The bet-size histogram is the *last* cheap feature on public history.
The next one (showdown-strength distribution) requires an I9
leak-rule audit. If 19-dim fails, the router is blocked on a
non-trivial infrastructure change.

## The hedged routing correction

Every "hedged is a disaster" number from the morning was actually
CallBot-vs-pool. The CLI guard `TRAINED_AGENTS` didn't include
`"full-hedged"` / `"hedged"`, so the ladder silently fell through to
the baseline. Fixed in `dd167df`. Corrected: hedged at default
threshold 0.50 is statistically identical to argmax (+7 107 vs +7 136).
See `HEDGED-FIXED-RESULT-2026-09-30.md`.

## The argmax-fallback metric correction

`probe --diag-fallback`'s `fb_used` column only counts decisions
where BOTH the picked expert AND robust missed. When only the expert
missed and robust covered, the action comes from robust but
`fallback_used` is reset to false. Added
`DecisionTrace.expert_missed_robust_covered` and a new `rc_used`
column. On jamfix, 40/80 decisions are robust-covered. See
`ARGMAX-FALLBACK-REALLY-MATTERS-2026-09-30.md`.

## What's still in flight

- `retrain-tiny-5M-experts` (nit expert at 8+ min elapsed, ETA ~40 min total)
- `collect --raw-opponent-19` (started 15:11)
- `train-router-19dim-queued` (waits for collect)
- `hedge-sweep-queued` (waits for retrain)
- `ladder-per-expert-queued` (waits for retrain)

## Session hygiene notes

- `wr.sh` got corrupted once mid-morning (backticks in a heredoc).
  Repaired by rewriting; single shebang confirmed.
- Watchexec duplicates: 5 processes, 2 watching nonexistent `wr1.sh`.
  User should kill the redundant watchers to eliminate the race class
  that produced the duplicate-commit incident at 12:24.
- macOS bash 3.2: no `mapfile`, no `setsid`. Use
  `perl -e 'use POSIX qw(setsid); setsid; exec @ARGV'` for
  session-leading children.

## The frontier

**Shipping configuration:** `artifacts/agent-honest`, `--agent full`
(= "play the LAG expert" per the degenerate-router finding). Ladder
+7 136, 9/9 wins. Never re-measured today.

**No new SOTA this session.**

**LBR:** tiny 5M robust (13 977 / 12 858). 20M delay0+eps02 ties its
mean but is worse on the ladder. The freeze work is closed as an
LBR-only optimisation with no ladder payoff.

## Recommended next steps

1. **Read the 19-dim router gate result** when `train-router-raw-19.log`
   lands. If it passes, install in a new bundle and ladder-compare.
   If it fails, the router is blocked on showdown-strength features
   (an I9 audit).
2. **Read the 5M-expert ladder** when the retrain finishes. The
   retrain replaces the 4 experts while keeping the robust and the
   router. If argmax on the new bundle beats +7 136, the expert
   upgrade is the confirmed next frontier lever.
3. **Read the per-expert ladders** to see if LAG is uniquely good.
4. **Do NOT re-run the freeze levers.** They optimize LBR, which is
   decoupled from the shipped ladder.
