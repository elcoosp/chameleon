# The mixture/argmax tradeoff (2026-09-28)

## The two metrics disagree

Same bundle (`artifacts/agent-honest`, 500k per expert, γ=1.0, synthetic-trained router):

| metric | full (= argmax) | full-mixture |
|---|---:|---:|
| **ladder** aggregate mb/seating | **+6 567** | +4 388 |
| **LBR** depth-100 seat 0 (mb/hand) | **31 032** | **20 447** |
| **LBR** depth-100 seat 1 (mb/hand) | **24 843** | **16 123** |

`<less is better>` for LBR (extractability). `<more is better>` for ladder
(our earnings vs the pool).

Argmax wins the ladder. **Mixture wins the LBR by ~35 %**. Neither
dominates.

## Why

* **Argmax** picks the highest-weight expert each decision and plays it
  purely. Against an opponent we have a good specialist for, that
  specialist's full strength is applied. But when the router's pick is
  wrong, there is no hedge — and against a Nash best responder (which the
  LBR measures), the *predictability* of "always play expert k" is a
  liability.
* **Mixture** blends per-decision. Against a specific archetype it
  includes some of the wrong experts' mass, which dilutes the specialist's
  edge. But the blend is harder to exploit — the LBR (which measures
  exploitability in the strict game-theoretic sense) reflects this.

Both are consistent with the theory: pure strategies maximize payoff
against a distribution over opponent types; mixed strategies minimize
worst-case exploitability. Which one we ship depends on who we're playing.

## Which matters

* **Against the archetype pool** (the ladder): argmax wins, and this is
  what "we beat the opponents we have tuned for" means.
* **Against a best-responder or unknown opponent**: mixture wins, and
  this is what "we don't get crushed by anyone" means.

For a ladder-based promotion gate we should ship argmax. For an
open-pool or adversarial scenario (e.g. Slumbot, or any evaluation where
we don't know the opponent's policy class) we should ship mixture.

## Recommendation

Keep the CLI split we already have (`full` = argmax, `full-mixture` =
mixture) and choose per evaluation context. The default should reflect
the *context we ship in*, not a metric preference:

* `ladder` runs against the archetype pool: `full` = argmax is correct.
* `slumbot`, `ab` against unknown arms, and any adversarial evaluation:
  use `full-mixture` for the safety.

## Measurement plumbing now in place

`ChameleonAgent::action_distribution(obs)` returns the exact distribution
`act_impl` would sample from — no sampling, no state advance. That is
what makes mixture LBR measurable. The measurement tool lives at
`/tmp/mixture_lbr/` (throwaway; the technique is documented here for
anyone who wants to reproduce).
