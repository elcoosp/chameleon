# DCFR alpha=0.5 is worse than alpha=0.9 — lever is dead at any discount ≤ 0.9 (2026-09-30)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


## The prediction

`DCFR-ALPHA05-PREDICTION-2026-09-29.md` predicted alpha=0.5 would land
in [25 000, 50 000] on either seat at 1000 deals. It did.

## The result

| variant | SB | BB | mean |
|---|---:|---:|---:|
| tiny 20M no-fix      | 13 319 | 14 706 | 14 012 |
| tiny 20M alpha=0.9   | 35 344 | 25 834 | 30 589 |
| **tiny 20M alpha=0.5** | **38 721** | **29 102** | **33 912** |

1000-deal LBR at depth 100. Lower is better.

Alpha=0.5 is worse than alpha=0.9 by **3 377 on SB and 3 268 on BB**.
The two are both catastrophic; alpha=0.5 is monotonically worse.

## The freeze diagnostic for alpha=0.5

        rows (w>=2):      21427
        frozen (one-hot): 10239 (47.8%)
        near-frozen (.9+):1185 (5.5%)
        soft (<.5):       2132 (10.0%)
        --- AVERAGE strategy ---
        avg_near_frozen:  2078 (9.7%)
        mean cur max_p:   0.818
        mean avg max_p:   0.579

Compare across discount rates:

| metric | no-fix | alpha=0.9 | alpha=0.5 |
|---|---:|---:|---:|
| soft (<0.5)         | 3.3% | 16.6% | 10.0% |
| avg_near_frozen     | 60.0% | 11.9% | 9.7% |
| mean cur max_p      | 0.871 | 0.734 | 0.818 |
| mean avg max_p      | 0.859 | 0.616 | 0.579 |
| LBR mean            | **14 012** | 30 589 | 33 912 |

The freeze diagnostic is **non-monotonic** in the discount: alpha=0.5
un-freezes the average (avg max_p 0.579, lower than alpha=0.9's 0.616),
but the LBR is worse. So the "most un-frozen" table is the worst policy.

## Interpretation

DCFR's positive-regret discount at alpha<1 has a memory half-life of
`ln(0.5) / ln(alpha)` iterations. At 20M iterations:

- alpha=0.9: ~7 iterations of memory
- alpha=0.5: ~1 iteration of memory

Both are far shorter than the timescale over which the true regrets
change (~100s of thousands of iterations). The "accumulated regret" is
noise, and RM+ on noise produces approximately uniform play. Because
the true equilibrium is *near-pure* on most rows, uniform play is
*very* exploitable.

**DCFR is not a usable lever for the tiny-abstraction RM+ freeze at
any discount ≤ 0.9.** The only untested discount range is 0.99+, where
the half-life is ~70 iterations (still short, but 10x longer than
alpha=0.9). Not worth testing without a different reason — the shape
of the failure (un-freeze to uniform) is now well understood.

## The lesson for the freeze story

The freeze diagnostic is **not a proxy for policy quality**. It is a
proxy for "the current iterate is concentrated". A well-converged RM+
on a near-pure equilibrium is exactly what the freeze diagnostic
reports as bad. The BB regression is real but it comes from a small
subset of rows; the diagnostic aggregates over all rows and calls the
whole picture "frozen".

**The right levers are the ones that keep the signal and add mixing
locally** (schedule weight, exploration floor), not the ones that
discard the signal globally (DCFR regret discount).

## Artifacts

- `artifacts/par-20M-alpha05/robust-7/policy/policy.bin`
- `artifacts/par-20M-alpha05/robust-7/table.snap`
- `artifacts/par-20M-alpha05-lbr.log`
- `artifacts/par-20M-alpha05-freeze.txt`
- `artifacts/par-20M-alpha05.log`
