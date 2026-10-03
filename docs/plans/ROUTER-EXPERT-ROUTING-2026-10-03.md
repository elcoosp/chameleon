# Router → expert routing, per opponent (2026-10-03)

> **CORRECTION (2026-10-03, later):** the "router is degenerate" claim
> below is **WRONG**. It was a measurement artifact: the 99.9% case was a
> *nit-only* pool (where concentrating on the nit expert is CORRECT), and
> the probe's per-opponent rows came from its shared-bot design. On the
> real 9-opponent ladder the router distributes healthily:
> `e0=10845 e1=39902 e2=28907 e3=41622`. The router is NOT degenerate.
> See `ROUTER-EXPERT-ROUTING-2026-10-03.md` correction and
> `docs/plans/SYNTHETIC-ROUTER-IS-DEGENERATE-2026-09-30.md` (which was
> about the *synthetic* router, a different artifact). The guard now
> exempts single-opponent pools.


Measured with `probe --diag-fallback` after adding an `argmax_pick[4]`
counter (commit `6db0256`). The counter reads `last_trace.argmax_k`.

## Data

| pool | opponent | argmax picks [e0 e1 e2 e3] |
|---|---|---|
| jamfix-only | jamfix | [0 0 0 **80**] → expert 3 |
| full | arch:nit | [**134** 0 0 18] → expert 0 |
| full | arch:tag | [**148** 0 0 0] → expert 0 |
| full | arch:lag | [**143** 0 0 0] → expert 0 |
| full | arch:station | [**153** 0 0 0] → expert 0 |
| full | jamfix | [**80** 0 0 0] → expert 0 |
| full | pnash:overfold | [**108** 0 0 0] → expert 0 |
| full | famB:tag | [**150** 0 0 0] → expert 0 |

Expert index → archetype (bundle convention): 0=nit, 1=tag, 2=lag,
3=station.

## Findings

1. **The router is heavily degenerate**: in the full pool, 8 of 9
   opponents route to expert 0 (nit). This is the
   `SYNTHETIC-ROUTER-IS-DEGENERATE-2026-09-30` finding, quantified — the
   router is not dispatching to specialists at all in practice.
2. **jamfix routes to expert 0 in the full-pool context** (the context
   the ladder uses), but to expert 3 in the jamfix-only context. The
   difference is tracker state: `probe` shares one bot across
   opponents, so the accumulated stats change the routing. The
   full-pool number is representative of ladder play.
3. **jamfix has an all-experts-miss problem**: 40/80 decisions had
   `expert_miss[k]` true for all four k, so the action fell back to
   robust. jamfix is partly a coverage/off-tree problem, not purely a
   policy-quality problem.

## Consequence for the jamfix-mixture experiment

The running experiment trains **expert 0** (nit) against
`mix:0.8:arch:nit~jamfix`. In the full-pool context expert 0 is the one
that handles jamfix, so the experiment is **correctly aimed**. If it
improves the full-bundle jamfix ladder number, the approach works.

## Caveat

The shared-bot probe design means per-opponent routing carries
accumulated tracker state. A clean per-opponent routing measurement
would reset the bot between opponents; `probe` does not. This does not
affect the conclusion (the ladder plays all opponents sequentially, so
the "accumulated" routing is the real one), but it means the
jamfix-only row should not be read as "jamfix is intrinsically routed
to expert 3".

Source: `artifacts/routing-check-2026-10-03/summary.txt`.
