# First competitive result (2026-09-28)

## The ladder

`chameleon ladder --agent full --fast` on the retrained `artifacts/agent-honest`
bundle (500k iters per expert, tiny abstraction, γ=1.0 averaging):

| opponent | mb/seating | bb/100 | VR |
|---|---:|---:|---:|
| arch:nit | +1 692 | +8.5 | ×1.46 |
| arch:tag | +2 501 | +12.5 | ×1.51 |
| arch:lag | +4 360 | +21.8 | ×1.31 |
| arch:station | +4 366 | +21.8 | ×1.22 |
| callbot | +10 157 | +50.8 | ×1.18 |
| jamfix | −449 ± 632 | −2.2 | ×0.07 (noise) |
| pnash:overfold:0.15 | +353 | +1.8 | ×2.49 |
| famB:tag | +1 186 | +5.9 | ×1.70 |
| noisy:0.1:arch:lag | +3 310 | +16.6 | ×1.79 |

**8/9 wins.** Fallback rate 0.6% (was 100% pre-L-1, 0.5% on the previous
γ=0.9 bundle).

## What changed to get here

1. **L-1 key-format break** (2026-09-27): the overflow byte in the key stream
   invalidated every pre-existing artifact. This retrain uses the current key
   format. It is the first loadable bundle since that fix.
2. **γ-underflow fix** (2026-09-28): `avg_gamma` default 0.9 → 1.0 in
   `cham_blueprint::default_avg_gamma` and both CLI defaults. Worth 40% on
   robust-alone seat 0 (39 692 → 23 280 mb/hand LBR).

## The two numbers and how to read them

**Ladder (this table):** what our policy EARNS against a fixed opponent.
Positive = we win money.

**LBR (best-response, 200 deals):** what a PERFECT opponent extracts FROM
our policy. For the same bundle:
- robust-alone at 500k iters, γ=1.0: 23 280 / 13 957 mb/hand
- The mixture is not directly measurable in the LBR harness because
  `ChameleonAgent` doesn't expose `action_probs` with the internal seq
  (the mixture LBR tool reads uniform — this is a plumbing gap, not a bug
  in the agent).

Both can be true simultaneously: we beat archetypes by 1–4 bb/seating AND
a Nash best-responder still extracts 23 bb/hand. The archetype wins are
the practical signal; the LBR is the equilibrium-gap signal.

## The jamfix VR anomaly

`VR ×0.07` on jamfix means the all-in-EV variance-reduction adjustment
made the estimate WORSE. jamfix is a pure all-in bot; every hand reaches
all-in runout, and H-11's adjustment fires on every deal. A VR factor
below 1.0 says the "replacement" is adding variance rather than removing
it — worth investigating, but jamfix is a single-opponent corner case.
The `±632` CI covers zero, so the −449 point estimate is essentially noise.
