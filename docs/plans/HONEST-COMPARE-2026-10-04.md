# Honest cross-config comparison, complete (2026-10-04/05)

Full-coverage buckets, slot bucket, 5M iters, 15k train deals (converged
budget), bound-gated (sum >= 0).

| arm | tree | schedule | seat0 | seat1 | sum |
|---|---|---|---:|---:|---:|
| tiny-full | 1 size | DCFR g2 | +4.13 | +4.06 | **8.19** |
| rich-lite-full | 2 sizes | DCFR g2 | +5.40 | +4.85 | **10.27** |
| rich-lite-full | 2 sizes | CFR+ | +5.13 | +4.98 | **10.11** |

## Findings

1. **DCFR is neutral.** g2 (10.27) vs CFR+ (10.11): delta 0.16 bb,
   far inside noise (~1 bb). The earlier "DCFR g2 wins" (-0.70 vs
   -1.45) was an UNDER-CONVERGED artifact (5k deals, sampled buckets).
   Once converged, the schedule does not matter.

2. **More sizes do not reduce exploitability** at matched iters:
   tiny 8.19 vs rich-lite 10.27/10.11. Direction agrees with the
   ladder (rich-lite lost 1565 mb there).

3. **But #2 is NOT conclusive.** ~2 bb on a ~1.4 bb sum SE is ~1.4
   sigma. And there is a confound: all arms used 5M iters while
   rich-lite has 2.3x the infosets (191k vs 83k), so it is less
   converged at equal iters. The fair test is MATCHED VISITS/INFOSET.

4. **Everything is ~8-10 bb exploitable.** Not the "~0" claimed all
   session. Correct buckets + converged budget + no fallback = a policy
   that is meaningfully beatable within its own abstraction.

## Corrections this doc forces

- **DCFR schedule choice was arbitrary.** The retrain fixes (F3/F4/F6a)
  were real and validated; the DCFR-over-CFR+ preference was noise.
- **"More sizes = the fix" is unsupported.** At best neutral, at worst
  negative, at this budget.

## Next (disentangle the tree effect)

Matched visits/infoset: train rich-lite to ~12M iters (191k infosets x
60 visits = 11.5M) and re-measure with the same 15k-deal budget. If
rich-lite@12M < tiny@5M, the tree helps with enough iters; if not, the
2-size tree does not pay off at any feasible budget on this hardware.

Also: the both_seats test does not PRINT se_bb (the fine-BR test does);
add it so deltas can be quoted against their SE.
