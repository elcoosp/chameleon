# LBR and ladder disagree: 500k robust beats 5M robust on the archetype pool (2026-09-30)

## Two robust policies, same abstraction, same seed

| policy | iters | abstraction | LBR (1000 deals, D100) |
|---|---:|---|---|
| `artifacts/agent-honest/robust/policy.bin` | 500k | tiny | 23 280 / 13 957 (200 deals) |
| `artifacts/par-5M/robust-7/policy/policy.bin` | 5M | tiny | 13 977 / 12 858 (1000 deals) |

The 5M policy is unambiguously better on LBR: SB improves by 9 303,
BB by 1 099. It is the LBR SOTA (per `SOTA-2026-09-28.md` and every
handoff since).

## Both measured on the same ladder

`ladder --fast` (2500 deals/deal-pair, 9 opponents, tiny abstraction):

| opponent | 500k robust (agent-honest) | 5M robust (par-5M) | winner |
|---|---:|---:|---|
| arch:nit      | −761.4   | −288.2  | **5M by +473** |
| arch:tag      | −921.5   | −1 088.4 | 500k by +167 |
| arch:lag      | −1 352.2 | −1 538.2 | 500k by +186 |
| arch:station  | +231.8   | −364.9  | 500k by +597 |
| callbot       | +7 230.6 | +5 787.5 | 500k by +1 443 |
| jamfix        | +4 265.7 | +3 404.0 | 500k by +862 |
| pnash         | +2 646.4 | +2 205.2 | 500k by +441 |
| famB:tag      | +583.2   | −181.6  | 500k by +765 |
| noisy:0.1:lag | −636.4   | −559.7  | 5M by +77 |
| **mean**      | **+1 698** | **+1 175** | **500k by +523** |

**The 500k robust policy is better on the archetype ladder by ~500
mb/seating mean, while being 10x worse on LBR.** Both are the same
abstraction, seed, mode, and topology. The only difference is training
budget.

## Why this matters

The entire freeze investigation this session
(`RM-PLUS-FREEZE-2026-09-29.md`, `AVG-DELAY-DELAY0-RESULT-2026-09-29.md`,
`AVG-UNIFORM-RESULT-2026-09-29.md`, `DELAY0-EPS02-RESULT-2026-09-30.md`)
is in service of making the *LBR* of the tiny-5M-then-20M robust policy
better. But the ladder is what measures play against the opponent pool
that actually matters for the shipped product.

If the 500k robust beats the 5M robust on the ladder, then **the
training-budget work is a bet against LBR, not against the ladder**.
The LBR peak at 5M and the LBR regression at 20M could be entirely
orthogonal to how good the shipped policy is.

This is not unprecedented in the literature: LBR and empirical
performance are different objectives. LBR is a 1-vs-exploiter bound;
the ladder is 1-vs-scripted-pool. Policies that exploit a uniform
random best responder may not exploit specific archetypes.

## The shipping picture, corrected

The 09-28 SOTA doc recommends `agent-honest` with `--agent full` (argmax
routing across 4 experts). That uses the 500k-iters robust as the
fallback AND four 500k-iters experts (nit, tag, lag, station). The
robust policy is not the shipping choice for that bundle — the four
experts are.

The `par-5M` bundle is a *robust-only* bundle. It has no experts. It
was constructed for LBR work, not for ladder measurement.

**The correct LBR-vs-ladder comparison is 500k robust vs 5M robust,
which we now have:** the older 500k one wins.

## Follow-up questions the next session should answer

1. **Does retraining the 4 experts at 5M improve the ladder?** If the
   aggregate +6 567 comes from the mixture, and the experts are only
   500k, then upgrading them is the natural ladder move.

2. **Does the par-5M robust policy as the fallback of the mixture make
   the ladder better or worse than the 500k robust fallback?** The
   fallback is what gets played on low-confidence rows; if the 5M
   fallback is worse on the ladder (per this measurement), then the
   mixture's fallback should stay at 500k.

3. **Is LBR even the right target for the training-budget work?** The
   `--agent full` ladder of the 500k bundle (+6 567) is the actual SOTA.
   The 20M delay0+eps02 LBR work (13 429 mean) is LBR-improving. If
   the fallback role is minor, the LBR work is off-target.

4. **The 09-28 handoffs quantify the routing lever as +3 400** (mixture
   no-router +3 184 → argmax+synthetic +6 567). The current session
   has spent hours on the LBR lever which is <500. So routing is
   probably 10x more leveraged than training-budget.

## Artifacts

- `artifacts/ladder-robust-honest.log`  (500k robust)
- `artifacts/ladder-robust-par5m.log`   (5M robust)
- `artifacts/par-5M/robust-7/policy/`   (5M policy)
- `artifacts/agent-honest/robust/`      (500k policy)
