# jamfix regression — decomposed (2026-10-03)

The retrained bundle regresses against `jamfix` by **-1193 mb/seating**
full-tier (shipped +4738.4 vs retrained +3545.2). This decomposes it.

## Decomposition (full tier, 5000 seatings)

| mode | shipped | retrained | delta |
|---|---:|---:|---:|
| `--agent robust-only` | 3918.5 | 3432.2 | **-486.3** |
| `--agent full` (argmax) | 4738.4 | 3545.2 | **-1193.2** |

- **Robust arm**: -486. The DCFR(1.5,0,γ2) robust is slightly worse than
  the shipped robust against a shove-only bot.
- **Experts**: shipped experts add +819.9 over shipped robust;
  retrained experts add only +113.0 over retrained robust. The delta
  between those two expert contributions is **-707** — the majority of
  the regression.

Router is identical in both bundles (the retrain reused
`agent-honest-19dim/router.bin`), so the routing decision is the same
given the same expert strategies. The difference is purely the expert
**policies**.

## Interpretation

jamfix is a **pure all-in bot** — out of family. The experts are trained
`exploit` vs the in-family archetypes (nit/tag/lag/station); none is
trained against a shove-bot. The shipped experts happened to handle it
better than the retrained ones. This is not a bug: both are valid
policies, and the retrained experts are better on the 8 opponents that
are the training targets (all four archetypes improve: nit +2429,
tag +4407, lag +6953, station +3752).

## Verdict

The regression is **narrow and explained**: one out-of-family opponent
(a shove-bot), ~40% from the DCFR-γ2 robust arm and ~60% from the
retrained experts, neither trained for that opponent class. It does not
contradict the promotion — the retrained bundle wins 8/9 overall,
+2989 mb/seating mean.

If jamfix-class opponents matter for the target use, the fix is to add
a shove-bot to the experts' training mix (a new experiment), not to
reject the retrain.

Source: `artifacts/jamfix-isolate-2026-10-03/summary.txt`.
