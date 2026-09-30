# delay0 + eps=0.02: mean tied with tiny-5M peak, different seat balance (2026-09-30)

## The result

The two best single levers on the 20M BB regression were:

- `CHAM_AVG_DELAY=0`: SB 13 431 / BB 13 618 / mean 13 524 (BB −7.4%)
- `CHAM_TRAIN_EPS=0.02`: SB 13 682 / BB 13 652 / mean 13 667 (BB −7.2%)

They act on different mechanisms (average weight schedule vs. current-
iterate exploration floor), so a combined run tested whether they
compose.

1000-deal LBR at depth 100, lower is better:

| variant | SB | BB | mean |
|---|---:|---:|---:|
| tiny 20M no-fix     | 13 319 | 14 706 | 14 012 |
| tiny 20M warmfix    | 13 554 | 14 090 | 13 822 |
| tiny 20M eps=0.02   | 13 682 | 13 652 | 13 667 |
| tiny 20M delay0     | 13 431 | 13 618 | 13 524 |
| tiny 20M avguniform | 13 977 | 13 133 | 13 555 |
| **tiny 20M delay0+eps02** | **13 608** | **13 251** | **13 429** |
| **tiny 5M peak (1000 deals)** | 13 977 | **12 858** | **13 417** |

Wall: ~44 min at 4 workers (parallel trainer).

## The comparison to tiny-5M peak

| metric | delay0+eps02 (20M) | tiny 5M peak | winner |
|---|---:|---:|---|
| SB | **13 608** | 13 977 | delay0+eps02 by 369 |
| BB | 13 251 | **12 858** | 5M peak by 393 |
| mean | 13 429 | **13 417** | tie (Δ=12) |

**The mean is a tie.** The two configurations trade SB for BB almost
exactly. delay0+eps02 is the first 20M variant to *match* the tiny-5M
peak mean, and it does so with a different seat balance: better SB,
worse BB. Whether it ships depends on which seat matters more for the
ladder (SB is the first to act, so it has more impact on the
matched-pair swing).

## The freeze diagnostic

        rows (w>=2):      21444
        frozen (one-hot): 6339 (29.6%)
        near-frozen (.9+):6829 (31.8%)
        soft (<.5):       780 (3.6%)
        --- AVERAGE strategy ---
        avg_near_frozen:  11383 (53.1%)
        mean cur max_p:   0.867
        mean avg max_p:   0.837

This is close to the no-fix 20M diagnostic (avg_near_frozen 60%,
mean avg max_p 0.859). The combination did NOT dramatically un-freeze
the table. And yet the LBR improved by 583 mb/hand on mean vs no-fix.

This is the same lesson as the DCFR negatives: **the freeze diagnostic
is not a proxy for policy quality.** The combination recovered most of
the BB regression without changing the freeze metric much. The
mechanism is subtler than "un-freeze the table".

The mechanism is probably: delay0 changes *which iterations* dominate
the average, and eps adds a floor to those iterations' sigma_rms. The
table itself may look frozen, but the *policy the average reads out* is
different (more mixing in the rows that matter for BB).

## What this means

1. **The frontier has moved at the 20M budget.** delay0+eps02 mean
   13 429 is the best 20M mean measured. It ties the tiny-5M peak on
   mean; it loses on BB but wins on SB.

2. **The 5M peak is no longer uniquely best.** For a ship candidate,
   either 5M no-fix (best mean, faster train) or delay0+eps02 20M
   (equal mean, better SB, 44 min) is defensible.

3. **The full recovery of BB to 5M parity (12 858) is still not
   achieved.** BB's best 20M is 13 133 (avguniform alone). Combining
   levers has now been tried (delay0+eps02) and produces 13 251 —
   *worse than avguniform's 13 133*. The levers do not compose on BB.

4. **A targeted BB lever would be a new direction.** The schedule
   fixes and the eps floor are global. The remaining BB regression
   (~400 mb/hand) is on a small subset of rows where equilibrium
   needs mixing. Per-row or per-street exploration, or a *tail-only*
   avg schedule, might close it. Not tested.

## Artifacts

- `artifacts/par-20M-delay0-eps02/robust-7/policy/policy.bin`
- `artifacts/par-20M-delay0-eps02/robust-7/table.snap`
- `artifacts/par-20M-delay0-eps02-lbr.log`
- `artifacts/par-20M-delay0-eps02-freeze.txt`
- `artifacts/par-20M-delay0-eps02.log`
