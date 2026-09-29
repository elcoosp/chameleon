# DCFR alpha=0.9 un-freezes the iterate — and produces a terrible policy (2026-09-29)

## The result

`--regret-discount 0.9` discounts every positive regret by 0.9 before
adding the new delta, at every iteration. This is Brown & Sandholm 2019
DCFR with the "positive" alpha only.

1000-deal LBR at depth 100, lower is better:

| variant | SB | BB | mean |
|---|---:|---:|---:|
| tiny 20M no-fix    | 13 319 | 14 706 | 14 012 |
| tiny 20M warmfix   | 13 554 | 14 090 | 13 822 |
| tiny 20M delay0    | 13 431 | 13 618 | 13 524 |
| tiny 20M avguniform| 13 976 | 13 133 | 13 555 |
| **tiny 20M alpha=0.9** | **35 344** | **25 834** | **30 589** |
| tiny 5M peak (1000 deals) | 13 977 | 12 858 | 13 417 |

Alpha=0.9 is **2.5x worse than any other 20M variant.** On both seats.

## What the freeze diagnostic says about the same table

        rows (w>=2):      21416
        frozen (one-hot): 7126 (33.3%)
        near-frozen (.9+):693 (3.2%)
        soft (<.5):       3563 (16.6%)
        all-zero-regret:  11 (0.1%)
        --- AVERAGE strategy ---
        avg_near_frozen:  2549 (11.9%)
        mean cur max_p:   0.734
        mean avg max_p:   0.616

Compare against the no-fix 20M diagnostic:

| metric | no-fix 20M | alpha=0.9 | direction |
|---|---:|---:|---|
| soft (<0.5) | 3.3% | 16.6% | **+13.3 pts — much softer** |
| avg_near_frozen (≥0.9) | 60.0% | 11.9% | **−48 pts — much less frozen** |
| mean cur max_p | 0.871 | 0.734 | **−0.137 — much less sharp** |
| mean avg max_p | 0.859 | 0.616 | **−0.243 — much less sharp** |

**The discount does exactly what it was designed to do.** The current
iterate is less one-hot, the average strategy is less concentrated,
and the frozen-row fraction is 5x lower. The RM+ freeze has been
largely defeated.

## The paradox

**Un-freezing did not help. It made things dramatically worse.** The
table now records a policy that plays more mixed actions, but the
policy is worse than the frozen one by 2.5x.

The mechanism: DCFR's positive-regret discount attenuates the
accumulated regret signal itself. After ~20M iterations, the effective
window is roughly `1/(1-0.9) = 10` iterations. The regret sum is
essentially the last 10 iterations' worth of deltas — noise — and the
strategy is a fresh RM+ solve on that noise. So the policy plays
near-uniformly across actions whose true regrets are very different.

**Un-freezing to random is worse than freezing to a decent approximate
equilibrium.** That is what alpha=0.9 shows. The freeze was producing
a nearly-pure strategy on a game where the equilibrium is *nearly
pure* on many rows — the sharpening wasn't wrong, it was just wrong on
the BB-important rows. Discarding the entire accumulated signal to
force mixing destroys the SB rows that were legitimately sharpening.

## What this rules out

- DCFR alpha<1 at this discount rate is not a usable lever for the
  tiny-abstraction freeze. The un-freeze works at the policy-cost of
  everything else.
- Softer discounts (alpha=0.99) might thread the needle — barely
  attenuating regret while slowly re-opening the frozen rows. Untested.
  The half-life at alpha=0.99 is ~100 iterations instead of 10, so
  much more of the accumulated signal survives. That is the next
  experiment worth running, if the freeze story is still the priority.
- **The exploration floor (eps=0.02) is fundamentally better than
  DCFR for this game.** The floor lets the current iterate stay
  slightly mixed while *keeping* the accumulated regret signal intact.
  DCFR replaces the signal.

## The decoupling this reveals

The schedule fixes (delay0, avguniform) and the warmup fix all improve
BB while (slightly) hurting SB — a small, controlled tradeoff. DCFR
alpha=0.9 makes the freeze diagnostic much better and the LBR much
worse. So "**the freeze is the problem**" was too simplistic. The
freeze is a *symptom* of proper RM+ convergence on rows whose true
equilibria are nearly pure. The BB regression comes from a small set
of rows where the equilibrium needs mixing, and those rows need a
*targeted* lever (schedule weight, per-row exploration), not a
*global* regret discount.

The right mental model is: the freeze is **correct on the rows where
it happens and wrong on the rows where the LBR cares about it**. DCFR
fixes the wrong ones and breaks the right ones.

## Artifacts

- `artifacts/par-20M-alpha09/robust-7/policy/policy.bin`
- `artifacts/par-20M-alpha09/robust-7/table.snap`       (765 766 bytes)
- `artifacts/par-20M-alpha09/robust-7/provenance.json`
- `artifacts/par-20M-alpha09-lbr.log`
- `artifacts/par-20M-alpha09-lbr.stderr.log`
- `artifacts/par-20M-alpha09-freeze.txt`
- `artifacts/par-20M-alpha09.log`

## Repro

    target/release/chameleon train-bp \
      --mode robust --iters 20000000 --depth 100 --seed 7 \
      --config config/abstraction-tiny.toml \
      --buckets artifacts/buckets-tiny \
      --out artifacts/par-20M-alpha09 \
      --threads 4 --thread-mode hogwild \
      --regret-discount 0.9

    /tmp/rm_freeze/target/release/rm_freeze \
      artifacts/par-20M-alpha09/robust-7/table.snap

## Related

- `RM-PLUS-FREEZE-2026-09-29.md` — the freeze diagnostic itself
- `AVG-DELAY-DELAY0-RESULT-2026-09-29.md` — the schedule fix that DID help
- `AVG-UNIFORM-RESULT-2026-09-29.md` — the strongest schedule fix
- `WARMUP-FIX-RESULT-2026-09-29.md` — the parallel trainer warmup fix
