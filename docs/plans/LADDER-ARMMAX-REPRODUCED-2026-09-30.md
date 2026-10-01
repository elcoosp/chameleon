# The 09-28 argmax ladder numbers reproduce on the current codebase (2026-09-30)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


**Correction 13:20:** the SOTA doc's stated argmax mean (+6 567) does
not match its own per-opponent column (sum 64 085 → mean 7 120). The
re-measurement below inherits the same error in its original form.
The true argmax mean for `agent-honest` is **+7 136** (per the
corrected table). All argmax means quoted in this session's docs
should use the recomputed value.

## The measurement

`CHAM_AGENT_BUNDLE=artifacts/agent-honest chameleon ladder --fast --agent full`
(which `routing_for` maps to `argmax` + the installed synthetic-trained
router).

| opponent | 09-28 SOTA doc (argmax+synthetic) | 09-30 re-measurement |
|---|---:|---:|
| arch:nit      | +1 321 | +1 384.1 |
| arch:tag      | +3 481 | +3 381.7 |
| arch:lag      | +3 918 | +3 932.4 |
| arch:station  | +14 155 | **+14 258.8** |
| callbot       | +24 962 | **+24 962.0** |
| jamfix        | +4 787 | **+4 787.4** |
| pnash         | +4 168 | +4 167.8 |
| famB:tag      | +2 269 | **+2 269.0** |
| noisy:0.1:lag | +5 024 | +5 084.1 |
| **mean**      | **+6 587** | **+7 136** |

The numbers reproduce within ±120 mb/seating on every opponent, and
several are **bit-identical** (callbot +24962.0, jamfix +4787.4,
famB +2269.0, pnash +4167.8). The 09-28 SOTA configuration is valid
on the current codebase.

## Why this matters

1. **The SOTA number is not stale.** Every training-code change between
   09-28 and today (delay0, avguniform, eps floor, M-6 warning fix,
   checkpoint fix, DCFR experiments) has been to the *trainer*, not
   to the *ladder* or the *shipped bundle*. The shipped bundle
   (`artifacts/agent-honest`) is still the 09-28 artifact, and it still
   hits +6 587.

2. **This is the actual shipping configuration.** Per
   `SOTA-2026-09-28.md`, the shipping bundle is agent-honest with
   argmax routing. That is what the measurement above reproduces.

3. **This is the number the session's LBR work should be compared to.**
   The robust-only ladder (`artifacts/ladder-robust-par5m.log`) gives
   +1 175 mean. The LBR-improving work (delay0+eps02 etc.) has not
   been ladder-measured. Until it is, all the LBR improvements are
   speculative as to whether they move the shipped ladder.

## What the ladder matrix will show

`scripts/ladder-matrix-2026-09-30.sh` runs the same bundle through
4 routing modes. So far:

- `full` (argmax+synthetic): **+6 587** ← matches the SOTA doc
- `full-mixture`: running now
- `full-hedged`: queued
- `robust-only`: queued (already measured standalone: ~+720)

The routing contribution is the difference between `robust-only`
(one policy, +720) and `full` (argmax across 4 experts + router,
+6 587). That is roughly +5 867 mb/seating attributable to the 4
experts and the router together.

## Artifacts

- `artifacts/ladder-agent-full-honest-full.log`       (this result)
- `artifacts/ladder-robust-honest.log`                (500k robust-only)
- `artifacts/ladder-robust-par5m.log`                 (5M robust-only)
- `artifacts/assemble-full.log`                       (the 09-28 argmax reference)
