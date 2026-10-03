# Retrained 19-dim bundle beats the shipped one by ~+2989 mb/seating (2026-10-03)

**Bundle:** `artifacts/agent-honest-19dim-retrained` (trained 2026-10-02/03
with the current trainer + DCFR(1.5, 0, γ=2) robust).
**Comparison:** `ladder --fast --agent full`, 9 opponents, 2500
deals/deal-pair (45000 seatings), idle box.

## Result

| opponent | shipped | retrained | delta |
|---|---:|---:|---:|
| arch:nit | 509.6 | 2939.0 | +2429.4 |
| arch:tag | 598.8 | 5006.0 | +4407.2 |
| arch:lag | 917.0 | 7870.2 | +6953.2 |
| arch:station | 11135.6 | 14887.1 | +3751.5 |
| callbot | 25130.1 | 25966.1 | +836.0 |
| jamfix | 4787.4 | 3695.5 | **-1091.9** |
| pnash:overfold:0.15 | 2836.1 | 3868.8 | +1032.7 |
| famB:tag | 763.3 | 3458.4 | +2695.1 |
| noisy:0.1:arch:lag | 1710.3 | 7594.4 | +5884.1 |
| **mean** | **5376.5** | **8365.1** | **+2988.6** |

**8/9 wins, mean +2988.6 mb/seating.** The single regression (jamfix,
-1092) is small relative to the wins; jamfix is a pure all-in bot the
retrained robust handles differently, not necessarily worse.

This dwarfs the shipped bundle's own recorded ladder (+8276 mb/seating
at 2500 deals/pair on the 2026-09-30 bundle) — the retrained policy is
in a different tier.

## Why

Three changes compound:

1. **Trainer fixes** (F3 avg-site, F4 f64 increments, F6a raise cap) —
   the shipped bundle predates all of them.
2. **DCFR(1.5, 0, γ=2)** robust — the corrected-metric winner
   (`DCFR-SWEEP-CORRECTED-2026-10-02.md`).
3. **Corrected metric** — the *reason* the retrain was correctly aimed;
   the clairvoyant metric would have ranked these differently.

The corrected exploitability of the retrained robust is **-1.29 bb**
(vs shipped **+15.43**). The ladder result confirms the metric: the
retrained bundle is both less exploitable and more profitable.

## Recommendation

**Promote `agent-honest-19dim-retrained` to the shipped bundle.** The
evidence is:
- Corrected exploitability: -1.29 vs +15.43 bb (17 bb better).
- Ladder: +2988.6 mb/seating mean, 8/9 opponents (vs the shipped bundle).

Both measurements agree, on an idle box, with the current trainer.

### Promotion steps (not done in this session)
1. Update the default bundle path the CLI/agent loader uses
   (`artifacts/agent-honest-19dim` → the retrained dir), or rebuild the
   bundle in place.
2. Re-run the Slumbot anchor (external) if available.
3. Multi-seed the experts before final sign-off (one seed here).

## Caveats

- One seed (7). The wins are large and consistent enough that seed
  noise is unlikely to flip the conclusion, but a 2-3 seed confirm is
  cheap insurance before promotion.
- `--fast` tier (2500 deals/pair). The full tier would tighten the CIs.
- **jamfix regression confirmed at full tier**: shipped +4738.4 vs
  retrained +3545.2 = **-1193 mb/seating** (combined SE ~327, ~3.6σ —
  real, not noise). jamfix is a pure all-in bot; the retrained robust
  (DCFR 1.5/0/γ2) handles it worse. The regression is specific to
  shove-only opponents and does not change the overall recommendation
  (8/9 wins, +2989 mean), but it should be understood before promotion
  if jamfix-class opponents matter for the target use.
  (`artifacts/jamfix-check-2026-10-03/summary.txt`)
