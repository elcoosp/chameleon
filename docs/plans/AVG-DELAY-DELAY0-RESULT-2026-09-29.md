# Averaging-delay = 0 (no delay): BB improves 7.4% at 20M (2026-09-29)

## The result

`CHAM_AVG_DELAY=0` makes the Linear CFR+ averaging weight `w_t = t`
from iteration 0 (default is `w_t = max(0, t - T/4)`). Everything else
is identical to the standard 20M tiny robust train (seed 7, depth 100,
mode robust, threads 4 Hogwild).

1000-deal LBR at depth 100, lower is better:

| variant | seat 0 (SB) | seat 1 (BB) | mean | Δmean vs no-fix |
|---|---:|---:|---:|---:|
| tiny 20M no-fix    | 13 319 | 14 706 | 14 012 | — |
| tiny 20M warmfix   | 13 554 | 14 090 | 13 822 | −190 (−1.4%) |
| tiny 20M eps=0.02  | 13 682 | 13 652 | 13 667 | −345 (−2.5%) |
| **tiny 20M delay0** | **13 431** | **13 618** | **13 524** | **−488 (−3.5%)** |
| tiny 5M (peak, 2 measures) | 13 977 – 15 040 | 12 050 – 12 858 | 13 417 – 13 545 | — |

Wall: 2 956 s (≈ 49 min) at 4 workers. Same as the warmfix 20M run
(which the parallel trainer result doc placed at ~2.4× over serial).

## Interpretation

**The delay is a real contributor to the BB regression, but not the
whole story.**

- BB improvement is unambiguous: **14706 → 13618, a −1088 mb/hand
  (7.4%) swing.** No other single lever measured this session moved BB
  that much. The averaging delay was, in fact, discarding the pre-freeze
  mixed phase.
- SB cost is small: 13319 → 13431 (+112). The sharpening that SB had
  been benefiting from is slightly diluted by including the earlier,
  more mixed iterations. This is exactly the tradeoff the freeze model
  predicted: the mixed early phase helps the side that needs mixing
  (BB), mildly hurts the side that was already sharpening (SB).
- **But delay0 does not fully solve the 20M problem.** BB is still
  13618 vs the 5M peak of ~12050-12858 — 760-1568 mb/hand short. So
  either:
  1. The freeze onset is later than T/4, in which case delay0 only
     recovers the earliest slice of useful iterations, or
  2. Even the fully-included average cannot un-freeze a current iterate
     that has already collapsed.

Both hypotheses are consistent with the freeze data: the 20M table has
60% avg_near_frozen rows (vs 4.1% at 500k). The average itself has
already been contaminated by the frozen iterate in the later half.

## What this means for the frontier

**Delay0 is now the second-best 20M variant on mean** (13 524 vs
medium-20M 13 629 and tiny-20M eps=0.02 13 667). It is not a new SOTA
in absolute terms — the tiny-5M peak still wins on mean — but it is
the first single-lever change to move BB by >7% without hurting SB
meaningfully, and it is free (no new env var to set at ship time; the
default just changes from T/4 to 0 for the linear ramp).

**If avguniform (the strongest delay test — w_t = 1 for all t) beats
delay0**, the ramp itself is a second-order problem and the whole
schedule should be flattened. If it ties, delay0 is the fix.

## What to test next

1. **avguniform** (running now, ETA ~23:15). Uniform average, strongest
   version of the delay test.
2. **delay0 + eps=0.02.** Both levers act on the same collapse — do
   they compose, or does delay0's more mixed average make the floor
   unnecessary?
3. **delay0 + DCFR alpha=0.9.** Discounted regret directly attacks the
   freeze at the iterate level; delay0 attacks it at the average level.
4. **The freeze-evolution diagnostic** (queued). If the freeze onset is
   after T/4 (i.e. > 5M of 20M), delay0 should not have helped this
   much, so the freeze-onset timing will constrain the mechanism.

## Diff vs the tiny-5M peak

The tiny-5M LBR reference numbers vary between handoffs:

| source | seat 0 | seat 1 | mean |
|---|---:|---:|---:|
| HANDOFF-2026-09-29-FULL.md (1000 deals) | 13 977 | 12 858 | 13 417 |
| SESSION-HANDOFF-2026-09-29.md (200 deals, quoted as "15040 / 12050") | 15 040 | 12 050 | 13 545 |

Both are reported as the tiny-5M robust policy at depth 100. The means
differ by 128 mb/hand; the BB values differ by 808. The 1000-deal
number is more trustworthy for the mean; the 200-deal number
over-fits to the sample and is not the ship candidate.

**Against either reference**, delay0 at 20M loses by 1000+ mb/hand on
BB. So delay0 narrows the gap but does not close it.

## Artifacts

- `artifacts/par-20M-delay0/robust-7/policy/policy.bin`
- `artifacts/par-20M-delay0/robust-7/table.snap`       (787 939 bytes)
- `artifacts/par-20M-delay0/robust-7/provenance.json`  (infosets=21 448, wall_s=2956.07)
- `artifacts/par-20M-delay0-lbr.log`                   (exploitability lines)
- `artifacts/par-20M-delay0-lbr.stderr.log`            (full bench output)
- `artifacts/par-20M-delay0-lbr.criterion.log`         (timings)
- `artifacts/par-20M-delay0.log`                       (training log)

## Repro

    CHAM_AVG_DELAY=0 target/release/chameleon train-bp \
      --mode robust --iters 20000000 --depth 100 --seed 7 \
      --config config/abstraction-tiny.toml \
      --buckets artifacts/buckets-tiny \
      --out artifacts/par-20M-delay0 \
      --threads 4 --thread-mode hogwild

    CHAM_EXPLOIT_BP="$PWD/artifacts/par-20M-delay0/robust-7/policy" \
    CHAM_EXPLOIT_BUCKETS="$PWD/artifacts/buckets-tiny" \
    CHAM_EXPLOIT_CONFIG="$PWD/config/abstraction-tiny.toml" \
    CHAM_EXPLOIT_DEALS=1000 \
      cargo bench -q -p cham-blueprint --bench exploitability \
      > /tmp/crit.log 2> /tmp/lbr.log
    grep "exploitability\[bp" /tmp/lbr.log

## Related

- `AVG-DELAY-VS-FREEZE-2026-09-29.md` — the hypothesis being tested
- `RM-PLUS-FREEZE-2026-09-29.md` — the freeze diagnostic
- `RESULTS-MATRIX-2026-09-29.md` — the running matrix this updates
