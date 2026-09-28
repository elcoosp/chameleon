# Full vs tiny abstraction: sample starvation (2026-09-28)

## The measurement

Both bundles 500k iters/expert, γ = 1.0, argmax routing, same 9-opponent pool.

| opponent | tiny (21k infosets) | full (380k infosets) |
|---|---:|---:|
| arch:nit | +1 383 | **+2 515** |
| arch:tag | +3 618 | +2 690 |
| arch:lag | +4 072 | +3 195 |
| arch:station | +13 989 | +3 901 |
| callbot | +24 962 | +6 972 |
| jamfix | +4 787 | **−1 052** (SPRT AcceptH0 at 500 seatings) |
| pnash | +4 168 | **−13** |
| famB:tag | +2 269 | +1 484 |
| noisy | +5 075 | +2 580 |
| **mean** | **+7 146** | **+2 474** |

**Full is 3× worse than tiny on this budget.** But the reason is not the
abstraction — it is the ratio of iterations to infosets:

| bundle | infosets/expert | 500k iters → visits/infoset |
|---|---:|---:|
| tiny | 21 000 | 24 |
| full | 380 000 | **1.3** |

1.3 visits per infoset is not training. CFR+'s regret-matching needs an
order of magnitude more data per node before it starts to converge. The
full bundle is **sample-starved**, not beaten by a finer abstraction.

## What this implies

To reach parity with tiny's 24 visits/infoset, full needs
`380 000 × 24 = 9.1 M iterations` per expert. At tiny's measured rate
(500k in ~10 min single-threaded, 380k infosets so ~20 min/expert at
full-abstraction rates), 9.1M = ~6 h per expert, ×5 experts = ~30 h.

**That is the cost of the next real experiment**: train the full
abstraction at 9M+ iterations per expert to see whether the finer
abstraction actually beats tiny at equal visits/infoset. This is the
proper test of "does more river resolution help".

## Also: tiny is at its own ceiling

From `50M-CONVERGENCE-2026-09-28.md`: 50M iterations on tiny
(2 400 visits/infoset) buys only ~14 % aggregate improvement over 500k
(24 visits/infoset), with seat-1 *regressing*. Tiny's exploitability
floor on this abstraction is ~15 bb/hand regardless of budget.

So there are two independent findings:

1. **Full needs 20× more iters** to be a fair comparison. It is not
   "worse" — it is untrained.
2. **Tiny cannot go below ~15 bb/hand** no matter how many iters. Its
   ceiling is real and low.

The remaining question — does the full abstraction go below 15 bb/hand
when properly trained — requires the 30-hour experiment above. The
alternative is to attack the trainer itself: a truly parallel
implementation would make the 30-hour experiment a 3-hour one, and would
unlock much larger iteration counts across the board.

## The parallel-trainer gap

The trainer is single-threaded (M-6 in the bug report). Every measurement
in this document is running on one core of an 8-core machine. A proper
worker pool (Hogwild or Snapbatch — the storage-layer code for both
already exists) would turn 30 hours into ~4 hours. **That is the highest-
leverage engineering work remaining.**
