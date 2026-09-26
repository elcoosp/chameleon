# Bench status snapshot

Source: `cargo run -q -p cham-cli -- verify --perf` on M1, target-cpu=native.

| Gate | Bench | Threshold | Measured | Over | Plan M1 baseline |
|------|-------|-----------|----------|------|------------------|
| P1 | eval_evaluate7 | 10 µs | 29.58 µs | 3.0x | 55 µs |
| P2 | engine_apply | 200 µs | 1.12 ms | 5.6x | 5.30 ms |
| P3a | encode_flop | 100 µs | 6.26 µs | OK | — |
| P3b | encode_river | 100 µs | 698 ns | OK | — |
| P4 | mccfr_iter_200bb_tiny | 2.00 ms | 2.45 ms | 1.2x | 11.8 ms |
| P5 | solve_rnr_400 | 7.00 ms | 11.76 ms | 1.7x | 45.65 ms |
| P6 | match_20_deals | 30 µs | 47.50 µs | 1.6x | 26 µs |
| P7 | decision_latency | 1 ms | 4.62 µs | OK | — |
| P8 | decision_latency_search | 50 ms | 13.98 ms | OK | — |

## Notes

- P6 is the only regression vs plan baseline (26 → 47.5 us). Likely the B1
  per-hand lifecycle cost (HandHistory build + on_hand_end) the harness now
  pays. Re-measure before attributing to a regression.
- B5 cache cannot move P5: 400 iters of CFR+ at ~29 us/iter dominate a
  ~5 us build. Cache's payoff is on the trigger path where a board class
  recurs; add a trigger bench to see it.

## Saved criterion baseline

`post-b1-b11` (via `cargo bench --workspace -- --save-baseline post-b1-b11`).
Future deltas: `cargo bench --workspace -- --baseline post-b1-b11`.
