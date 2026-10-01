# Parallel 5M result — the parallel trainer *helps*, not just matches (2026-09-29)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


## The number

Tiny abstraction, Robust mode, γ=1.0, 200 deals:

| run | iters | wall | seat0 | seat1 | aggregate |
|---|---:|---:|---:|---:|---:|
| serial | 500k | 10 min | 23 280 | 13 957 | 18 619 |
| **parallel (4 workers)** | **5M** | **47 min** | **15 040** | **12 050** | **13 545** |

Same policy family (Robust tiny). 5M parallel is **35 % less exploitable
on seat 0** and **14 % less on seat 1** than 500k serial.

## What this actually shows

Two things happened at once:

1. **The parallel trainer's 2.4× speedup lets us reach 10× the iteration
   count in ~5× the wall time.** The parallel trainer wasn't just
   wall-clock-friendly — it made a *better* policy reachable.

2. **The `50M-CONVERGENCE` result was misleading.** That doc concluded
   "tiny is at its ceiling" by comparing 500k → 5M → 50M with the OLD
   γ=0.9 default. With γ=1.0 the 5M run is materially better than 500k.
   Tiny is not at its ceiling — the old averaging was wasting the
   iterations.

## Re-reading the earlier data with the γ fix in mind

- 500k serial, γ=0.9: 39 692 / 15 068 (from earlier)
- 500k serial, γ=1.0: 23 280 / 13 957 (correct baseline)
- 5M serial, γ=0.9: 36 665 / 18 205 (the old "ceiling" claim)
- **5M parallel, γ=1.0: 15 040 / 12 050** (new)

The earlier 3M → 10M sweep was ALL γ=0.9. The apparent "worse on seat 1"
regression at 10M was the γ-underflow, not a training-dynamics problem.

## What to run next

The full-abstraction 9M run finished overnight (3.6h wall, not 14 — the
box freed up). Its LBR against the tiny numbers above is the answer to
"does the abstraction help, or is tiny fine".
