# 50M-iteration convergence result (2026-09-28)

## The number

Tiny abstraction, tiny-bundle robust policy, γ=1.0:

| iters | seat 0 | seat 1 | mean | wall |
|---|---:|---:|---:|---:|
| 500 000 | 23 280 | 13 957 | 18 619 | 10 min |
| 5 000 000 | 36 665 | 18 205 | 27 435 | 100 min |
| 50 000 000 | **14 147** | 17 948 | **16 048** | 7.7 h |

Reading:
- 500k → 5M was measured with γ = 0.9 (pre-fix); disregard.
- 500k vs 50M (both γ = 1.0): seat 0 drops 39 %, seat 1 RISES 29 %. Net
  aggregate improvement 14 %. 100× more compute buys 14 %.

## What this settles

**The tiny abstraction is at its ceiling.** 50M iterations roughly equals
the 500k baseline on aggregate. More iterations on this abstraction will
not meaningfully reduce exploitability.

**Seat asymmetry is real.** The 500k→50M improvement is entirely on seat
0 (SB). Seat 1 (BB) got worse. This deserves a follow-up: either the
trainer's seat-randomization schedule degrades BB over very long runs, or
the 200-deal sample has too much noise on the seat-1 statistic. But either
way, doubling iterations is not the route to a materially stronger bot on
tiny.

## What this implies for the roadmap

1. **Do not run tiny for more iterations.** 500k is enough on this
   abstraction. Real gains must come from a richer abstraction (the full
   bundle) or from the trainer's efficiency.
2. **The full abstraction is now the only remaining single-dimensional
   improvement.** Bundle assembled at `artifacts/agent-full-honest`
   (ladder in flight as of this writing).
3. **Exploitability of ~14-18 bb/hand** on the seat that matters is still
   far from equilibrium (a well-trained HUNL bot is measured in tens of
   **milli**bb). The gap is not an iteration problem on tiny.
