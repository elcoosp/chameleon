# Search's adaptive benefit is timing-dependent (2026-10-07)

## Result

`self-exploit` adaptive manipulator vs live `full`, 20k deals, controlled
env, LATE switch points (manipulator plays nit for most of the session):

| switch_at | OFF | ON (gadget) | delta | sigma |
|---|---:|---:|---:|---:|
| 2000 | -3407.3 +/- 207.8 | -4532.6 +/- 248.8 | **-1125.3** | 3.5 |
| 10000 | -3346.0 +/- 202.2 | -4171.0 +/- 235.9 | **-825.0** | 2.7 |
| 18000 | -3213.8 +/- 196.2 | -3687.8 +/- 223.3 | **-474.0** | 1.6 |

(SEs are unpaired-conservative: sqrt of the two reported SEs squared.)

## The finding: search's benefit shrinks as the deviation gets later

Monotonic: -1125 -> -825 -> -474. At switch=18000 (manipulator stays nit
for 90% of the session) the benefit is only **1.6 sigma — not significant**.

Mechanism: search supplies real-time ROBUSTNESS when the opponent's
behaviour changes and the agent's learned tracker goes stale. When the
opponent is stable (nit for most of the run), the tracker is well-calibrated
and search has little to correct. So search's value is largest against
opponents that ADAPT/CHANGE mid-session, smallest against stable ones.

## Combined picture (all switch points)

| switch_at range | search benefit |
|---|---:|
| early (20/40/80) | -1189 (3.6 sigma) |
| 2000 | -1125 (3.5 sigma) |
| 10000 | -825 (2.7 sigma) |
| 18000 | -474 (1.6 sigma, NS) |

Search helps in every case except the very-latest deviation, and the
benefit degrades smoothly with how stable the opponent is.

## Interpretation

Search is a **robustness tool**, not a general strength upgrade:
- vs a STABLE scripted bot: costs -2104 on the ladder (safety vs exploitation).
- vs an opponent that CHANGES: gains up to -1189 (harder to exploit).
- vs an opponent that changes LATE: gains little (-474, NS).

Enable search where the opponent may adapt; the benefit scales with how
much the opponent's behaviour shifts during the session.

## Caveats

- 3 switch points, single manipulator family, single snapshot.
- Unpaired SEs (conservative); a paired test would tighten the 18000 point.
- The OFF baseline itself drifts (-3407 -> -3214) as the switch moves; the
  ON arm drifts more (-4533 -> -3688), which is where the shrinking delta
  comes from.
