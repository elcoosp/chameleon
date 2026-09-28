# Router temperature sweep — hedge is not a bug (2026-09-28)

## The hypothesis I tested

The 2026-09-28 routing comparison showed argmax beating mixture by ~2
bb/seating on 6/9 opponents. Argmax is the T→0 limit of the mixture's
sharpening (`prior ∝ p^(1/T)`). If mixture loses because its softmax is
too smooth, then lowering T should recover the gap.

## The measurement (trained synthetic router, tiny bundle)

Ladder mb/seating on the same pool. Only the temperature changed.

| opponent | T=0.7 | T=0.35 | T=0.2 | T=0.1 | T=0.05 |
|---|---:|---:|---:|---:|---:|
| arch:nit | +1 572 | +1 591 | **−3 104** | −3 169 | −3 224 |
| arch:tag | +3 277 | +3 300 | +1 804 | **−3 058** | −3 195 |
| arch:lag | +5 581 | +5 611 | +5 486 | +5 557 | +5 325 |
| arch:station | +12 776 | +12 816 | +12 888 | +12 606 | +12 595 |
| callbot | +11 170 | +11 155 | +11 140 | +11 129 | +11 238 |
| jamfix | −387 | −387 | −387 | −387 | −387 |
| pnash | +222 | +242 | +242 | +226 | +237 |
| famB:tag | +635 | +633 | +600 | +428 | **+94** |
| noisy | +4 708 | +4 809 | +4 720 | +4 831 | +4 903 |

## What this says

**The hypothesis is wrong.** Sharpening toward argmax does not recover
the mixture-vs-argmax gap — it *destroys the mixture*:

- T ≥ 0.35 ≈ the historical default. No measurable difference.
- T ≤ 0.2 starts catastrophically losing on arch:nit and arch:tag.
  The sharper router picks ONE expert; when the router is uncertain
  (or wrong), the mixture loses the hedge that was protecting it.

The mixture is a hedge against router error, not a bug that argmax
avoids. Argmax wins on aggregate because the synthetic router is
*actually accurate enough* on the opponents it wins big against
(station, callbot), but a **real** router is likely to be less accurate,
making the hedge even more valuable — not less.

## Consequences

1. Temperature is not a competitive lever. Leave at 0.7.
2. The mixture/argmax gap is structural: they make different bets about
   router accuracy.
3. The right comparison is with a real-data router. See
   ROUTER-TRAINING-GAP-2026-09-28.md.
