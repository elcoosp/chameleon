# The fair search test is still confounded (2026-10-05)

## What was fixed

`action_distribution` did NOT run the search bridge (only `act_impl` did),
so the first test compared OFF to OFF (delta exactly 0). Added
`ChameleonAgent::deployed_distribution` which mirrors `act_impl`'s search
branch. The test now shows a real delta.

## Result (2000 train deals / 300 test / 20 sweeps)

| arm | seat 0 | seat 1 | sum |
|---|---:|---:|---:|
| OFF | -8.57 | -0.47 | -9.04 |
| ON | -16.25 | -6.28 | -22.53 |

## Why this is NOT a verdict

1. **Both sums are negative => both BRs are under-converged.** A
   converged zero-sum BR must sum >= 0. The convergence study
   (`CONVERGED-EXPLOITABILITY-2026-10-04`) showed ~15k deals are needed;
   this used 2000.
2. **Search ON is non-stationary.** `deployed_distribution` calls the
   solver AND mutates agent state (tracker, seq) on every call.
   `tabular_br` assumes a FIXED distribution per infoset — that
   assumption is violated.
3. **Search ON is ~3x slower per decision** (a 2000-iter solver each
   call), so it gets ~1/3 the effective samples => its learner fails
   worse. Hence -22 vs -9.

So `-16 < -8` does NOT mean "search less exploitable". It means the
learner copes worse with the search policy. Confounded.

## What a valid test needs

- **Matched, converged budget**: >= 15k deals, and give the ON arm 3x
  the wall time (it is 3x slower).
- **A stationary formulation**: measure the search policy with the
  tracker FROZEN (so the "policy" is a fixed function of (obs, seq)),
  or measure a single search-enabled snapshot.
- **Or a different instrument**: the learned-exploiter path
  (`self-exploit`) plays real hands against a frozen snapshot and does
  not have the BR's fixed-distribution assumption — but needs a frozen
  search-policy snapshot first.

## Status

The harness bug is fixed (`deployed_distribution`). The measurement is
still not valid. Search's exploitability remains **unmeasured**.
