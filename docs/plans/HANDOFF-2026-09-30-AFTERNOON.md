# CHAMELEON — Session handoff, 2026-09-30 afternoon

**Written:** 2026-09-30 13:47 CEST.
**Supersedes for "what changed today":** `HANDOFF-2026-09-30-MORNING.md` (12:28).
The 09-29 handoffs (`SESSION-HANDOFF-2026-09-29.md`,
`SESSION-HANDOFF-2026-09-29-EVENING-ADDENDUM.md`) remain the primary
reference for the freeze investigation.

**HEAD at handoff:** `0300a5a docs(plans): 11-dim router NEGATIVE — tilt
scalar doesn't separate TAG/LAG`.

---

## 1. What is running

Nothing. All ladders and collections are done. The chameleon build is
idle. `pkr-trainer` (PID 32395) is running externally.

## 2. The single biggest finding of the day

**The shipped ladder is dominated by routing, not by training budget.**

| configuration | ladder mean (mb/seating) |
|---|---:|
| agent-honest / argmax+synthetic (SHIPPED) | **+7 136** |
| agent-honest / full-mixture | +4 388 |
| agent-honest / robust-only | +720 |
| agent-honest / full-hedged | +7 107 ≈ argmax |

**Routing lever: ~+6 400 mb/seating** (argmax vs robust-only, same
bundle). Every LBR improvement from this session (delay0, avguniform,
eps, delay0+eps02, warmfix, DCFR, medium abstraction) moves the
robust policy's *LBR* by at most 1 500. None of them has been
ladder-measured, so their effect on the shipped number is unknown.

See `LBR-VS-LADDER-2026-09-30.md` and `PAR5M-ROBUST-LADDER-2026-09-30.md`.

## 3. Findings that landed this afternoon

### 3.1 Hedged routing works and equals argmax (corrected)

The "hedged is a disaster" claim was an artifact of a CLI guard bug:
`TRAINED_AGENTS` in `crates/cham-cli/src/cmd/guard.rs` didn't include
`"full-hedged"` or `"hedged"`. Every prior hedged ladder silently
measured CallBot.

Fixed in `dd167df`. Corrected result (`ff9c57c`):

| mean | hedged | argmax | Δ |
|---|---:|---:|---:|
| +7 107 | +7 136 | −29 |

Statistically identical to argmax. See `HEDGED-FIXED-RESULT-2026-09-30.md`.

### 3.2 11-dim router NEGATIVE

Added the preflop/postflop tilt scalar from `params.rs` design
estimates; trained an 11-dim honest router on 120k rows. Result:

| metric | 10-dim | 11-dim |
|---|---:|---:|
| top-1 B-test | 0.797 | 0.766 |
| TAG recall | 0.515 | 0.517 |
| LAG recall | 0.584 | 0.503 |
| ECE | 0.363 | 0.305 |

Still fails the gate. See `ROUTER-11DIM-NEGATIVE-2026-09-30.md`.

### 3.3 Argmax fallback matters more than the metric showed

`probe --diag-fallback` on jamfix shows all 4 experts miss on 40/80
decisions; the action comes from `robust_sigma`, but `fallback_used`
is reset to false by the R3 policy. So the "expert miss + robust
cover" case is invisible in the current metric.

Added field `expert_missed_robust_covered` to `DecisionTrace`
(`5211684`) and an `rc_used` column to the probe output. On jamfix,
40/80 decisions are robust-covered. That's why the 5M-vs-500k
robust swap changes argmax on jamfix by −1 100.

See `ARGMAX-FALLBACK-REALLY-MATTERS-2026-09-30.md`.

### 3.4 Argmax mean correction (7136, not 6587)

The 09-28 SOTA doc's `argmax+synthetic` mean of +6 567 does not match
its own per-opponent column. Recomputing gives +7 120 for the doc's
column and +7 136 for the 2026-09-30 re-measurement. All session docs
using +6 567 / +6 587 have correction notes. See `24cb878`.

### 3.5 Hybrid result (500k vs 5M robust fallback)

Same bundle, only the robust slot differs:

- **mixture** mean +4 388 → +4 401 (+13, noise)
- **argmax**  mean +7 136 → +6 983 (−153)
- **robust-only** mean +720 → +820 (+100)

The 5M robust is LBR-better but *argmax-ladder-worse*. Effect is
opponent-specific: wins nit/tag/famB, loses jamfix/pnash by ~1 100 each.
See `HYBRID-LADDER-2026-09-30.md`.

## 4. Code fixes that landed (chronological)

| commit | what |
|---|---|
| `da90405` | M-6 warning is serial-only; provenance `threads` reflects real pool |
| `dcb96eb` | `--checkpoint-dir` exposed on train-bp |
| `d106161` | `sb_root_internals` diagnostic `#[ignore]`d |
| `0b1b4e9` | `tracker_raw_freq.rs` tests fixed (Player::Sb → Player::Bb) |
| `e6ffbc9` `4d4b0a7` | parallel trainer honors `checkpoint_every` |
| `8273a56` | `ladder-matrix` concurrency guard |
| `88b8b4a` | `modes.rs` accepts `hedged` routing |
| `6069578` `97a4e9e` | tests for `AgentMode::validate` routing set |
| `dd167df` | `TRAINED_AGENTS` includes full-hedged/hedged |
| `ec52ee6` `0c5cf08` | `CHAM_HEDGE_DEBUG` instrumentation (cached) |
| `66f8eac` | `Tracker::preflop_postflop_tilt` + tests |
| `a049b86` | `ChameleonAgent::opponent_only_features_11` |
| `b37b29b` | `collect --real --raw-opponent-11` flag |
| `5211684` | `DecisionTrace.expert_missed_robust_covered` + probe column |
| `8757718` | `weights_top_evolves` diagnostic test |

## 5. Known issues NOT fixed

- `river_eq_edges` mismatch in the full abstraction (see
  `RIVER-EQ-EDGES-MISMATCH-2026-09-30.md`).
- Watchexec has 5 processes watching `wr.sh`/`wr1.sh`; `wr1.sh` doesn't
  exist. The 3 `wr.sh` watchers race on every file write. This caused
  the duplicate-commit incident earlier today. **User should kill the
  redundant watchers.**
- `hero::build_hero` and `ladder.rs::run_opponent` both check
  `requires_trained_artifacts`. If the set is incomplete, the whole
  path silently degrades to CallBot. The new test
  `every_routable_agent_requires_artifacts` catches the specific set
  mismatch but not a *future* alias added to `hero::routing_for`.

## 6. What a successor should do next

**Highest value:**

1. **Measure the robust-only ladder for the 20M delay0+eps02 policy.**
   It has mean 13 429 LBR (tied with the tiny-5M peak). If it doesn't
   beat `agent-honest` robust-only (+720), the entire freeze
   investigation is off-target for the shipped ladder.

2. **Try stronger TAG/LAG features.** The 11-dim negative shows the
   raw-frequency path is exhausted. Two paths remain:
   - **Showdown-strength distribution** (invasive — needs the I9
     leak-rule audit).
   - **Bet-size histogram** (already in ActionSeq, needs tracker
     aggregation — a few hours).
   Either is likely necessary for the router gate to pass.

3. **Retrain the four experts at 5M each.** They're currently 500k.
   The 5M robustness was already measurable on the ladder. Retraining
   the experts at higher budgets is the *ladder-equivalent* of the
   20M work this session did for the robust policy.

**Medium-term:**

4. **Read the freeze-evolution raw output.** The freeze-diag now
   produces per-iteration snapshots (checkpoint fix), but the raw
   file is currently only a header — the run happened before the
   fix. Re-run the freeze-diag with the fixed binary (2-3 hours).

5. **Investigate `hedged` threshold semantics.** Hedged at default
   0.5 ≈ argmax; at 1.0 it should ≈ mixture. The corrected sweep was
   never run. Cheap (30 min).

**Not worth pursuing:**

- DCFR at alpha ≤ 0.9 (documented negative twice).
- Tilt scalar variants (the 11-dim negative covers this design space).

## 7. The frontier

**Shipping configuration:** `artifacts/agent-honest`, `--agent full`
(argmax+synthetic router, 4×500k experts + 500k robust). Ladder
+7 136, 9/9 wins. Never re-measured today because every worker
focused on other paths.

**The 4 alternatives that were measured and do NOT beat it:**
- mixture (+4 388)
- robust-only (+720)
- full-hedged (+7 107)
- hybrid 5M robust fallback argmax (+6 983)

**No new SOTA on the ladder this session.**

**LBR:** tiny 5M robust (13 977 / 12 858). **20M best:** delay0+eps02
(13 608 / 13 251, mean 13 429). Both are LBR-only metrics and have no
confirmed relationship to the shipped ladder.

## 8. Session-hygiene notes

- `wr.sh` was corrupted once (mid-session) by a heredoc whose
  contents contained backticks. Repaired by rewriting; watch for
  this. `<< 'EOF'` should prevent expansion but did not in one case.
- `setsid(1)` doesn't exist on macOS. Use
  `perl -e 'use POSIX qw(setsid); setsid; exec @ARGV' -- cmd args`
  or `nohup cmd &`.
- The `ladder` command is slow when `pkr-trainer` is running
  (contention). Expect 10-15 min per run instead of 6.
