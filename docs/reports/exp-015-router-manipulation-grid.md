# EXP-015 router-manipulation grid — report

60 cells: `switch_at × N0 × temp` = 5 × 4 × 3, 2000 deals each, live
`full` victim (tiny bundle `artifacts/agent`) vs `switch:arch:nit->arch:lag`
manipulator. Static snapshot: `artifacts/blueprints-tiny/robust-7`
(buckets-tiny + abstraction-tiny). Raw cells: `artifacts/exp-015-grid/`.

Metric: manipulator earn in mb/seating (positive = manipulator exploits the
victim). Mean ± 1.96·SE per cell.

## Heatmap (manipulator earn ± CI)

| switch | n0=4, t=0.5 | n0=4, t=0.7 | n0=4, t=1.0 | n0=8, t=0.5 | n0=8, t=0.7 | n0=8, t=1.0 | n0=16, t=0.5 | n0=16, t=0.7 | n0=16, t=1.0 | n0=32, t=0.5 | n0=32, t=0.7 | n0=32, t=1.0 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 10 | +1467±1036 | +1467±1036 | +1467±1036 | +1470±1036 | +1470±1036 | +1470±1036 | +1444±1040 | +1447±1040 | +1447±1040 | +1329±1035 | +1329±1035 | +1329±1035 |
| 20 | +1476±1036 | +1476±1036 | +1476±1036 | +1452±1040 | +1452±1040 | +1452±1040 | +1382±1038 | +1380±1038 | +1380±1038 | +1312±1031 | +1284±1030 | +1284±1030 |
| 40 | +1390±1036 | +1390±1036 | +1390±1036 | +1393±1036 | +1393±1036 | +1393±1036 | +1415±1040 | +1415±1040 | +1415±1040 | +1337±1036 | +1337±1036 | +1337±1036 |
| 80 | +1335±1031 | +1335±1031 | +1335±1031 | +1334±1034 | +1334±1034 | +1334±1034 | +1380±1034 | +1380±1034 | +1380±1034 | +1252±1031 | +1252±1031 | +1252±1031 |
| 150 | +1171±1023 | +1171±1023 | +1171±1023 | +1203±1024 | +1203±1024 | +1203±1024 | +1227±1027 | +1227±1027 | +1227±1027 | +1236±1024 | +1236±1024 | +1236±1024 |

All 60 cells produced a result; no failures.

## Verdict (per `experiments/PREREG-EXP-015.toml`: report-only, promote only with ladder follow-up)

- **Default confirmed near-optimal; no ladder follow-up triggered.** Default
  cell (switch=40, N0=8, temp=0.7): +1393. Best-victim cell (150, 4, 0.5):
  +1171 — Δ = −222, far inside the ±~1030 CI. No cell beats default
  significantly; no hyperparameter is promoted.
- **The manipulator exploit is robust to router hyperparameters.** Every cell
  is positive (+1171 … +1476): the nit→lag switch exploits live full by
  >1100 mb/seating regardless of switch point, prior strength, or
  temperature. The router cannot tune its way out — this needs the
  structural Phase 7 (ensemble-disagreement shield) fix, not hyperparams.
- **Temperature is a non-knob** on these trajectories (identical streams
  across t=0.5/0.7/1.0 within each switch×N0 — the mixture sampling rarely
  flips actions). N0=32 and late switch (150) shave ~150–200 off the
  manipulator earn, consistently but insignificantly. If a cheap mitigation
  is ever wanted, N0=32 is directionally best; it is NOT promoted (inside
  noise, and N0 interacts with fallback rates — see EXP-014).
