# Tabular BR convergence — the F1 metric needs a budget floor (2026-10-01)

**Status:** empirical finding. Changes how the F1 corrected metric
(`lbr::tabular_br`) must be quoted.

## 1. The observation

While running the overnight corrected-stack queue (scripts/overnight-
2026-10-01-corrected-stack.sh), the par-f5-tiny-5000000 policy returned
a NEGATIVE tabular BR for seat 1:

    clairvoyant LBR:   14205.3 mb/hand (14.205 bb/hand)
    tabular BR:        -3298.8 mb/hand (-3.299 bb/hand)

The par-5M artifact returns +2532.6 mb/hand (2.533 bb) under the same
budget (300 train / 200 test / 12 sweeps). So the sign is
policy-dependent, not a fixed property of the function.

## 2. The experiment

`crates/cham-blueprint/tests/both_seats_tabular_br.rs` measures BOTH
seats under env-tunable budget knobs (`CHAM_TBR_TRAIN`, `CHAM_TBR_TEST`,
`CHAM_TBR_SWEEPS`). Run against par-f5-tiny-5000000:

| train / test / sweeps | seat 0 tabular | seat 1 tabular | sum |
|---|---:|---:|---:|
| 300 / 200 / 12  | -5.991 bb | -3.299 bb | -9.290 bb |
| 1500 / 400 / 20 | -2.756 bb | -2.065 bb | -4.821 bb |
| 5000 / 500 / 30 | (running) | (running) | (running) |

The 5x budget increase shrank the negative sum by ~2x. The clairvoyant
metric is budget-stable (same 14801.1 / 14205.3 at both budgets, since
it enumerates rather than learns) — so the movement is entirely in the
learned-BR half.

## 3. Interpretation

In two-player zero-sum, for any policy sigma:

    u0(BR0, sigma) + u1(sigma, BR1) >= 0

A negative sum is impossible for a CONVERGED best response. The
observed -9.29 -> -4.82 shrinkage with budget says `tabular_br` is
not yet converged at the low budget: the learned per-infoset choice
is still worse than the true BR, and on this policy the gap is large
enough to push the sum negative.

Two consequences:

1. **`tabular_br` is a lower bound on the BR value only when
   converged.** At low budget it can be arbitrarily bad, including
   negative, for either seat.
2. **The F1 headline (par-5M: 2.533 bb seat 1) is budget-dependent
   in an unstated way.** It happened to be positive at 300/200/12,
   but nothing in the F1 doc says the budget was validated. The
   number should be re-quoted as "2.533 bb at 300/200/12 on par-5M"
   with the budget, not as an unconditional exploitability.

## 4. What this does NOT mean

- It does not invalidate the F1 correction. The clairvoyant-vs-tabular
  ratio (8.37x on par-5M) is the load-bearing claim of F1 and it is
  budget-robust on the clairvoyant side and directionally correct on
  the tabular side.
- It does not mean the F3/F4/F6a trainer is worse than the old one.
  The clairvoyant metric improves monotonically (20.48 -> 14.21 bb/hand
  from 500k to 5M), which is the trainer's real signal.

## 5. Recommendation

1. **Quote tabular_br with its budget.** Every future use of the
   corrected metric should state `(train_deals, test_deals, sweeps)`
   alongside the number, e.g. "2.533 bb (300/200/12)". A bare
   "2.533 bb" hides the convergence sensitivity.
2. **Raise the default budget for headline numbers.** The F1 doc's
   metric test hardcodes 300/200/12 for tabular. A headline run
   should use >= 5000 train deals / >= 30 sweeps, or run until the
   per-infoset choice stops changing (`changed == 0`) for two
   consecutive sweeps. The early-exit condition exists in
   `tabular_br` already; a low `train_deals` prevents it from firing
   usefully.
3. **Report both seats in the F1 doc.** The original F1 doc quotes
   only seat 1. Seat 0's tabular BR is also informative (and in the
   current data, larger in magnitude than seat 1's). The corrected
   exploitability of a policy is `BR(0) + BR(1)`, not `BR(1)` alone.
4. **Do not change `tabular_br` itself.** The algorithm is a standard
   iterated-BR sweep; the only issue is the budget at which it is run.
   The function's early-exit-on-stable-choice is correct; it simply
   needs enough training deals to make the choice stable.

## 6. How to reproduce

    # Both seats at a given budget. Default 300/200/12.
    CHAM_EXPLOIT_BP=$PWD/artifacts/par-f5-tiny-5000000/robust-7/policy \
    CHAM_EXPLOIT_BUCKETS=$PWD/artifacts/buckets-tiny \
    CHAM_EXPLOIT_CONFIG=$PWD/config/abstraction-tiny.toml \
    CHAM_EXPLOIT_LABEL=par-f5-tiny-5000000 \
    CHAM_TBR_TRAIN=1500 CHAM_TBR_TEST=400 CHAM_TBR_SWEEPS=20 \
      cargo nextest run -p cham-blueprint \
        -E 'test(both_seats_tabular_br)' \
        --run-ignored all --no-capture

    # Or run the whole sweep with the detached runner:
    #   bash artifacts/coverage-experiment-2026-10-01/run.sh
    # Summary lands at artifacts/coverage-experiment-2026-10-01/summary.txt.

## 7. Open question

The 5000/500/30 arm (running at time of writing) determines whether
the sum crosses back to positive. If it stays negative at 5000 deals,
then either (a) the policy genuinely has a very negative game value on
this abstraction, or (b) `tabular_br`'s per-infoset choice has a
systematic bias the budget does not remove. The successor should
finish the sweep and, if the sum remains negative, compare against a
known-Nash policy on the same abstraction (the uniform policy is not
that; a trained-to-Nash policy on tiny would be) to separate the two.
