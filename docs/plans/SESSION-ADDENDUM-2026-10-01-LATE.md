# Session addendum — 2026-10-01 late (post-19:40)

**Written:** 2026-10-01 20:05 CEST.
**Supersedes for "what was actually verified":** the evening handoff.
The handoff (19:40) was written before the session's three biggest
discoveries; this addendum records what was found by inspecting the
running overnight queue and re-running the corrected metric.

## 1. The evening handoff's three incorrect claims

The 19:40 handoff (`HANDOFF-2026-10-01-EVENING.md`) claims, in §2 and §4:

1. "F5 (proper DCFR) landed — CLI flags `--dcfr-alpha`, `--dcfr-beta`."
2. The overnight corrected-stack queue would sweep DCFR(1.5, 0, 2).
3. The F1 corrected metric's headline (21199.7 / 2532.6 / 8.37x) is
   reproducible.

**Claim 1 was false until 20:00.** The F5 commit (`56233cd`) added
`dcfr_alpha`/`dcfr_beta` fields to `TrainerConfig` and to the three
struct literals in `crates/cham-cli/src/cmd/train_bp.rs`, but never
added the clap `#[arg]`s. The CLI silently had no `--dcfr-alpha` flag.
Fixed this session in `crates/cham-cli/src/main.rs` and
`crates/cham-cli/src/cmd/train_bp.rs` (commits `9e57650` and the
main.rs commit that preceded it).

**Claim 2 is still not quite right.** The overnight script's
`train_tiny_dcfr()` passes `--dcfr-alpha 1.5 --dcfr-beta 0.0` but does
not set `CHAM_AVG_DELAY=0`. Per the F5 doc itself, the paper's
recommended schedule is α=1.5, β=0.0, γ=2.0 where γ=2 corresponds to
`avg_delay=0`. So even with the flags fixed, tonight's DCFR arm is
only one third of the paper's schedule.

**Claim 3 reproduces exactly.** `par5m_metric_compare` on
`artifacts/par-5M/robust-7/policy` prints clairvoyant 21199.7 mb/hand,
tabular 2532.6 mb/hand, ratio 8.37x — matching the handoff's
21200 / 2533 / 8.4x. `tabular_br` is correct; the F1 fix is real.

## 2. A new observation: negative tabular BR on an undertrained policy

The 500k arm of the overnight curve (started 19:40, finished 19:45)
returned clairvoyant 20484.2 mb/hand vs tabular -1593.6 mb/hand. The
old ratio print then divided by `tab.max(1e-9)`, producing a nonsense
20484176173864.12x. Two facts worth recording:

1. A best response for seat 1 can legitimately be negative.
   `tabular_br` returns the value of the best fixed action choice per
   infoset for the BR seat. On a sufficiently undertrained policy
   under the tiny abstraction, that value can be negative. It is not
   "exploitability is negative"; it is that the fixed-choice BR earns
   less than zero against this specific policy.
2. The ratio print was wrong for negative tabular BR. Fixed this
   session: the ratio now prints `n/a` when tabular ≤ 0.

The `tabular_le_clairvoyant_on_uniform` test passes trivially in the
negative case. It does not pin the sign, and shouldn't — a negative
tabular BR is a legitimate signal that the policy under test is
underserving seat 1 in a way the abstraction cannot recover.

## 3. What the running overnight queue is actually measuring

- 500k arm: finished 19:45, F3+F4+F6a under CFR+ identity.
- 5M arm: started 19:46, still running at 20:05. Same binary
  (pre-CLI-fix, post-F5-trainer; with α=β=1.0 the F5 changes are a
  no-op, so this tests the F3/F4/F6a trainer).
- 20M arm: queued, same as 5M.
- DCFR arm: queued, will parse now (binary rebuilt at 20:00) but is
  still missing `CHAM_AVG_DELAY=0`, so it is DCFR(1.5, 0, γ=default)
  not DCFR(1.5, 0, 2).
- Medium 20M arm: queued last.

## 4. What was landed this session (late, after the 19:40 handoff)

| commit | what |
|---|---|
| de398ad | docs: caveat clairvoyant LBR in RESULTS-MATRIX-2026-09-29.md |
| a202651 | docs: caveat clairvoyant LBR in TINY-CURVE-COMPLETE-2026-09-29.md |
| 20183ef | docs: caveat clairvoyant LBR in TINY-PEAKS-AT-5M-2026-09-29.md |
| 7653854 | docs: caveat clairvoyant LBR in RM-PLUS-FREEZE-2026-09-29.md |
| 082ae44 | perf(cham-blueprint): ArrayVec opponent dist (F9 sites 1/2) |
| main.rs | feat(cham-cli): wire --dcfr-alpha/--dcfr-beta into TrainBp |
| 9e57650 | feat(cham-cli): accept dcfr_alpha/beta in train_bp::run |
| 9a5283b | test(cham-blueprint): env label + ratio guard in par5m_metric_compare |

F9-alloc sites 1/2 (handoff §5.6) are DONE. Both
`Vec<(Action, f64)>` at `traversal.rs:292` and `:365` are now
`ArrayVec<(Action, f64), 12>`.

## 5. What to do next, updated

- §5.1 (caveat historical docs): only the four "important" docs were
  done. The `grep -L` list from this session shows ~50 more docs
  referencing LBR without the caveat. A follow-up script can batch
  them.
- §5.3 (DCFR A/B): the flags now work, but the DCFR arm needs
  `CHAM_AVG_DELAY=0` for the γ=2 arm. The F5 doc's recommended sweep
  is three arms; the overnight script has one.
- §5.5 (F6c off-tree translation): not started. Half-day item.
- §5.6 (F9-alloc sites 1/2): DONE this session.
- New: the 500k arm's negative tabular BR is worth a second look once
  the 5M and 20M arms finish. If they too produce negative tabular
  BR, the tiny abstraction may not be rich enough to express a
  profitable BR against undertrained policies.

## 6. Repository state at end of this session

    git log --oneline -1
    # 9a5283b (or later)

    git status --short
    # clean except artifacts/ledger and this addendum before its commit

Still nothing to push until the user says so.

## 7. New finding: tabular BR worsens as the corrected trainer improves

The overnight curve (still running at 20:30) is producing a pattern that
neither the handoff nor the F1 metric doc anticipated. The two arms
that have finished so far:

| arm | clairvoyant (bb/hand) | tabular (bb/hand) |
|---|---:|---:|
| 500k | 20.48 | -1.59 |
| 5M   | 14.21 | -3.30 |

The **clairvoyant** metric improves by 6.3 bb/hand as the trainer goes
from 500k to 5M iterations under the new F3+F4+F6a code path. That is
the expected direction: the new average-accumulation site is a real
trainer fix, and more iterations are finding a better policy.

The **tabular** metric moves in the *opposite* direction: from -1.59
to -3.30 bb/hand. A best response's value against a policy that is
getting closer to Nash should *decrease* toward the game value — so
the direction is correct. The magnitude raises the question of what
the game value of the tiny abstraction is for seat 1.

Two explanations are consistent with the data:

1. **The tiny abstraction's game value for seat 1 is genuinely
   negative in the -1 to -3 bb/hand range.** A coarse abstraction can
   produce large equilibrium losses for one seat, especially when the
   ladder cap is 1 (no 3-bets below all-in — see the report's F6a).
   If this is the case, both -1.59 and -3.30 are correct: they say
   "the BR seat cannot recover the abstraction's structural loss",
   which is consistent with the report's diagnosis.
2. **`tabular_br` has a perspective or sign bug that does not show
   on the par-5M artifact.** Possible but less likely: the same
   function produces a clean +2532.6 on par-5M under identical inputs.

To distinguish, the successor should:

- Measure `tabular_br` on the uniform policy (already done at 2.8
  bb/hand at depth 50) **and** on the shipped `agent-honest-19dim`
  bundle at depth 100. If the shipped bundle also produces a negative
  seat-1 tabular BR, explanation (1) is confirmed and the number is
  not a bug.
- Measure seat 0 as well. The F1 metric doc only ever reports seat 1.
  If seat 0's tabular BR is strongly positive (say +5 or more), the
  abstraction is unbalanced and seat 1's negative value is a
  structural property, not a measurement error.
- If explanation (1) holds, the handoff's headline ("2.53 bb/hand on
  par-5M, competitive with low-to-mid-strength GTO") needs revision:
  2.53 is the par-5M number under the **old** trainer. The new
  trainer's artifacts produce negative seat-1 tabular BR, which is a
  different statement about the same game.

**This is the most important open question of the session.** Until it
is resolved, no corrected-metric number from the overnight curve
should be quoted as "exploitability" without the seat-1 sign caveat.
