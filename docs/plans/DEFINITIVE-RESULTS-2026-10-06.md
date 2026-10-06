# Definitive results — first bound-valid table (2026-10-06)

Full-coverage buckets, slot bucket, 15k train deals (converged), direct test
binary (immune to the cross-repo `pkill nextest`). ALL sums >= 0 => valid.

## Exploitability (both seats, BR value, lower = better)

| policy | tree | schedule | iters | sum bb |
|---|---|---|---:|---:|
| tiny-full | 1 size | DCFR g2 | 5M | 8.19 |
| rlf-g2 | 2 sizes | DCFR g2 | 5M | 10.27 |
| rlf-cfr | 2 sizes | CFR+ | 5M | 10.11 |
| rlf-12m | 2 sizes | DCFR g2 | 12M | 10.56 |

Per-seat SEs ~0.45-0.72 bb; sum SE ~0.9 bb.

## Search (same policy + buckets, only search toggled)

| arm | sum bb |
|---|---:|
| OFF | 1.63 |
| ON  | 7.20 |

Delta +5.57 bb (search WORSE), both sums valid.

## Conclusions

1. **DCFR is neutral** (10.27 vs 10.11; inside noise). The earlier
   "DCFR g2 wins" was under-convergence on the wrong metric.
2. **More bet sizes make the policy MORE exploitable** (8.19 -> 10.27),
   and 12M iters does not help (10.56). Do not add sizes.
3. **Search, as implemented, is HARMFUL** (+5.57 bb). This is the
   unsafe-re-solving prediction: no Burch/Brown-Sandholm gadget, so the
   re-solve is an unbounded deviation toward a mismodeled subgame
   equilibrium. Search needs the gadget or stays off.
4. **The base tiny policy is ~8 bb exploitable** (within its own
   abstraction) — not the "~0" claimed before the ruler was fixed.

## Caveats

- **Within-abstraction BR, not cross-abstraction.** A sharper bucket
  set gives the BR more freedom and measures HIGHER exploitability even
  for the same policy. The four-arm table is internally consistent
  (same buckets); the search table is internally consistent (same
  policy+buckets). Their absolute numbers are NOT comparable across
  tables.
- Single seed.
- The search ON arm is 3x slower per decision; both arms were given the
  same deal budget, so ON is if anything under-measured (its true
  exploitability could be even higher).

## What this leaves

No abstraction lever helps (sizes worse, DCFR neutral). Search is
harmful without a gadget. The remaining first-order work:
1. **Safety gadget** for search (the only way search can help).
2. **Real-game gap**: external/adaptive opponents (never measured).
