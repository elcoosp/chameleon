# Overnight corrected-stack results (2026-10-01 → 10-02)

**Queue:** `scripts/overnight-2026-10-01-corrected-stack.sh`
**Ran:** 19:45 → 23:29 CEST, 2026-10-01.
**Status:** all five arms completed. First curve measured under the
F3+F4+F6a trainer with the F1 corrected metric.

## 1. The curve (seat 1)

| arm | iters | clairvoyant `lbr_vs` | tabular `tabular_br` |
|---|---:|---:|---:|
| tiny CFR+ (α=β=1) | 500k | 20 484.2 mb/hand (20.48 bb) | -1 593.6 (-1.59 bb) |
| tiny CFR+ | 5M | **14 205.3 (14.21 bb)** | -3 298.8 (-3.30 bb) |
| tiny CFR+ | 20M | 15 960.1 (15.96 bb) | -1 940.5 (-1.94 bb) |
| tiny DCFR(1.5, 0.0) | 20M | 15 856.7 (15.86 bb) | -1 548.8 (-1.55 bb) |
| medium CFR+ | 20M | 15 718.9 (15.72 bb) | -3 024.4 (-3.02 bb) |

All tabular numbers were measured at the metric test's default
`(300 train / 200 test / 12 sweeps)` budget. Per
`TABULAR-BR-CONVERGENCE-2026-10-01.md`, that budget is too low for
the tabular BR to be a stable lower bound — all five values are
negative, which a converged BR cannot be on both seats. **The
clairvoyant column is the load-bearing column for this table;**
the tabular column is only meaningful at a much larger budget.

## 2. What the curve says

### 2.1 The trainer converges, then plateaus

Tiny CFR+ at 500k → 5M drops from 20.48 to 14.21 bb/hand — a 6.3 bb
improvement, the strongest signal in the table. That is the
F3+F4+F6a trainer doing real work.

At 20M the clairvoyant metric is 15.96 — *worse* than the 5M value.
That is a 1.75 bb regression at 4x the iterations. Consistent with
the freeze investigation (`FREEZE-ONSET-AT-5M-2026-09-30.md`): the
policy reaches a low point around 5M and then degrades on the
clairvoyant metric. Tiny does **not** want 20M iterations.

### 2.2 DCFR(1.5, 0.0) does not help at 20M

Tiny CFR+ identity at 20M: 15.96 bb. Tiny DCFR(1.5, 0.0) at 20M:
15.86 bb. Difference: 0.1 bb — inside noise. The paper's recommended
α=1.5, β=0.0 schedule is **neutral** on this abstraction at this
budget, at least without the γ=2 (`avg_delay=0`) half of the schedule
which this queue did not test.

This is the first honest DCFR A/B: the old `DCFR-ALPHA09-NEGATIVE`
and `DCFR-ALPHA05-NEGATIVE` results tested the wrong discount with
the wrong metric, so they said nothing. This one says "no measurable
difference" — which is a real negative result.

### 2.3 Medium does not beat tiny at matched wall-time

Medium 20M: 15.72 bb. Tiny 5M: 14.21 bb. Medium is 1.5 bb *worse* on
the clairvoyant metric, despite 4x the infosets (163 787 vs 61 052)
and the same wall-time per arm (~51 min). This is consistent with
the medium-abstraction doc's own caveat (medium was intended to be
trained to matched **visits/infoset**, which 20M iters on medium
does not achieve — 163 787 infosets at 20M iters is ~122 visits /
infoset, comparable to tiny at 500k).

The correct medium comparison is at a higher iteration count that
matches tiny's 5M visits/infoset — but the plateau at 5M on tiny
suggests more iterations is not the answer either.

## 3. Caveats

- Seat 0 was not measured in the overnight script. Only the
  both-seats coverage experiment measured seat 0, and only on the
  5M policy. A successor should measure seat 0 for every arm.
- The DCFR arm did not set `CHAM_AVG_DELAY=0`, so it tested
  DCFR(1.5, 0.0, γ=default) not DCFR(1.5, 0.0, 2.0). The paper's
  schedule is three-parameter.
- The 500k and 5M arms ran on the pre-20:20 binary; since they use
  α=β=1.0, the F5 code path was a strict no-op, so this is valid.
  The 20M, DCFR, and medium arms ran on the post-20:20 binary.
- The metric budget (300/200/12) is too low for a stable tabular
  value, per the convergence finding.

## 4. What this tells us to do next

1. **Do not train tiny beyond 5M.** The 20M arms are all worse than
   the 5M arm on the clairvoyant metric. The freeze investigation
   already flagged this; the corrected stack confirms it. If a
   longer run is wanted, it should be a different abstraction or a
   different averaging schedule, not more iterations on tiny.
2. **DCFR is not a lever here.** At 20M the difference is 0.1 bb.
   Before spending more nights on DCFR arms, the γ=2 half must be
   tested — but on the current evidence the expected return is small.
3. **The honest number to quote for the shipped tiny policy is
   ~14.2 bb/hand clairvoyant at 5M.** Not 21.2 (the par-5M value),
   not 15.96 (the plateau). The 5M checkpoint is the best tiny point.
4. **The tabular column needs a bigger budget before it can be
   quoted at all.** The coverage experiment shows the sum
   `BR(0)+BR(1)` moves from -9.29 (300/200/12) to -1.45
   (5000/500/30) on the 5M policy. Until every arm in a curve is
   measured at the same, adequate budget, the tabular column is a
   relative indicator at best.

## 5. The one number that matters

The 5M tiny CFR+ policy measured **14.21 bb/hand clairvoyant** and
its corrected tabular BR at adequate budget (5000/500/30) was
**+0.58 bb for seat 1** (from the coverage experiment). That is the
current frontier: a policy that is ~14 bb exploitable under the
clairvoyant (perfect-information) metric but whose seat-1
infoset-consistent best response is near break-even.

The clairvoyant/actual gap is the same story the F1 doc told: the
clairvoyant metric systematically overstates how exploitable the
policy is by a large factor. The actual number on the 5M policy, at
an honest budget, is +0.58 bb seat 1 — a very different policy
description than "14 bb exploitable."

This is not "the policy is Nash". It is "the policy is much closer
to its own abstraction's equilibrium than the clairvoyant metric
implied", which is what the F1 correction was always saying.

## 6. Artifacts produced

| path | size | note |
|---|---:|---|
| `artifacts/par-f5-tiny-500000/` | 3.2M | 500k tiny CFR+ |
| `artifacts/par-f5-tiny-5000000/` | 4.7M | 5M tiny CFR+ (the best point) |
| `artifacts/par-f5-tiny-20000000/` | 5.1M | 20M tiny CFR+ |
| `artifacts/par-f5-tiny-dcfr15/` | 5.1M | 20M tiny DCFR(1.5, 0.0, γ=default) |
| `artifacts/par-f5-medium-20M/` | 9.8M | 20M medium CFR+ |

Each has a `-metric.log` with the seat-1 clairvoyant/tabular line.

## 7. How to reproduce the best point

    target/release/chameleon train-bp \
      --mode robust --iters 5000000 --depth 100 --seed 7 \
      --config config/abstraction-tiny.toml \
      --buckets artifacts/buckets-tiny \
      --out artifacts/par-f5-tiny-5000000 \
      --threads 4 --thread-mode hogwild

    # then the corrected metric at a proper budget:
    CHAM_EXPLOIT_BP=$PWD/artifacts/par-f5-tiny-5000000/robust-7/policy \
    CHAM_EXPLOIT_BUCKETS=$PWD/artifacts/buckets-tiny \
    CHAM_EXPLOIT_CONFIG=$PWD/config/abstraction-tiny.toml \
    CHAM_EXPLOIT_LABEL=par-f5-tiny-5000000 \
    CHAM_TBR_TRAIN=5000 CHAM_TBR_TEST=500 CHAM_TBR_SWEEPS=30 \
      cargo nextest run -p cham-blueprint \
        -E 'test(both_seats_tabular_br)' \
        --run-ignored all --no-capture

## 8. Corrections to the evening handoff

The 19:40 handoff (`HANDOFF-2026-10-01-EVENING.md`) claimed:

> The tiny-5M policy's corrected exploitability is probably ~1.5-2
> bb/hand on the tiny abstraction — competitive with a low-to-mid
> GTO approximation.

This queue shows the corrected tabular BR of the 5M policy at an
adequate budget is **+0.58 bb seat 1**, and the clairvoyant is
**14.21 bb**. The "~1.5-2 bb" figure was an eyeballed midpoint of
the 300/200/12 tabular value (+2.53 bb, which we now know is
budget-unstable); the honest statement is "0.58 bb at 5000/500/30,
budget-sensitive."

The handoff's headline direction (the bot is closer to Nash than
prior sessions thought) survives. The specific number does not.
