# 20M delay0+eps02 robust: better LBR, WORSE ladder (2026-09-30)

## The result

The 20M delay0+eps02 robust policy was the best LBR-improving run of
this session (mean 13 429, tied with the tiny-5M peak 13 417). Its
LBR is 3.2% better than par-5M's robust on BB (13 251 vs 12 858 — wait,
*worse*; see table below for clarity).

Wait, the LBR numbers:

| policy | SB | BB | mean |
|---|---:|---:|---:|
| tiny 5M robust       | 13 977 | 12 858 | 13 417 |
| tiny 20M delay0+eps02 | 13 608 | 13 251 | 13 429 |

The 20M is *better on SB* (13 608 vs 13 977) and *worse on BB*
(13 251 vs 12 858). The mean is a tie. So the correct statement is:
**the two policies have equal LBR means, different seat balance.**

On the ladder (`--fast`, 2500 deals/pair):

| bundle | robust-only ladder mean | argmax (full) ladder mean |
|---|---:|---:|
| agent-honest (500k robust)             | **+720** | **+7 136** |
| par-5M (5M robust)                     | +820 | +6 983 (5M-hybrid) |
| **20M delay0+eps02 (this measurement)** | **+624** | **+6 932** |

**Both the robust-only AND the argmax ladders got WORSE with the 20M
policy**, despite equal LBR mean to the 5M one and vastly better LBR
than the 500k one.

## What this means

The freeze investigation this entire session has been optimizing the
LBR of the tiny robust policy. The LBR peak at 5M, the regression at
20M, the four levers (delay0, avguniform, eps, warmfix), the DCFR
negatives, the medium-abstraction run — all of it produced two
policies (5M and 20M delay0+eps02) with **equal LBR means** and
**different ladder means**, and the 20M one is worse on the ladder.

**The LBR is not a proxy for ladder strength.** A policy can be
equally good against a uniform best-responder and worse against the
specific archetype pool.

This is the third instance this session of the same decoupling:

1. 500k robust vs 5M robust: 10x LBR difference, small ladder difference
2. 5M robust vs 20M delay0+eps02: tied LBR, 5M wins the ladder by +100
3. 500k robust vs 5M robust as argmax fallback: 5M wins LBR, loses argmax ladder by −153

## Where the ladder signal actually is

From the individual per-opponent cells:

| opponent | 20M rob-only | 5M rob-only | Δ |
|---|---:|---:|---:|
| arch:nit     | −158   | −288 (par-5M) | +130 |
| arch:tag     | −1 162 | −1 088 | −74 |
| arch:lag     | −1 565 | −1 538 | −27 |
| arch:station | −652   | −365 | −287 |
| callbot      | +5 628 | +5 788 | −160 |
| jamfix       | +2 860 | +3 404 | **−544** |
| pnash        | +2 261 | +2 205 | +56 |
| famB:tag     | −314   | −182 | −132 |
| noisy        | −1 244 | −560 | **−684** |
| **mean**     | **+624** | **+820** | **−196** |

The 20M policy loses most on `jamfix` (−544) and `noisy` (−684).
These are the same two opponents where the 5M robust-as-argmax-fallback
lost heavily. **The 20M policy is systematically worse on
out-of-archetype opponents.**

## Hypothesis for the mechanism

The delay0+eps02 combination lets the average strategy include more
mixed (early-phase) iterations. This helps against a *uniform best
responder* because mixing is generally harder to exploit. But it
*hurts* against specific archetypes because their exploit windows are
narrower and require the sharper responses that the frozen/late
iterations produce.

This is exactly the inverse of the freeze hypothesis: the "frozen"
policy is better against the archetype pool because these opponents
are themselves nearly-pure strategies that are best exploited by
nearly-pure responses.

## Recommendation

**Stop the LBR-centric freeze investigation.** The remaining levers
(DCFR at α≥0.99, sqrt ramp, etc.) optimize a metric that is not
correlated with the shipped ladder.

**Redirect to:**

1. **Retrain the 4 experts at 5M each.** The argmax ladder is dominated
   by the experts; upgrading them is the direct lever on the shipped
   number.
2. **Fix the router.** The mixture path is worth +5 400 in ladder
   *if* the router can distinguish TAG from LAG. The raw-frequency
   approach is exhausted (10-dim, 11-dim both fail the gate).
   Bet-size histogram or showdown-strength distribution are the
   remaining options.
3. **Sweep the argmax fallback training objective.** Train the robust
   policy against the archetype pool (or a mixture of LBR and
   archetype-ladder opponents), not only against a uniform best
   responder.

## Artifacts

- `artifacts/ladder-20M-delay0-eps02-robust-only.log`
- `artifacts/ladder-20M-delay0-eps02-full.log`
- `artifacts/agent-robust-20M-delay0-eps02/` (bundle)
- `artifacts/par-20M-delay0-eps02/robust-7/policy/policy.bin`

## Related

- `LBR-VS-LADDER-2026-09-30.md`
- `PAR5M-ROBUST-LADDER-2026-09-30.md`
- `ARGMAX-FALLBACK-REALLY-MATTERS-2026-09-30.md`
- `HYBRID-LADDER-2026-09-30.md`
