# F6a silently 4x'd the infoset count (2026-10-03)

**Finding:** the F6a raise-cap fix expanded tiny's infoset count from
**~21,400 to ~80,700** at the same config and seed. Every
"visits/infoset" comparison taken before F6a is ~4x optimistic.

## The numbers (tiny robust, seed 7, 5M iters)

| artifact | trainer | infosets | visits/infoset |
|---|---|---:|---:|
| `par-5M` | pre-F6a | 21,386 | 234 |
| `par-f5-tiny-5000000` | post-F6a | 80,772 | 62 |

Same `config/abstraction-tiny.toml`, same `buckets-tiny`, same seed.
The only change is the trainer.

## Why

Pre-F6a, a bet consumed the street's raise budget
(`raises < raises_per_street_cap` counted bets as raises), cutting off
long aggressive histories. Post-F6a only re-raises count, so
bet -> raise -> re-raise -> ... histories are reachable and the action
space grows combinatorially.

## Consequences

1. **All prior visits/infoset numbers are stale.** Anything tuned to a
   visits/infoset target must be re-tuned to the post-F6a tree size.
2. **Every abstraction is ~4x bigger post-F6a** (tiny, medium, rich,
   rich-lite). The rich infoset explosion is partly this.
3. **"Tiny peaks at 5M" partly a visits-budget effect**: 80k infosets at
   5M = 62 visits/infoset.
4. **Matched-visits comparisons must use current-tree counts.**

## Action

- Quote the trainer era with any visits/infoset number.
- The rich / rich-lite analyses this session used post-F6a counts
  (correct).
- Pre-2026-10-01 docs justifying a config by visits/infoset are suspect.

Source: `artifacts/par-5M` (21,386) vs
`artifacts/par-f5-tiny-5000000` (80,772) provenance.
