# F7 A/B result: mode-taking beats sampling against the scripted pool (2026-10-01)

## The measurement

Same bundle (`artifacts/agent-honest-19dim`, 500k experts + 500k robust
+ 19-dim honest calibrated router). Same ladder invocation. Only the
expert-decision mechanism differs:

| routing | decision | mean |
|---|---|---:|
| `argmax` | play expert k's MODE (`argmax_of(σ_k)`) | **+8 271** |
| `sample-expert` | sample expert k's MIXED strategy (`sample_index(σ_k, rng)`) | +7 034 |
| Δ | | **−1 236** |

Per-opponent:

| opponent | argmax (mode) | sample-expert | Δ |
|---|---:|---:|---:|
| arch:nit      | +2 377 | +1 712 | −665 |
| arch:tag      | +4 901 | +3 620 | −1 281 |
| arch:lag      | +7 715 | +6 472 | −1 243 |
| arch:station  | +14 015 | +12 750 | −1 265 |
| callbot       | +25 130 | +23 908 | −1 222 |
| jamfix        | +4 787 | +4 199 | −588 |
| pnash         | +4 585 | +4 161 | −424 |
| famB:tag      | +3 494 | +785 | **−2 709** |
| noisy:0.1:lag | +7 432 | +5 704 | −1 728 |
| **mean**      | **+8 271** | **+7 034** | **−1 236** |

**Every opponent loses.** The biggest loss is on `famB:tag` (−2 709) —
a family-B opponent whose tendencies are quite different from the
family-A training data. That is the exact case where the review
expected sampling to win.

## Why mode wins here (counter to review prediction)

The review argued sampling is more exploitable-resistant because
"a best-responder loses nothing to mixing." That is correct **against
a fully adaptive best responder**. But the archetype pool is scripted
and pure: each opponent plays a fixed policy with a fixed bluff
frequency. Against a *fixed* opponent:

- **Mode-taking exploits their fixed tendencies directly.** If TAG
  checks 52% on the flop, mode-taking picks bet (mode) and maximizes
  the immediate exploit.
- **Sampling dilutes the exploit.** The expert's mixed strategy
  already accounts for the scripted opponent's tendencies; playing
  the mode of that mix is a sharper response than sampling it.

Put another way: mode-taking is the argmax over the expert's already-
correct best response, while sampling is a randomized draw from that
distribution. Against a fixed opponent, the argmax exploit dominates
the randomized one in expectation.

## When sampling would matter

- **Against an adaptive opponent** (e.g. Slumbot, a real human
  exploiter, or a Nash-GTO bot): mode-taking is exploitable because
  the opponent can read the pure response and counter-exploit.
  Sampling preserves the mixed-strategy equilibrium and prevents
  this read.
- **Against a strong Best Response to the current strategy:**
  the theory applies.

The current pool has neither. `jamfix` and `pnash` are deterministic
scripts. `famB:tag` is scripted with a different prior. `noisy:0.1` is
a small perturbation.

## Recommendation

**Ship `--agent full` (argmax mode) as the default.** Keep
`sample-expert` available as a diagnostic / alternate for evaluation
against stronger opponents when they're added.

Note: this is an *empirical* result on the current pool; the theory
still favors sampling against any opponent that adapts. Document
clearly so a future session doesn't look at the ladder table and
conclude "sampling is bad" — sampling is the correct deployment when
the opponent type distribution includes adaptive play.

## Artifacts

- `artifacts/ladder-19dim-argmax.log`
- `artifacts/ladder-19dim-sample-expert.log`

## Related

- `COMPETITIVE-REVIEW-2026-10-01.md` — the review's F7 prediction
- `SYNTHETIC-ROUTER-IS-DEGENERATE-2026-09-30.md` — earlier mode-vs-mix confusion
