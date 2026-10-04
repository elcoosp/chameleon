# Honest cross-config comparison (2026-10-04)

Full-coverage buckets, 15,000 train deals (converged budget), slot bucket,
DCFR γ2 unless noted, bound-gated (sum >= 0).

| arm | tree | schedule | seat0 | seat1 | sum |
|---|---|---|---:|---:|---:|
| tiny-full | 1 size | DCFR g2 | +4.13 | +4.06 | **8.19** |
| rich-lite-full | 2 sizes | DCFR g2 | +5.40 | +4.85 | **10.27** |
| rich-lite-full | 2 sizes | CFR+ | (running) | (running) | ? |

## Findings

1. **More sizes is MORE exploitable, not less.** rich-lite (2 sizes,
   191k infosets) sums +10.27 vs tiny (1 size, 83k infosets) +8.19.
   At fixed 5M iters, the 2.3x-larger tree is less converged. This
   agrees with the ladder (rich-lite lost 1565 mb there).

2. **Even tiny is ~8 bb exploitable.** The earlier "tiny ~= -1.29
   unexploitable" was under-convergence (5k deals) on sampled buckets.
   Correct buckets + converged budget => 8.19.

3. **Seat symmetry.** tiny's two seats agree (4.13 / 4.06), a clean
   read; rich-lite's diverge more (5.40 / 4.85), consistent with its
   higher per-seat noise from under-convergence.

## Caveat (important)

Within-abstraction BR is not a clean cross-abstraction comparison: a
richer abstraction admits more exploitation by construction (finer
actions/infosets). So "10.27 > 8.19" does not by itself prove
rich-lite is worse in the real game. It DOES agree with the real-game
ladder (rich-lite lost there), so both point the same way: the 2-size
tree is not helping at this iteration budget.

## Pending

- rich-lite-full CFR+ (the DCFR effect).
- A clean cross-abstraction measure would be the fine-information BR
  with enough deals (the fine-BR arm was starved at the coarse budget).
