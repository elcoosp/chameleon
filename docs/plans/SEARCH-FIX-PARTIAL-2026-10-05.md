# §3.1 search fix: partial (2026-10-05)

The other agent's `label_to_action` fix (maps solver bet labels to real
sizes instead of collapsing to the min-bet) is compiled into the
release binary. Re-measured search OFF vs ON:

| opponent | OFF | ON | delta | pre-fix delta |
|---|---:|---:|---:|---:|
| callbot | +24789.6 | +12444.7 | **-12345** | -12471 |
| arch:station | +13761.4 | +9796.0 | **-3965** | -8354 |

## What changed

- **station: loss halved** (-8354 -> -3965). The min-bet mapping was a
  real contributor against a calling station.
- **callbot: unchanged** (-12471 -> -12345, within SE).

## What this means

The §3.1 diagnosis was **partly right**: fixing bet sizing helped one
calling-heavy opponent (station) a lot, and did nothing for the other
(callbot). Search is **still net-negative**.

So the bet-size mapping was A cause, not THE cause. The remaining loss
must be the other shared ingredient the audit itself flagged: the
**villain range** (`search_bridge` builds it from tracker marginals /
a 3-class heuristic). If the range makes the solver over-check, the
chosen action is "check" — which the bet-size fix does not touch. That
would explain why callbot (the extreme caller) is unchanged.

## Next

Implement the **blueprint-reach villain range** (F1 plan, approach (a)):
walk the public action sequence, reweight villain classes by the robust
policy's own action probabilities from villain's seat. That is the
principled range; the heuristic (c) is demonstrably insufficient.

Keep the §3.1 fix (it helped). Add the range fix, then re-run this A/B.
