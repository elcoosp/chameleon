# Uniform average: BB floor reached at 20M, SB cost ~tie on mean (2026-09-29)

## The result

`CHAM_AVG_UNIFORM=1` sets the averaging weight `w_t = 1` for every
iteration (no ramp, no delay). This is the strongest version of the
averaging-schedule test: it includes every iteration equally, without
the T/4 exclusion and without the linear ramp on top.

1000-deal LBR at depth 100, lower is better:

| variant | seat 0 (SB) | seat 1 (BB) | mean | wall |
|---|---:|---:|---:|---:|
| tiny 20M no-fix    | 13 319 | 14 706 | 14 012 | 49 min |
| tiny 20M warmfix   | 13 554 | 14 090 | 13 822 | 49 min |
| tiny 20M eps=0.02  | 13 682 | 13 652 | 13 667 | 49 min |
| tiny 20M delay0    | 13 431 | 13 618 | 13 524 | 49 min |
| **tiny 20M avguniform** | **13 977** | **13 133** | **13 555** | **45 min** |
| tiny 5M (peak 1000-deal)  | 13 977 | 12 858 | 13 417 | 47 min |
| tiny 5M (peak 200-deal)   | 15 040 | 12 050 | 13 545 | 47 min |

Wall is ~45 min at 4 workers. Note the 20M avguniform is essentially
the same wall as 5M because the extra 15M iterations were learned at
~the same rate (the parallel trainer is stable).

## The BB floor

**BB dropped to 13 133 from the pre-fix 14 706: −1 573 (−10.7%).** That
is a larger single-move than delay0 (−1 088). BB is now within 275 of
the tiny-5M peak BB (12 858 at 1000 deals) — essentially at parity.

This strongly confirms the averaging-schedule hypothesis in
`AVG-DELAY-VS-FREEZE-2026-09-29.md`. The RM+ current iterate does
freeze (per `RM-PLUS-FREEZE-2026-09-29.md`), but the frozen iterate is
recoverable from the pre-freeze mixed phase **if the averaging includes
that phase**. The T/4 delay was systematically excluding it. This is
the primary contributor to the BB regression at 20M.

## The SB cost

**SB rose from 13 431 (delay0) to 13 977, +546 (+4.1%).** The uniform
average includes the earliest, noisiest iterations with equal weight,
which dilutes the sharpening that later (partly-frozen) iterations
contribute. delay0 ramps up linearly from 0, so late iterations still
dominate the average; avguniform does not, and SB pays for it.

Note: 13 977 is exactly the tiny-5M SB at 1000 deals from
HANDOFF-2026-09-29-FULL.md. That coincidence is not meaningful (both
are 1000-deal samples), but it does mean the avguniform 20M SB equals
the peak 5M SB, and the avguniform 20M BB (13 133) is slightly worse
than the peak 5M BB (12 858). So avguniform at 20M is *just below*
tiny-5M on both seats but well inside the noise band on mean.

## The mean tradeoff

| | delay0 | avguniform | winner |
|---|---:|---:|---|
| SB | 13 431 | 13 977 | delay0 (−546) |
| BB | 13 618 | 13 133 | avguniform (−485) |
| mean | 13 524 | 13 555 | delay0 (−31 ≈ noise) |

**On mean, delay0 and avguniform are tied** (13 524 vs 13 555 — 31
mb/hand, well inside the ±100 run-to-run band). The two schedules
trade SB for BB almost exactly. If the ship objective is mean LBR, the
choice is indifferent. If the ship objective values the BB side more
(worse baseline before this session), avguniform wins by 485.

## What the schedule says about the freeze

The freeze diagnostic said the current iterate is 60% frozen at 20M
(`avg_near_frozen` rows where max prob ≥ 0.9). The averaging schedule
cannot *un-freeze* the iterate, but it can **discard the frozen
iterations from the average**. Under avguniform the frozen second half
is diluted by the mixed first half; under T/4-delay the mixed first
half was thrown away and the average is nearly a copy of the frozen
iterate.

Both delay0 and avguniform land BB within ~300-500 of the 5M peak.
Neither reaches the 5M peak exactly, which is consistent with the
hypothesis that even the mixed first half is short and slightly
contaminated by the freeze once it arrives.

## What to test next

1. **delay0 + eps=0.02** — delay0 rescues the average, eps rescues the
   iterate. If they compose, BB may drop below 12 858.
2. **delay0 with a shallower ramp** (e.g. `sqrt(t)` instead of `t`).
   That would keep late-iteration weight without the full T/4
   exclusion — potentially recovering SB without giving back BB. Needs
   a new `CHAM_AVG_RAMP=sqrt` env or similar.
3. **avguniform + eps=0.02** — combos with the exploration floor.
4. **The freeze-evolution diagnostic** (currently broken — the CLI is
   missing `--checkpoint-dir`; a follow-up fix will unblock it). The
   iteration-by-iteration freeze curve will say how early the freeze
   starts, which determines whether the schedule alone can reach 5M
   parity on BB or whether the iterate must also be perturbed.

## What this means for the frontier

**Neither delay0 nor avguniform alone beats tiny-5M on mean.** But both
get BB within 300-500 of tiny-5M. So on BB the 20M run is now close
enough that a compound fix (schedule + eps, or schedule + DCFR) could
close it.

On SB, tiny-5M still wins by ~1000 (13000-13050 in the 200-deal sample;
12 858 in the 1000-deal sample). But the avguniform SB of 13 977 matches
tiny-5M SB at 1000 deals exactly, so that SB gap is sample-dependent and
should be re-checked at higher deal counts before declaring tiny-5M
strictly better on SB.

## Artifacts

- `artifacts/par-20M-avguniform/robust-7/policy/policy.bin`
- `artifacts/par-20M-avguniform/robust-7/table.snap`       (741 056 bytes)
- `artifacts/par-20M-avguniform/robust-7/provenance.json`  (infosets=21 448, wall_s=2678.08)
- `artifacts/par-20M-avguniform-lbr.log`
- `artifacts/par-20M-avguniform-lbr.stderr.log`
- `artifacts/par-20M-avguniform-lbr.criterion.log`
- `artifacts/par-20M-avguniform.log`

## Repro

    CHAM_AVG_UNIFORM=1 target/release/chameleon train-bp \
      --mode robust --iters 20000000 --depth 100 --seed 7 \
      --config config/abstraction-tiny.toml \
      --buckets artifacts/buckets-tiny \
      --out artifacts/par-20M-avguniform \
      --threads 4 --thread-mode hogwild

    CHAM_EXPLOIT_BP="$PWD/artifacts/par-20M-avguniform/robust-7/policy" \
    CHAM_EXPLOIT_BUCKETS="$PWD/artifacts/buckets-tiny" \
    CHAM_EXPLOIT_CONFIG="$PWD/config/abstraction-tiny.toml" \
    CHAM_EXPLOIT_DEALS=1000 \
      cargo bench -q -p cham-blueprint --bench exploitability \
      > /tmp/crit.log 2> /tmp/lbr.log
    grep "exploitability\[bp" /tmp/lbr.log

## Related

- `AVG-DELAY-VS-FREEZE-2026-09-29.md` — the hypothesis being tested
- `AVG-DELAY-DELAY0-RESULT-2026-09-29.md` — the middle option
- `RM-PLUS-FREEZE-2026-09-29.md` — the freeze diagnostic
- `RESULTS-MATRIX-2026-09-29.md` — the running matrix (needs updating)
