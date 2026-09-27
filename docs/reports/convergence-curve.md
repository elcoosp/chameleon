# Convergence curve — LBR vs training iterations (v7 Item 2)

> Status: infrastructure landed; full 100M sweep NOT yet run (multi-hour
> wall-clock). This doc records the honest baseline, the new checkpoint
> machinery, and the exact command loop for the sweep, plus the decision
> rule. Update the table below when the sweep lands.

## Honest baseline (measured, post-RBP-fix `fe84467`)

| iters | seat0_lbr | seat1_lbr | seat_mean_lbr | source |
|---|---|---|---|---|
| 3,000,000 | 36746.62 | 16536.3 | 26641.46 | `honest-lbr-tiny-3M-s7` (ledger) |
| 3,000,000 (uniform) | 40413.89 | 35099.54 | 37756.72 | same ledger row, reference |
| 10,000,000 | — | — | — (bundle `artifacts/nopruning-diag/theta-inf-10M-s7/robust-7` exists; LBR bench pending) | — |

Verdict so far: 3M honest iters is only 29% below uniform-random
(26,641 vs 37,757 mb/hand) — under-training, not tuning. Do not interpret
any downstream lever (DCFR, EMD, search, router) until the curve below exists.

## Checkpoint machinery (landed this session)

- `TrainerConfig.checkpoint_every` + `checkpoint_dir` (`crates/cham-blueprint/src/trainer.rs`):
  every N iters writes `<dir>/iter-<t>/table.snap` (self-contained).
- CLI: `train-bp --checkpoint-every 5000000` (writes `<out>/checkpoints/`).
- Cache key unchanged (checkpoints don't affect the artifact).

## Sweep loop (run when compute is available)

```bash
cargo run -q -p cham-cli -- train-bp --mode robust \
    --buckets artifacts/buckets-tiny --iters 100000000 --seed 7 \
    --thread-mode deterministic --checkpoint-every 5000000 \
    --out artifacts/checkpoints/robust-100M-s7
for ckpt in artifacts/checkpoints/robust-100M-s7/checkpoints/iter-*; do
  n=$(basename "$ckpt")
  cargo bench -p cham-blueprint --bench exploitability -- \
      --bundle "$ckpt" --save-baseline "convergence-$n"
done
```

Append results to the table above + a ledger `bench` row
`convergence-sweep-100M-s7` following the `honest-lbr-tiny-3M-s7` schema,
then apply the three-way verdict from the runbook (still-dropping →
scale further; flat-above-competitive → prioritize Item 4 EMD; flat-near →
Items 3/5/6).
