# Full-abstraction 9M result (2026-09-29)

## The numbers, side by side

LBR at depth 100, 200 deals. Lower is better.

| bundle | iters | infosets/expert | visits/infoset | seat 0 | seat 1 |
|---|---:|---:|---:|---:|---:|
| tiny, serial 500k | 500k | ~21 000 | 24 | 23 280 | 13 957 |
| tiny, parallel 5M | 5M | ~21 000 | 238 | **15 040** | **12 050** |
| **full, parallel 9M** | **9M** | **~380 000** | **24** | **18 079** | **14 298** |

## At MATCHED visits/infoset (both 24)

  tiny 500k: 23 280 / 13 957
  full 9M:   18 079 / 14 298

**The full abstraction beats tiny on seat 0 by 22 %** at the same
training-quality-per-infoset. It is essentially tied on seat 1
(14 298 vs 13 957, full 2 % worse).

So the abstraction is NOT the ceiling. With equal visits per infoset,
more river buckets gives a materially better SB policy.

## But visits matter more than buckets

Tiny at 238 visits/infoset (5M iters) achieves 15 040 / 12 050 — **better
than full at 24 visits/infoset** on both seats.

To match tiny's 5M-quality on full, we would need approximately:
  238 visits/infoset × 380 000 infosets = **90M iterations per expert**

At the observed 9M wall time (3.6 hours parallel, 4 workers), 90M is
**36 hours per expert**, ×5 experts = **7.5 days** for a full bundle.

## What the roadmap should be

The pragmatic answer is not "train full longer" — it is **make the tiny
policy better per visit**. The two ends of the frontier:

1. **Tiny + more iters.** 5M gives 15 040 / 12 050. 50M might give
   10 000 / 9 000 (we do not know; the earlier 50M run used γ=0.9
   and is invalid).

2. **Full + far more iters.** 90M per expert needs 7.5 days wall.
   Reachable with patience; the current parallel trainer makes it
   physically possible, which it was not before today.

## The seat asymmetry persists

Every configuration shows **seat 1 (BB) has LBR ~2-3 bb/hand lower than
seat 0 (SB)**. SB has position postflop and gets exploited harder by a
best responder. If we ship one policy for both seats, that policy is
better at BB than at SB. A seat-conditional policy is a real potential
win — not investigated.

## Recommendation, ordered

1. **Do not switch the SOTA bundle to full.** Tiny parallel 5M is the
   current best: 15 040 / 12 050, 47 min wall.
2. **Run tiny parallel 20M or 50M next.** ~3-7 hours each, gives the
   tiny-side convergence curve with correct γ.
3. **Run full parallel 90M in the background** (7.5 days). It is the
   only way to see whether the abstraction's advantage compounds with
   visits. It runs unattended; nothing else blocks on it.
4. **Investigate seat-conditional policies.** The persistent seat-1
   advantage suggests a shared policy is leaving BB value on the table.
