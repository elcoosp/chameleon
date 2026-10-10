# W2 verdict — skeleton (fill when real-full D1 lands)

Two W2 mechanisms have trained bundles. The river slice said only one
matters:

| bundle | river slice (bb) | full-game D1 (bb) |
|---|---|---|
| tiny-full | 5.890 | 10.7319 +/- 0.6206 (180 bd) |
| rlf-g2 | 5.926 | (rlf, not re-run) |
| rlf-cfr | 5.956 | — |
| rlf-12m | 5.885 | — |
| real-full (128/64) | 5.870 | **pending** |
| real-preflop | 11.343 | D1 unreliable (91% miss) |

**If real-full full-game D1 ~ 8.5 (matching agent-honest-19dim):**
bucket fineness does not move the full-game number. W2's "more
buckets" is not the lever; the preflop tree is (per the real-preflop
river outlier).

**If real-full full-game D1 < 8.5 by > 3 SE:** bucket fineness helps
and W2's bucket mechanism is validated.

Fill in the pending cell from `artifacts/d1-real-full-60.log`.
