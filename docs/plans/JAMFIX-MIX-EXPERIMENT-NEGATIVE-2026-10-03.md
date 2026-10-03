# jamfix expert-mixture experiment — NEGATIVE (2026-10-03)

**Hypothesis:** training expert 0 (nit) against
`mix:0.8:arch:nit~jamfix` would fix the jamfix regression (−1193)
without hurting the archetype win.

**Result:** it does neither. The jamfix number is unchanged; nit
degrades.

## Data (full 9-opponent ladder, 2500 deals/pair)

| metric | promoted | nmix | delta |
|---|---:|---:|---:|
| jamfix | +3695.5 | +3695.5 | **0.0** |
| arch:nit | +2927.7 | +2816.0 | **-111.7** |
| mean (9 opps) | +8365.1 | +8278.5 | **-86.6** |

Router picks: promoted `e0=10845 e1=39902 e2=28907 e3=41622`;
nmix `e0=11458 e1=39878 e2=28903 e3=41598`. So the bundle DID change
(613 more e0 picks) — yet jamfix is byte-identical.

## Why (the real finding)

**jamfix is not an expert-policy problem; it is a robust-arm / coverage
problem.** Two pieces of evidence:

1. The jamfix ladder number is IDENTICAL to 0.1 mb after swapping
   expert 0's policy. If jamfix decisions used expert 0, its policy
   change would move the number.
2. Earlier routing data (`ROUTER-EXPERT-ROUTING-2026-10-03.md`): for
   jamfix, **40/80 decisions had all four experts `expert_miss`** — the
   action came from the robust fallback, not from any expert.

So jamfix's off-tree shove lines produce infoset keys no expert has
rows for; robust covers, and robust's DCFR-γ2 policy is the −486 part
of the regression. The experts contribute the rest through their own
coverage gaps.

## Conclusion

- The expert-mixture fix is a **negative result**. Do not ship it.
- The jamfix regression is **accepted** as a narrow, out-of-family
  effect (one shove-bot opponent, −1193) against an 8/9-opponent,
  +2989-mean gain. It does not justify further retrains on its own.
- If jamfix-class opponents ever matter, the fix targets the **robust
  arm** (train robust against a mixture including a shove-bot) or
  **coverage** (the off-tree lines), NOT the experts. That is a
  separate, larger experiment.

## Machinery kept

The `mix:` spec + `MixerAgent` + analytic `JamBot::action_probs`
remain (committed, tested) — they are correct and reusable for a future
robust-arm experiment.

Source: `artifacts/exp-jamfix-mix-2026-10-03/summary.txt`,
`artifacts/nmix-full.log`.
